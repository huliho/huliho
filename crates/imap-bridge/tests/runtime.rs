// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The bridge as a host runs it: an account's folders sync on their
//! own in tree order on one conversation, a Gmail account holds two, a
//! folder that fails every time is given up after the bound, a batch
//! past the deadline costs its session alone, an idle conversation is
//! logged out and a forgotten account takes no write.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use huliho_imap_bridge::jmap::{CORE_CAPABILITY, HULIHO_CAPABILITY, MAIL_CAPABILITY};
use huliho_imap_bridge::runtime::{Bridge, FOLDER_FAILURE_BOUND, Registration, Timing};
use huliho_imap_bridge::session::TlsMode;
use huliho_imap_bridge::store::{AccountKey, MailboxFacts, Store, StoreError};
use huliho_imap_bridge::sync::SYNC_BATCH;
use huliho_imap_bridge::testing::imap::{FakeImap, HOST, Script};
use huliho_imap_bridge::testing::seal::TestSealer;
use huliho_imap_bridge::testing::{
    Behavior, Counting, Extension, Folder, Mailboxes, Message, TestConnector,
};
use serde_json::{Value, json};
use tokio::time::sleep;

const ACCOUNT: &str = "a1";
const SESSION_STATE: &str = "host-state-7";

/// Room for a loopback exchange.
const STEP: Duration = Duration::from_secs(2);

/// How often and how long a test looks for the sync to finish.
const LOOK: Duration = Duration::from_millis(25);
const LOOKS: usize = 800;

/// Clocks a test does not wait for.
fn quick() -> Timing {
    Timing {
        interval: Duration::ZERO,
        deadline: Duration::from_secs(5),
        idle_close: Duration::from_secs(60),
        retry_interval: Duration::from_millis(200),
        reconnect_pause: Duration::from_millis(5),
    }
}

fn key() -> AccountKey {
    AccountKey::new(ACCOUNT)
}

/// The messages `1..=count`.
fn mail(count: u32) -> Vec<Message> {
    (1..=count).map(Message::new).collect()
}

/// A Dovecot-shaped account with mail in every folder and one user
/// folder, listed after the roles.
fn shaped() -> Mailboxes {
    Mailboxes::new(
        vec![
            Folder::new("Work").with_mail((51..=52).map(Message::new).collect()),
            Folder::special("Trash", "\\Trash").with_mail((41..=41).map(Message::new).collect()),
            Folder::special("Junk", "\\Junk").with_mail((31..=31).map(Message::new).collect()),
            Folder::special("Archive", "\\Archive")
                .with_mail((21..=22).map(Message::new).collect()),
            Folder::special("Sent", "\\Sent").with_mail((11..=12).map(Message::new).collect()),
            Folder::special("Drafts", "\\Drafts").with_mail(vec![Message::new(1)]),
            Folder::new("INBOX").with_mail(mail(3)),
        ],
        Extension::all(),
    )
}

/// The scripted server, the bridge over it and what a test reads back.
struct Rig {
    bridge: Bridge<Counting>,
    fake: FakeImap,
    connects: Arc<AtomicUsize>,
    registration: Registration,
}

impl Rig {
    /// The bridge over the model, the account started.
    async fn start(mailboxes: Mailboxes, gmail: bool, timing: Timing) -> Self {
        let fake = FakeImap::start(Script {
            mailboxes,
            ..Script::tls()
        })
        .await;
        let scripted =
            TestConnector::scripted(fake.trusting(), fake.target(HOST, TlsMode::Implicit), STEP);
        let (connector, connects) = Counting::new(scripted);
        let bridge = Bridge::with_timing(
            Arc::new(Store::in_memory().unwrap()),
            connector,
            Arc::new(TestSealer::default()),
            timing,
        );
        let registration = Registration {
            key: AccountKey::new(ACCOUNT),
            gmail,
            session_state: SESSION_STATE.to_owned(),
        };
        bridge.start(&registration);
        Self {
            bridge,
            fake,
            connects,
            registration,
        }
    }

