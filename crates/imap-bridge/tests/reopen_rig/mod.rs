// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The bridge as a process runs it over one database file: started,
//! stopped and started again over the same file, beside a connector that
//! refuses connects until a folder gives up and changes the server once
//! before the folder opens again.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use huliho_imap_bridge::jmap::{CORE_CAPABILITY, HULIHO_CAPABILITY, MAIL_CAPABILITY};
use huliho_imap_bridge::runtime::{Bridge, ConnectError, Connector, Registration, Timing};
use huliho_imap_bridge::session::{ImapSession, TlsMode};
use huliho_imap_bridge::store::{AccountKey, MIGRATIONS, MailboxRow, Progress, Standing, Store};
use huliho_imap_bridge::testing::imap::{FakeImap, HOST, Script};
use huliho_imap_bridge::testing::seal::TestSealer;
use huliho_imap_bridge::testing::{Behavior, Mailboxes, TestConnector};
use rusqlite::Connection;
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::time::sleep;

pub const ACCOUNT: &str = "a1";

/// Room for a loopback exchange.
const STEP: Duration = Duration::from_secs(2);

/// How often and how long a test looks for the sync to get somewhere.
const LOOK: Duration = Duration::from_millis(25);
const LOOKS: usize = 800;

/// Clocks a test does not wait for. A fetch that never answers holds its
/// turn for the deadline, which leaves a test the time to stop the process.
#[must_use]
pub fn quick() -> Timing {
    Timing {
        interval: Duration::ZERO,
        deadline: Duration::from_secs(5),
        search: Duration::from_secs(5),
        idle_close: Duration::from_secs(60),
        list_age: Duration::from_secs(60),
        retry_interval: Duration::from_millis(200),
        reconnect_pause: Duration::from_millis(5),
    }
}

/// The second UID FETCH of every connection never answers, so a first sync
/// holds after its first batch.
#[must_use]
pub fn held() -> Behavior {
    Behavior {
        stalls_at: Some(1),
        ..Behavior::default()
    }
}

/// A server over the folders of the model, misbehaving as `behavior` says.
/// Every server of a test shares the mail of the model.
pub async fn serve(model: &Mailboxes, behavior: Behavior) -> FakeImap {
    let mut mailboxes = model.clone();
    mailboxes.behavior = behavior;
    FakeImap::start(Script {
        mailboxes,
        ..Script::tls()
    })
    .await
}

/// A connector that signs in on that server.
#[must_use]
pub fn reach(fake: &FakeImap) -> TestConnector {
    TestConnector::scripted(fake.trusting(), fake.target(HOST, TlsMode::Implicit), STEP)
}

/// The database file the processes of one test open in turn.
pub struct Volume {
    _directory: TempDir,
    path: PathBuf,
}

impl Volume {
    /// A fresh file with every migration applied.
    #[must_use]
    pub fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("huliho.db");
        let connection = Connection::open(&path).unwrap();
        for sql in MIGRATIONS {
            connection.execute_batch(sql).unwrap();
        }
        Self {
            _directory: directory,
            path,
        }
    }
}

/// A connector that reaches `first` once, refuses the `refusals` connects
/// after it, runs `between` and reaches `then` from there on.
pub struct Gate {
    first: TestConnector,
    then: TestConnector,
    refusals: usize,
    between: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    connects: AtomicUsize,
}

impl Gate {
    pub fn new(
        (first, then): (TestConnector, TestConnector),
        refusals: usize,
        between: impl FnOnce() + Send + 'static,
    ) -> Self {
        Self {
            first,
            then,
            refusals,
            between: Mutex::new(Some(Box::new(between))),
            connects: AtomicUsize::new(0),
        }
    }
}

impl Connector for Gate {
    type Session = ImapSession;

    async fn connect(&self, key: &AccountKey) -> Result<ImapSession, ConnectError> {
        let index = self.connects.fetch_add(1, Ordering::SeqCst);
        if index == 0 {
            return self.first.connect(key).await;
        }
        if index <= self.refusals {
            return TestConnector::Refusing.connect(key).await;
        }
        let between = self
            .between
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(between) = between {
            between();
        }
        self.then.connect(key).await
    }
}

