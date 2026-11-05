// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The scaffolding of the Gmail scenario suite: one suite runs against
//! a transcript through the replayers, against a server as it stands
//! and, with the recorders in between, to write such a transcript. The
//! sends of the suite go to the SMTP server of the connection.

pub mod corpus;
pub mod gmail;
pub mod scenario;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use huliho_imap_bridge::blob;
use huliho_imap_bridge::jmap::{
    CORE_CAPABILITY, HULIHO_CAPABILITY, MAIL_CAPABILITY, QUOTA_CAPABILITY, SUBMISSION_CAPABILITY,
    handle,
};
use huliho_imap_bridge::mailboxes::{self, SyncError};
use huliho_imap_bridge::runtime::{Link, Timing};
use huliho_imap_bridge::session::{ImapSession, Session, TlsMode};
use huliho_imap_bridge::store::{AccountKey, MailboxRow, Store};
use huliho_imap_bridge::sync::{Cache, FolderSync, Step};
use huliho_imap_bridge::testing::imap::HOST;
use huliho_imap_bridge::testing::record::fixed::RECORDED_USER;
use huliho_imap_bridge::testing::record::{self, Upstream};
use huliho_imap_bridge::testing::seal::TestSealer;
use huliho_imap_bridge::testing::{
    Fake, PASSWORD, Protocol, Recorder, Replayer, SmtpRecorder, SmtpReplayer, Submitting,
    TestConnector, Transcript, replay, smtp,
};
use serde_json::{Value, json};

/// The one account of the suite.
pub const ACCOUNT: &str = "gmail";

/// The sessions one folder may cost before the suite gives up.
const MAX_SESSIONS: usize = 20;

/// A limit above every blob of the corpus.
const BLOB_LIMIT: u64 = 1024 * 1024;

/// Longer than any run of the suite, so a recording never signs its
/// conversation in afresh for a pass and the replay opens the
/// connections the recording did.
const LIST_AGE: Duration = Duration::from_hours(1);

/// How the bridge reaches the account: the IMAP and the SMTP server with
/// the trust each is checked against, who signs in, the address the
/// account's identity sends from and how long one step may take.
#[derive(Clone)]
pub struct Connection {
    pub imap: Upstream,
    pub smtp: Upstream,
    pub user: String,
    pub password: String,
    pub address: String,
    pub step: Duration,
}

/// What stands between the bridge and the servers.
enum Server {
    Replaying { imap: Replayer, smtp: SmtpReplayer },
    Recording { imap: Recorder, path: PathBuf },
    Direct,
}

/// The cache, the link and the way to the servers.
pub struct Suite {
    pub cache: Cache,
    pub link: Arc<Link<Submitting>>,
    connection: Connection,
    server: Server,
}

/// A scripted server as the bridge reaches it, under the name its
/// certificate carries.
fn through<P: Protocol>(fake: &Fake<P>, host: &str) -> Upstream {
    Upstream {
        tls: fake.trusting(),
        target: fake.target(host, TlsMode::Implicit),
    }
}

impl Suite {
    /// Against the transcript through the replayers, signed in as the
    /// recorded user.
    pub async fn replaying(mut transcript: Transcript, step: Duration) -> Self {
        let sends = std::mem::take(&mut transcript.smtp);
        let smtp = SmtpReplayer::start(replay::smtp::Script::new(sends)).await;
        let imap = Replayer::start(replay::Script::new(transcript)).await;
        let connection = Connection {
            imap: through(&imap, HOST),
            smtp: through(&smtp, smtp::HOST),
            user: RECORDED_USER.to_owned(),
            password: PASSWORD.to_owned(),
            address: RECORDED_USER.to_owned(),
            step,
        };
        Self::new(connection, Server::Replaying { imap, smtp })
    }

    /// Against the servers the connection names; through the recorders
    /// when a path to write the transcript to is given.
    pub async fn against(connection: Connection, record: Option<PathBuf>) -> Self {
        let Some(path) = record else {
            return Self::new(connection, Server::Direct);
        };
        let imap = Recorder::start(record::Script::new(connection.imap.clone())).await;
        // The SMTP recorder writes into the same recording; its listener
        // serves on after this handle is gone.
        let smtp = SmtpRecorder::start(imap.script().smtp(connection.smtp.clone())).await;
        let through = Connection {
            imap: through(&imap, HOST),
            smtp: through(&smtp, smtp::HOST),
            ..connection
        };
        Self::new(through, Server::Recording { imap, path })
    }

    fn new(connection: Connection, server: Server) -> Self {
        let inner = TestConnector::Server {
            tls: Arc::clone(&connection.imap.tls),
            target: connection.imap.target.clone(),
            step: connection.step,
            user: connection.user.clone(),
            password: connection.password.clone(),
        };
        let connector = Submitting {
            inner,
            tls: Arc::clone(&connection.smtp.tls),
            smtp: connection.smtp.target.clone(),
            step: connection.step,
        };
        let timing = Timing {
            interval: Duration::ZERO,
            list_age: LIST_AGE,
            ..Timing::default()
        };
        Self {
            cache: Cache {
                store: Arc::new(Store::in_memory().unwrap()),
                sealer: Arc::new(TestSealer::default()),
                key: AccountKey::new(ACCOUNT),
                gmail: true,
                address: connection.address.clone(),
                uploads: huliho_imap_bridge::testing::uploads(),
            },
            link: Arc::new(Link::with_timing(connector, timing)),
            connection,
            server,
        }
    }

