// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A scripted server with mail in its folders, a cache the sync writes
//! to and the driver a host runs: a fresh session after every failure.

use std::sync::Arc;
use std::time::Duration;

use huliho_imap_bridge::jmap::{CORE_CAPABILITY, HULIHO_CAPABILITY, MAIL_CAPABILITY, handle};
use huliho_imap_bridge::mailboxes::{self, SyncError};
use huliho_imap_bridge::session::{ImapSession, Session, TlsMode};
use huliho_imap_bridge::store::{AccountKey, ChangeKind, MailboxRow, ObjectType, Store};
use huliho_imap_bridge::sync::{Cache, FolderSync, Step};
use huliho_imap_bridge::testing::imap::{FakeImap, HOST, Script};
use huliho_imap_bridge::testing::seal::TestSealer;
use huliho_imap_bridge::testing::{Extension, Folder, Mailboxes, Message, PASSWORD, USER};
use serde_json::{Value, json};

/// Room for a loopback exchange.
const STEP: Duration = Duration::from_secs(2);

/// The sessions one folder may cost before the driver gives up.
const MAX_SESSIONS: usize = 200;

/// The one account of the rig.
pub const ACCOUNT: &str = "a1";

/// The server and the cache. A test keeps the model it started the
/// server over and edits the folders through it.
pub struct Rig {
    pub cache: Cache,
    pub fake: FakeImap,
}

/// An INBOX holding this mail behind every extension.
pub fn inbox(mail: Vec<Message>) -> Mailboxes {
    Mailboxes::new(vec![Folder::new("INBOX").with_mail(mail)], Extension::all())
}

impl Rig {
    /// The server over the model, listed once.
    pub async fn start(mailboxes: Mailboxes) -> Self {
        let rig = Self::over(mailboxes).await;
        rig.pass().await.unwrap();
        rig
    }

    /// The server over the model with an empty cache.
    pub async fn over(mailboxes: Mailboxes) -> Self {
        let fake = FakeImap::start(Script {
            mailboxes,
            ..Script::tls()
        })
        .await;
        Self {
            cache: Cache {
                store: Arc::new(Store::in_memory().unwrap()),
                sealer: Arc::new(TestSealer::default()),
                key: AccountKey::new(ACCOUNT),
            },
            fake,
        }
    }

    /// A session past LOGIN.
    pub async fn session(&self) -> ImapSession {
        let target = self.fake.target(HOST, TlsMode::Implicit);
        let mut session = ImapSession::connect(self.fake.trusting(), &target, STEP)
            .await
            .unwrap();
        session.login(USER, PASSWORD).await.unwrap();
        session
    }

    /// One mailbox pass on a session of its own.
    pub async fn pass(&self) -> Result<u64, SyncError> {
        let mut session = self.session().await;
        let store = Arc::clone(&self.cache.store);
        mailboxes::sync(&mut session, store, self.cache.key.clone()).await
    }

    /// The row of the folder with that wire name.
    pub fn folder(&self, imap_name: &str) -> MailboxRow {
        self.cache
            .store
            .mailbox_snapshot(&self.cache.key)
            .unwrap()
            .rows
            .into_iter()
            .find(|row| row.facts.imap_name == imap_name)
            .unwrap()
    }

    /// Opens the sync of a folder on a fresh session.
    pub async fn open(&self, imap_name: &str) -> (ImapSession, Option<FolderSync>) {
        let mut session = self.session().await;
        let sync = FolderSync::open(&mut session, &self.cache, &self.folder(imap_name))
            .await
            .unwrap();
        (session, sync)
    }

    /// Runs a folder to its end the way a host does: after a failure a
    /// fresh session resumes the same sync. Answers the last step and
    /// the sessions it took.
    pub async fn sync(&self, imap_name: &str) -> (Step, usize) {
        let (mut session, sync) = self.open(imap_name).await;
        let mut sync = sync.unwrap();
        let mut sessions = 1;
        loop {
            match sync.finish(&mut session, &self.cache).await {
                Ok(step) => return (step, sessions),
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
    pub fn call(&self, call: &Value) -> Value {
        let body = json!({
            "using": [CORE_CAPABILITY, MAIL_CAPABILITY, HULIHO_CAPABILITY],
            "methodCalls": [call],
        });
        let bytes = handle(
            &self.cache.store,
            self.cache.sealer.as_ref(),
            &self.cache.key,
            &serde_json::to_vec(&body).unwrap(),
        )
        .unwrap();
        let mut response: Value = serde_json::from_slice(&bytes).unwrap();
        response["methodResponses"][0][1].take()
    }

    /// Every change of a type after a state, as id and kind.
    pub fn changes(&self, object: ObjectType, since: u64) -> Vec<(String, ChangeKind)> {
        self.cache
            .store
            .changes_since(&self.cache.key, object, since)
            .unwrap()
            .changes
            .unwrap()
            .into_iter()
            .map(|change| (change.id, change.kind))
            .collect()
    }

    /// The ids of the emails created since a state, oldest change first.
    pub fn created(&self, since: u64) -> Vec<String> {
        self.changes(ObjectType::Email, since)
            .into_iter()
            .filter(|(_, kind)| *kind == ChangeKind::Created)
            .map(|(id, _)| id)
            .collect()
    }
}