/// One process of the bridge over the volume, its account started.
pub struct Process<C: Connector + 'static> {
    bridge: Bridge<C>,
    registration: Registration,
}

impl<C: Connector + 'static> Process<C> {
    /// Opens the file as a process does at its start and starts the account.
    pub fn start(volume: &Volume, connector: C, gmail: bool, timing: Timing) -> Self {
        let store = Arc::new(Store::new(Connection::open(&volume.path).unwrap()));
        let sealer = Arc::new(TestSealer::default());
        let bridge = Bridge::with_timing(store, connector, sealer, timing);
        let registration = Registration {
            key: AccountKey::new(ACCOUNT),
            gmail,
            session_state: "host-state".to_owned(),
        };
        bridge.start(&registration);
        Self {
            bridge,
            registration,
        }
    }

    /// Ends the process: its task stops and its store writes no more.
    pub async fn stop(self) {
        self.bridge.forget(&self.registration.key).await;
    }

    fn store(&self) -> &Store {
        self.bridge.store()
    }

    /// The row of the folder with that wire name.
    pub fn folder(&self, imap_name: &str) -> Option<MailboxRow> {
        let snapshot = self.store().mailbox_snapshot(&self.registration.key);
        let mut rows = snapshot.unwrap().rows.into_iter();
        rows.find(|row| row.facts.imap_name == imap_name)
    }

    /// How far the folder stands; nothing before the first mailbox pass.
    pub fn progress(&self, imap_name: &str) -> Progress {
        self.folder(imap_name)
            .map_or_else(Progress::default, |row| {
                let progress = self.store().sync_progress(&self.registration.key, &row.id);
                progress.unwrap()
            })
    }

    /// The UIDs the rows of the folder hold, lowest first.
    pub fn rows(&self, imap_name: &str) -> Vec<u32> {
        let row = self.folder(imap_name).unwrap();
        let standing = Standing {
            folder: &row.id,
            uid_validity: row.facts.uid_validity.unwrap(),
        };
        let stored = self.store().stored_uids(&self.registration.key, &standing);
        stored.unwrap()
    }

    /// The state the account stands at.
    pub fn state(&self) -> u64 {
        self.store().state(&self.registration.key).unwrap()
    }

    /// Whether every store folder of the account is done.
    pub fn done(&self) -> bool {
        let snapshot = self.store().mailbox_snapshot(&self.registration.key);
        let snapshot = snapshot.unwrap();
        let mut stores = snapshot
            .rows
            .iter()
            .filter(|row| row.facts.store)
            .peekable();
        stores.peek().is_some() && stores.all(|row| snapshot.done.contains(&row.id))
    }

    /// Waits until `reached` holds; whether it did.
    pub async fn wait(&self, reached: impl Fn(&Self) -> bool) -> bool {
        for _ in 0..LOOKS {
            if reached(self) {
                return true;
            }
            sleep(LOOK).await;
        }
        false
    }

    /// The arguments of the first response to one call; a `/changes` call
    /// refreshes the account first.
    pub async fn call(&self, call: &Value) -> Value {
        let body = json!({
            "using": [CORE_CAPABILITY, MAIL_CAPABILITY, HULIHO_CAPABILITY],
            "methodCalls": [call],
        });
        let body = serde_json::to_vec(&body).unwrap();
        let bytes = self.bridge.handle(&self.registration, &body).await;
        let mut response: Value = serde_json::from_slice(&bytes.unwrap()).unwrap();
        response["methodResponses"][0][1].take()
    }

    /// `Email/changes` since a state, which runs the refresh.
    pub async fn changes_since(&self, state: u64) -> Value {
        let arguments = json!({ "accountId": ACCOUNT, "sinceState": state.to_string() });
        self.call(&json!(["Email/changes", arguments, "c1"])).await
    }

    /// `Email/get` of one email with the named properties.
    pub async fn email(&self, id: &Value, properties: &[&str]) -> Value {
        let arguments = json!({ "accountId": ACCOUNT, "ids": [id], "properties": properties });
        let mut got = self.call(&json!(["Email/get", arguments, "c1"])).await;
        got["list"][0].take()
    }
}