    /// The address the account's one identity sends from.
    pub fn address(&self) -> &str {
        &self.connection.address
    }

    /// One blob read whole, as text.
    pub async fn blob(&self, blob_id: &str) -> String {
        let mut blob = blob::open(&self.cache, &self.link, blob_id, BLOB_LIMIT)
            .await
            .unwrap();
        let mut bytes = Vec::new();
        while let Some(chunk) = blob.next().await {
            bytes.extend(chunk.unwrap());
        }
        String::from_utf8(bytes).unwrap()
    }

    /// Whether the answers come from a transcript, where a body text
    /// reads as filler.
    pub fn is_replay(&self) -> bool {
        matches!(self.server, Server::Replaying { .. })
    }

    /// A session past LOGIN, the bridge's own.
    pub async fn session(&self) -> ImapSession {
        let connection = &self.connection;
        let mut session = ImapSession::connect(
            Arc::clone(&connection.imap.tls),
            &connection.imap.target,
            connection.step,
        )
        .await
        .unwrap();
        session
            .login(&connection.user, &connection.password)
            .await
            .unwrap();
        session
    }

    /// One mailbox pass on a session of its own.
    pub async fn pass(&self) -> u64 {
        let mut session = self.session().await;
        let state = mailboxes::sync(&mut session, &self.cache).await.unwrap();
        session.logout().await.unwrap();
        state
    }

    /// The mailbox rows as they stand.
    pub fn rows(&self) -> Vec<MailboxRow> {
        self.cache
            .store
            .mailbox_snapshot(&self.cache.key)
            .unwrap()
            .rows
    }

    /// The row of the mailbox with that role.
    pub fn by_role(&self, role: &str) -> MailboxRow {
        self.rows()
            .into_iter()
            .find(|row| row.facts.role.as_deref() == Some(role))
            .unwrap_or_else(|| panic!("no mailbox carries the role {role}"))
    }

    /// The row of the mailbox with that name as the account shows it.
    pub fn by_name(&self, name: &str) -> MailboxRow {
        self.rows()
            .into_iter()
            .find(|row| row.facts.name == name)
            .unwrap_or_else(|| panic!("no mailbox is named {name}"))
    }

    /// The state of the account.
    pub fn state(&self) -> u64 {
        self.cache.store.state(&self.cache.key).unwrap()
    }

    /// Runs a folder to its end the way a host does: after a failure a
    /// fresh session resumes the same sync.
    pub async fn sync(&self, folder: &MailboxRow) -> Step {
        let mut session = self.session().await;
        let mut sync = FolderSync::open(&mut session, &self.cache, folder)
            .await
            .unwrap()
            .expect("a store opens a sync");
        let mut sessions = 1;
        loop {
            match sync.finish(&mut session, &self.cache).await {
                Ok(step) => {
                    session.logout().await.unwrap();
                    return step;
                }
                Err(SyncError::Session(_)) if sessions < MAX_SESSIONS => {
                    session = self.session().await;
                    sessions += 1;
                    assert!(sync.resume(&mut session).await.unwrap());
                }
                Err(other) => panic!("{other}"),
            }
        }
    }

    /// The Response object to one call under every capability.
    pub async fn response(&self, call: &Value) -> Value {
        let body = json!({
            "using": [
                CORE_CAPABILITY,
                MAIL_CAPABILITY,
                QUOTA_CAPABILITY,
                SUBMISSION_CAPABILITY,
                HULIHO_CAPABILITY,
            ],
            "methodCalls": [call],
        });
        let bytes = handle(
            &self.cache,
            &self.link,
            &serde_json::to_vec(&body).unwrap(),
            ACCOUNT,
        )
        .await
        .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// The arguments of the first response to one call.
    pub async fn call(&self, call: &Value) -> Value {
        let mut response = self.response(call).await;
        response["methodResponses"][0][1].take()
    }

    /// The end of a run: a replay must have gone as recorded on both
    /// wires, a recording is redacted, scanned and written.
    pub fn finish(self) {
        match self.server {
            Server::Replaying { imap, smtp } => {
                let (imap, smtp) = (imap.script(), smtp.script());
                assert_eq!(imap.mismatches(), Vec::<String>::new());
                assert_eq!(smtp.mismatches(), Vec::<String>::new());
                assert_eq!(imap.unplayed(), 0, "recorded connections nobody opened");
                assert_eq!(smtp.unplayed(), 0, "recorded sends nobody opened");
            }
            Server::Recording { imap, path, .. } => {
                let transcript = imap
                    .script()
                    .finish()
                    .unwrap_or_else(|findings| panic!("the recording is not clean: {findings:#?}"));
                std::fs::write(&path, transcript.to_json()).unwrap();
            }
            Server::Direct => {}
        }
    }
}
