// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The scaffolding of the Gmail scenario suite: one suite runs against
//! a transcript through the replayer, against a server as it stands and,
//! with the recorder in between, to write such a transcript.

pub mod corpus;
pub mod gmail;
pub mod scenario;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use huliho_imap_bridge::jmap::{CORE_CAPABILITY, HULIHO_CAPABILITY, MAIL_CAPABILITY, handle};
use huliho_imap_bridge::mailboxes::{self, SyncError};
use huliho_imap_bridge::runtime::Link;
use huliho_imap_bridge::session::{ImapSession, Session, Target, TlsMode};
use huliho_imap_bridge::store::{AccountKey, MailboxRow, Store};
use huliho_imap_bridge::sync::{Cache, FolderSync, Step};
use huliho_imap_bridge::testing::imap::HOST;
use huliho_imap_bridge::testing::record::fixed::RECORDED_USER;
use huliho_imap_bridge::testing::record::{self, Upstream};
use huliho_imap_bridge::testing::seal::TestSealer;
use huliho_imap_bridge::testing::{
    PASSWORD, Recorder, Replayer, TestConnector, Transcript, replay,
};
use serde_json::{Value, json};
use tokio_rustls::rustls::ClientConfig;

/// The one account of the suite.
pub const ACCOUNT: &str = "gmail";

/// The sessions one folder may cost before the suite gives up.
const MAX_SESSIONS: usize = 20;

/// How the bridge reaches the server: the trust, where it is, who signs
/// in and how long one step may take.
#[derive(Clone)]
pub struct Connection {
    pub tls: Arc<ClientConfig>,
    pub target: Target,
    pub user: String,
    pub password: String,
    pub step: Duration,
}

/// What stands between the bridge and the server.
enum Server {
    Replaying(Replayer),
    Recording { recorder: Recorder, path: PathBuf },
    Direct,
}

/// The cache, the link and the way to the server.
pub struct Suite {
    pub cache: Cache,
    pub link: Link<TestConnector>,
    connection: Connection,
    server: Server,
}

impl Suite {
    /// Against the transcript through the replayer, signed in as the
    /// recorded user.
    pub async fn replaying(transcript: Transcript, step: Duration) -> Self {
        let replayer = Replayer::start(replay::Script::new(transcript)).await;
        let connection = Connection {
            tls: replayer.trusting(),
            target: replayer.target(HOST, TlsMode::Implicit),
            user: RECORDED_USER.to_owned(),
            password: PASSWORD.to_owned(),
            step,
        };
        Self::new(connection, Server::Replaying(replayer))
    }

    /// Against the server the connection names; through the recorder
    /// when a path to write the transcript to is given.
    pub async fn against(connection: Connection, record: Option<PathBuf>) -> Self {
        let Some(path) = record else {
            return Self::new(connection, Server::Direct);
        };
        let upstream = Upstream {
            target: connection.target.clone(),
            tls: Arc::clone(&connection.tls),
        };
        let recorder = Recorder::start(record::Script::new(upstream)).await;
        let through = Connection {
            tls: recorder.trusting(),
            target: recorder.target(HOST, TlsMode::Implicit),
            ..connection
        };
        Self::new(through, Server::Recording { recorder, path })
    }

    fn new(connection: Connection, server: Server) -> Self {
        let connector = TestConnector::Server {
            tls: Arc::clone(&connection.tls),
            target: connection.target.clone(),
            step: connection.step,
            user: connection.user.clone(),
            password: connection.password.clone(),
        };
        Self {
            cache: Cache {
                store: Arc::new(Store::in_memory().unwrap()),
                sealer: Arc::new(TestSealer::default()),
                key: AccountKey::new(ACCOUNT),
                gmail: true,
            },
            link: Link::with_interval(connector, Duration::ZERO),
            connection,
            server,
        }
    }

    /// Whether the answers come from a transcript, where a body text
    /// reads as filler.
    pub fn is_replay(&self) -> bool {
        matches!(self.server, Server::Replaying(_))
    }

    /// A session past LOGIN, the bridge's own.
    pub async fn session(&self) -> ImapSession {
        let connection = &self.connection;
        let mut session = ImapSession::connect(
            Arc::clone(&connection.tls),
            &connection.target,
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

    /// The arguments of the first response to one call under every
    /// capability.
    pub async fn call(&self, call: &Value) -> Value {
        let body = json!({
            "using": [CORE_CAPABILITY, MAIL_CAPABILITY, HULIHO_CAPABILITY],
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
        let mut response: Value = serde_json::from_slice(&bytes).unwrap();
        response["methodResponses"][0][1].take()
    }

    /// The end of a run: a replay must have gone as recorded, a
    /// recording is redacted, scanned and written.
    pub fn finish(self) {
        match self.server {
            Server::Replaying(replayer) => {
                let script = replayer.script();
                assert_eq!(script.mismatches(), Vec::<String>::new());
                assert_eq!(script.unplayed(), 0, "recorded connections nobody opened");
            }
            Server::Recording { recorder, path } => {
                let transcript = recorder
                    .script()
                    .finish()
                    .unwrap_or_else(|findings| panic!("the recording is not clean: {findings:#?}"));
                std::fs::write(&path, transcript.to_json()).unwrap();
            }
            Server::Direct => {}
        }
    }
}