    /// Whether every store folder of the account is done.
    fn done(&self) -> bool {
        let snapshot = self.bridge.store().mailbox_snapshot(&key()).unwrap();
        let stores: Vec<_> = snapshot.rows.iter().filter(|row| row.facts.store).collect();
        !stores.is_empty() && stores.iter().all(|row| snapshot.done.contains(&row.id))
    }

    /// Waits for the sync to finish; whether it did.
    async fn wait_done(&self) -> bool {
        for _ in 0..LOOKS {
            if self.done() {
                return true;
            }
            sleep(LOOK).await;
        }
        false
    }

    /// Waits until the server received `count` lines holding `needle`;
    /// whether it did.
    async fn wait_received(&self, needle: &str, count: usize) -> bool {
        for _ in 0..LOOKS {
            if self.received(needle) >= count {
                return true;
            }
            sleep(LOOK).await;
        }
        false
    }

    /// How many lines the server received hold `needle`.
    fn received(&self, needle: &str) -> usize {
        self.fake
            .lines()
            .iter()
            .filter(|line| line.contains(needle))
            .count()
    }

    /// The folders EXAMINE selected, in order of first appearance.
    fn examined(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for line in self.fake.lines() {
            if let Some((_, name)) = line.split_once(" EXAMINE ") {
                let name = name.trim_matches('"').to_owned();
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
        names
    }

    /// The arguments of the first response to one call, and the
    /// response's `sessionState`.
    async fn call(&self, call: &Value) -> (Value, Value) {
        let body = json!({
            "using": [CORE_CAPABILITY, MAIL_CAPABILITY, HULIHO_CAPABILITY],
            "methodCalls": [call],
        });
        let bytes = self
            .bridge
            .handle(&self.registration, &serde_json::to_vec(&body).unwrap())
            .await
            .unwrap();
        let mut response: Value = serde_json::from_slice(&bytes).unwrap();
        (
            response["methodResponses"][0][1].take(),
            response["sessionState"].take(),
        )
    }

    /// The Mailbox object of the folder with that wire name.
    async fn mailbox(&self, imap_name: &str) -> Value {
        let id = self
            .bridge
            .store()
            .mailbox_snapshot(&key())
            .unwrap()
            .rows
            .into_iter()
            .find(|row| row.facts.imap_name == imap_name)
            .unwrap()
            .id;
        let call = json!(["Mailbox/get", { "accountId": ACCOUNT, "ids": [id] }, "c1"]);
        self.call(&call).await.0["list"][0].take()
    }
}

#[tokio::test]
async fn an_account_syncs_every_store_folder_on_its_own_in_tree_order() {
    let rig = Rig::start(shaped(), false, quick()).await;
    assert!(rig.wait_done().await, "{:?}", rig.fake.lines());
    assert_eq!(
        rig.examined(),
        [
            "INBOX", "Drafts", "Sent", "Archive", "Junk", "Trash", "Work"
        ]
    );
    let inbox = rig.mailbox("INBOX").await;
    assert_eq!(inbox["syncedEmails"], 3);
    assert_eq!(inbox["totalEmails"], 3);
    let (answer, state) = rig
        .call(&json!([
            "Email/changes",
            { "accountId": ACCOUNT, "sinceState": "0" },
            "c1"
        ]))
        .await;
    assert_eq!(state, SESSION_STATE);
    assert_eq!(answer["created"].as_array().unwrap().len(), 12);
    // The requests took the conversation the sync ran on.
    assert_eq!(rig.connects.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_gmail_account_holds_one_conversation_for_the_sync_and_one_for_the_requests() {
    let rig = Rig::start(Mailboxes::gmail(mail(3)), true, quick()).await;
    assert!(rig.wait_done().await, "{:?}", rig.fake.lines());
    let (answer, _) = rig
        .call(&json!([
            "Mailbox/changes",
            { "accountId": ACCOUNT, "sinceState": "0" },
            "c1"
        ]))
        .await;
    assert!(!answer["created"].as_array().unwrap().is_empty());
    assert_eq!(rig.connects.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn a_folder_that_fails_every_fetch_is_given_up_after_the_bound_and_the_cache_answers() {
    let mut model = Mailboxes::new(
        vec![Folder::new("INBOX").with_mail(mail(2))],
        Extension::all(),
    );
    model.behavior = Behavior {
        drops_after: Some(0),
        ..Behavior::default()
    };
    let rig = Rig::start(model, false, quick()).await;
    assert!(rig.wait_received(" LOGIN ", FOLDER_FAILURE_BOUND).await);
    // The mailbox pass rides the first connection; every failed fetch costs one more.
    assert!(!rig.done());
    let inbox = rig.mailbox("INBOX").await;
    assert_eq!(inbox["totalEmails"], 2);
    assert_eq!(inbox["syncedEmails"], 0);
}

#[tokio::test]
async fn a_batch_past_the_deadline_costs_its_session_and_the_next_one_carries_on() {
    let count = u32::try_from(2 * SYNC_BATCH + 100).unwrap();
    let mut model = Mailboxes::new(
        vec![Folder::new("INBOX").with_mail(mail(count))],
        Extension::all(),
    );
    // The second fetch of every connection never answers.
    model.behavior = Behavior {
        stalls_at: Some(1),
        ..Behavior::default()
    };
    let timing = Timing {
        deadline: Duration::from_millis(300),
        ..quick()
    };
    let rig = Rig::start(model, false, timing).await;
    assert!(rig.wait_done().await, "{:?}", rig.fake.lines());
    assert_eq!(
        rig.mailbox("INBOX").await["syncedEmails"],
        count,
        "{:?}",
        rig.fake.lines()
    );
    // One connection per batch: each one answers its first fetch and stalls on its second.
    assert_eq!(rig.received(" LOGIN "), 3);
}

#[tokio::test]
async fn an_idle_conversation_is_logged_out_after_its_bound_and_a_request_connects_again() {
    let timing = Timing {
        idle_close: Duration::from_millis(200),
        ..quick()
    };
    let rig = Rig::start(shaped(), false, timing).await;
    assert!(rig.wait_done().await);
    assert!(
        rig.wait_received(" LOGOUT", 1).await,
        "{:?}",
        rig.fake.lines()
    );
    let before = rig.connects.load(Ordering::SeqCst);
    let (answer, _) = rig
        .call(&json!([
            "Mailbox/changes",
            { "accountId": ACCOUNT, "sinceState": "0" },
            "c1"
        ]))
        .await;
    assert!(answer["newState"].is_string(), "{answer}");
    assert_eq!(rig.connects.load(Ordering::SeqCst), before + 1);
}

#[tokio::test]
async fn a_forgotten_account_stops_its_task_and_takes_no_write() {
    let rig = Rig::start(shaped(), false, quick()).await;
    assert!(rig.wait_done().await);
    let logins = rig.received(" LOGIN ");
    rig.bridge.forget(&key()).await;
    let facts = MailboxFacts {
        name: "INBOX".to_owned(),
        imap_name: "INBOX".to_owned(),
        parent_imap_name: None,
        role: Some("inbox".to_owned()),
        sort_order: 0,
        subscribed: true,
        selectable: true,
        store: true,
        gmail_label: None,
        uid_validity: Some(1),
        uid_next: Some(4),
        highest_modseq: None,
        total_emails: 3,
        unread_emails: 0,
    };
    let refused = rig.bridge.store().apply_mailboxes(&key(), &[facts]);
    assert!(matches!(refused, Err(StoreError::Forgotten)), "{refused:?}");
    sleep(Duration::from_millis(400)).await;
    assert_eq!(rig.received(" LOGIN "), logins, "the task connected again");
}
