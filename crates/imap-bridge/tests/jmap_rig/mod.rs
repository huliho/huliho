// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A store the mailbox pass filled from the Dovecot-shaped script, and
//! one request against it over a link to the same server.

use std::sync::Arc;
use std::time::Duration;

use huliho_imap_bridge::jmap::{CORE_CAPABILITY, MAIL_CAPABILITY, RequestError, handle};
use huliho_imap_bridge::mailboxes::sync;
use huliho_imap_bridge::runtime::Link;
use huliho_imap_bridge::session::{ImapSession, Session, TlsMode};
use huliho_imap_bridge::store::{AccountKey, Store};
use huliho_imap_bridge::sync::Cache;
use huliho_imap_bridge::testing::imap::{FakeImap, HOST, Script};
use huliho_imap_bridge::testing::seal::TestSealer;
use huliho_imap_bridge::testing::{Folder, Mailboxes, PASSWORD, TestConnector, USER};
use serde_json::{Map, Value, json};

/// Room for a loopback exchange.
const STEP: Duration = Duration::from_secs(1);

/// The one account of the rig.
pub const ACCOUNT: &str = "a1";

/// The session state the host hands the rig.
pub const SESSION_STATE: &str = "host-state-1";

/// The key of that account.
pub fn key() -> AccountKey {
    AccountKey::new(ACCOUNT)
}

/// The scripted server, its mailbox model, the store the pass fills
/// and the link a request reaches the server through.
pub struct Rig {
    /// The store the pass fills.
    pub store: Arc<Store>,
    pub cache: Cache,
    link: Link<TestConnector>,
    fake: FakeImap,
    mailboxes: Mailboxes,
}

impl Rig {
    /// The Dovecot-shaped script listed once: state 1.
    pub async fn start() -> Self {
        let mailboxes = Mailboxes::dovecot();
        let fake = FakeImap::start(Script {
            mailboxes: mailboxes.clone(),
            ..Script::tls()
        })
        .await;
        let store = Arc::new(Store::in_memory().unwrap());
        let connector =
            TestConnector::scripted(fake.trusting(), fake.target(HOST, TlsMode::Implicit), STEP);
        let rig = Self {
            store: Arc::clone(&store),
            cache: Cache {
                store,
                sealer: Arc::new(TestSealer::default()),
                key: key(),
                gmail: false,
            },
            link: Link::with_interval(connector, Duration::ZERO),
            fake,
            mailboxes,
        };
        assert_eq!(rig.pass().await, 1);
        rig
    }

    /// One more pass; the state it ends at.
    pub async fn pass(&self) -> u64 {
        let target = self.fake.target(HOST, TlsMode::Implicit);
        let mut session = ImapSession::connect(self.fake.trusting(), &target, STEP)
            .await
            .unwrap();
        session.login(USER, PASSWORD).await.unwrap();
        let state = sync(&mut session, &self.cache).await.unwrap();
        session.logout().await.unwrap();
        state
    }

    /// Work added, Archive gone, INBOX counting one more: the second
    /// state once passed.
    pub fn edit(&self) {
        self.mailboxes.push(Folder::new("Work"));
        self.mailboxes.remove("Archive");
        let mut folders = self.mailboxes.folders();
        folders[0] = Folder::new("INBOX").with_counts(18, 4);
        self.mailboxes.set(folders);
    }

    /// One request with the given capabilities and calls, the Response
    /// object parsed.
    pub async fn request(&self, using: &[&str], calls: Value) -> Result<Value, RequestError> {
        let mut body = Map::new();
        body.insert("using".to_owned(), json!(using));
        body.insert("methodCalls".to_owned(), calls);
        let bytes = self.raw(&serde_json::to_vec(&body).unwrap()).await?;
        Ok(serde_json::from_slice(&bytes).unwrap())
    }

    /// One request body as it arrived, the answer as bytes.
    pub async fn raw(&self, body: &[u8]) -> Result<Vec<u8>, RequestError> {
        handle(&self.cache, &self.link, body, SESSION_STATE).await
    }

    /// A request under core and mail, which every method of this bridge
    /// runs under.
    pub async fn mail(&self, calls: Value) -> Value {
        self.request(&[CORE_CAPABILITY, MAIL_CAPABILITY], calls)
            .await
            .unwrap()
    }

    /// The id of the mailbox with that wire name.
    pub fn id_of(&self, imap_name: &str) -> String {
        self.store
            .mailbox_snapshot(&key())
            .unwrap()
            .rows
            .into_iter()
            .find(|row| row.facts.imap_name == imap_name)
            .unwrap()
            .id
            .to_string()
    }
}

/// A `Mailbox/get` for every mailbox under the call id.
pub fn mailbox_get(call_id: &str) -> Value {
    json!(["Mailbox/get", { "accountId": ACCOUNT, "ids": null }, call_id])
}

/// The first response of a Response object.
pub fn first(response: &Value) -> &Value {
    &response["methodResponses"][0]
}

/// The error type of a method response, if it is one.
pub fn error_type(invocation: &Value) -> Option<&str> {
    (invocation[0] == "error").then(|| invocation[1]["type"].as_str().unwrap())
}
