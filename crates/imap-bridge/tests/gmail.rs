// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The Gmail label model against the scripted server's Gmail mode:
//! three stores, every other folder a label over All Mail, one email
//! per X-GM-MSGID across the stores and threads from X-GM-THRID.

mod sync_rig;

use std::collections::{BTreeSet, HashMap};

use huliho_imap_bridge::jmap::{MAIL_CAPABILITY, Urls, session_object};
use huliho_imap_bridge::session::{FetchItems, FlagFetch, Session, UidRange};
use huliho_imap_bridge::store::{ChangeKind, MailboxRow, ObjectType};
use huliho_imap_bridge::sync::Step;
use huliho_imap_bridge::testing::mailboxes::{ALL_MAIL, SPAM, TRASH};
use huliho_imap_bridge::testing::{Extension, Mailboxes, Message};
use serde_json::{Value, json};
use sync_rig::{ACCOUNT, Rig, inbox};

const INBOX: &str = "INBOX";
const WORK: &str = "Work";

/// A thread two messages share.
const SHARED_THREAD: u64 = 77;

/// `<Type>/changes` since a state.
async fn changes(rig: &Rig, object: &str, since: u64) -> Value {
    let arguments = json!({ "accountId": ACCOUNT, "sinceState": since.to_string() });
    rig.call(&json!([format!("{object}/changes"), arguments, "c1"]))
        .await
}

/// The lengths of the three lists of a `/changes` answer.
fn lengths(answer: &Value) -> (usize, usize, usize) {
    let count = |list: &str| answer[list].as_array().unwrap().len();
    (count("created"), count("updated"), count("destroyed"))
}

/// The ids one list of a `/changes` answer names.
fn listed(answer: &Value, list: &str) -> Vec<String> {
    answer[list]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| id.as_str().unwrap().to_owned())
        .collect()
}

fn state(rig: &Rig) -> u64 {
    rig.cache.store.state(&rig.cache.key).unwrap()
}

fn rows(rig: &Rig) -> Vec<MailboxRow> {
    rig.cache
        .store
        .mailbox_snapshot(&rig.cache.key)
        .unwrap()
        .rows
}

/// The Mailbox object of the folder with that wire name.
async fn mailbox(rig: &Rig, imap_name: &str) -> Value {
    let id = rig.folder(imap_name).id;
    let call = json!(["Mailbox/get", { "accountId": ACCOUNT, "ids": [id] }, "c1"]);
    rig.call(&call).await["list"][0].take()
}

/// `Email/get` for the ids with the named properties.
async fn emails(rig: &Rig, ids: &[String], properties: &[&str]) -> Vec<Value> {
    let arguments = json!({ "accountId": ACCOUNT, "ids": ids, "properties": properties });
    rig.call(&json!(["Email/get", arguments, "c1"])).await["list"]
        .as_array()
        .unwrap()
        .clone()
}

/// The wire names of the mailboxes an email is a member of.
async fn mailboxes_of(rig: &Rig, id: &str) -> BTreeSet<String> {
    let names: HashMap<String, String> = rows(rig)
        .into_iter()
        .map(|row| (row.id.to_string(), row.facts.imap_name))
        .collect();
    let found = emails(rig, &[id.to_owned()], &["mailboxIds"]).await;
    found[0]["mailboxIds"]
        .as_object()
        .unwrap()
        .keys()
        .map(|id| names[id].clone())
        .collect()
}

fn names(wire: &[&str]) -> BTreeSet<String> {
    wire.iter().map(|name| (*name).to_owned()).collect()
}

/// The lines the server received that hold `needle`.
fn received(rig: &Rig, needle: &str) -> usize {
    rig.fake
        .lines()
        .iter()
        .filter(|line| line.contains(needle))
        .count()
}

/// The server over the model, listed once, for an account the host
/// calls a Gmail account.
async fn start_gmail(mailboxes: Mailboxes) -> Rig {
    let mut rig = Rig::over(mailboxes).await;
    rig.cache.gmail = true;
    rig.pass().await.unwrap();
    rig
}

/// Every store synced, in the order the sync takes them.
async fn synced(rig: &Rig) {
    for store in [ALL_MAIL, SPAM, TRASH] {
        assert_eq!(rig.sync(store).await, (Step::Done, 1), "{store}");
    }
}

#[tokio::test]
async fn a_gmail_account_lists_three_stores_and_every_other_folder_as_a_label() {
    let rig = start_gmail(Mailboxes::gmail(Vec::new()).with_label(WORK)).await;
    let rows = rows(&rig);
    let facts = |name: &str| {
        &rows
            .iter()
            .find(|row| row.facts.imap_name == name)
            .unwrap()
            .facts
    };
    for (store, role) in [(ALL_MAIL, "archive"), (SPAM, "junk"), (TRASH, "trash")] {
        let found = facts(store);
        assert!(found.store && found.gmail_label.is_none(), "{store}");
        assert_eq!(found.role.as_deref(), Some(role), "{store}");
    }
    let labels = [
        (INBOX, "\\Inbox", Some("inbox")),
        ("[Gmail]/Sent Mail", "\\Sent", Some("sent")),
        ("[Gmail]/Drafts", "\\Drafts", Some("drafts")),
        ("[Gmail]/Starred", "\\Flagged", Some("flagged")),
        ("[Gmail]/Important", "\\Important", Some("important")),
        (WORK, WORK, None),
    ];
    for (name, label, role) in labels {
        let found = facts(name);
        assert!(!found.store && found.selectable, "{name}");
        assert_eq!(found.gmail_label.as_deref(), Some(label), "{name}");
        assert_eq!(found.role.as_deref(), role, "{name}");
    }
    let placeholder = facts("[Gmail]");
    assert!(!placeholder.selectable && !placeholder.store && placeholder.gmail_label.is_none());
    let (_, sync) = rig.open(INBOX).await;
    assert!(sync.is_none(), "a label folder opens no sync of its own");
}

#[tokio::test]
async fn a_message_under_two_labels_is_one_email_in_the_thread_the_server_names() {
    let mail = vec![
        Message::new(1)
            .labeled(&["\\Inbox", WORK])
            .in_thread(SHARED_THREAD),
        Message::new(2).in_thread(SHARED_THREAD),
        Message::new(3).labeled(&[WORK, "\\Starred", "\\Draft"]),
    ];
    let rig = start_gmail(Mailboxes::gmail(mail).with_label(WORK)).await;
    assert_eq!(rig.sync(ALL_MAIL).await, (Step::Done, 1));
    let created = rig.created(0);
    assert_eq!(created.len(), 3);
    let found = emails(&rig, &created, &["id", "threadId", "size"]).await;
    let by_uid = |uid: u64| {
        found
            .iter()
            .find(|email| email["size"] == 1000 + uid)
            .unwrap()
    };
    let shared = format!("t{SHARED_THREAD}");
    assert_eq!(by_uid(1)["threadId"], shared);
    assert_eq!(by_uid(2)["threadId"], shared);
    assert_ne!(by_uid(3)["threadId"], shared);
    assert_eq!(
        mailboxes_of(&rig, by_uid(1)["id"].as_str().unwrap()).await,
        names(&[ALL_MAIL, INBOX, WORK])
    );
    assert_eq!(
        mailboxes_of(&rig, by_uid(3)["id"].as_str().unwrap()).await,
        names(&[ALL_MAIL, WORK, "[Gmail]/Starred", "[Gmail]/Drafts"]),
        "both spellings of the system labels map"
    );
    let thread = rig
        .call(&json!(["Thread/get", { "accountId": ACCOUNT, "ids": [shared] }, "c1"]))
        .await;
    assert_eq!(thread["list"][0]["emailIds"].as_array().unwrap().len(), 2);
    assert_eq!(received(&rig, "X-GM-LABELS X-GM-MSGID X-GM-THRID"), 1);
    assert_eq!(
        received(&rig, "EXAMINE"),
        1,
        "the label folders are never opened"
    );
    assert_eq!(rig.changes(ObjectType::Thread, 0).len(), 2);
}

#[tokio::test]
async fn label_mailbox_counts_follow_status_until_all_mail_is_done_and_the_memberships_after() {
    let mail = vec![
        Message::new(1).flagged(&[]),
        Message::new(2),
        Message::new(3).labeled(&[WORK]),
    ];
    let rig = start_gmail(Mailboxes::gmail(mail).with_label(WORK)).await;
    let inbox = mailbox(&rig, INBOX).await;
    assert_eq!(inbox["totalEmails"], 2);
    assert_eq!(inbox["unreadEmails"], 1);
    assert_eq!(inbox["syncedEmails"], 0);
    assert_eq!(mailbox(&rig, ALL_MAIL).await["totalEmails"], 3);
    assert_eq!(rig.sync(ALL_MAIL).await, (Step::Done, 1));
    let inbox = mailbox(&rig, INBOX).await;
    assert_eq!(inbox["totalEmails"], 2);
    assert_eq!(inbox["unreadEmails"], 1);
    assert_eq!(inbox["syncedEmails"], 2);
    assert_eq!(inbox["totalThreads"], 2);
    assert_eq!(mailbox(&rig, WORK).await["totalEmails"], 1);
    assert_eq!(mailbox(&rig, ALL_MAIL).await["totalEmails"], 3);
}

#[tokio::test]
async fn a_label_added_or_removed_between_two_passes_is_one_updated_with_the_new_mailbox_ids() {
    let model = Mailboxes::gmail(vec![Message::new(1)]).with_label(WORK);
    let rig = start_gmail(model.clone()).await;
    synced(&rig).await;
    let id = rig.created(0).remove(0);
    let before = state(&rig);
    model.relabel(ALL_MAIL, 1, &["\\Inbox", WORK]);
    let added = changes(&rig, "Email", before).await;
    assert_eq!(lengths(&added), (0, 1, 0));
    assert_eq!(listed(&added, "updated"), [id.as_str()]);
    assert_eq!(
        mailboxes_of(&rig, &id).await,
        names(&[ALL_MAIL, INBOX, WORK])
    );
    let work = rig.folder(WORK).id.to_string();
    assert!(listed(&changes(&rig, "Mailbox", before).await, "updated").contains(&work));
    let between = state(&rig);
    model.relabel(ALL_MAIL, 1, &[WORK]);
    let removed = changes(&rig, "Email", between).await;
    assert_eq!(lengths(&removed), (0, 1, 0));
    assert_eq!(mailboxes_of(&rig, &id).await, names(&[ALL_MAIL, WORK]));
    assert_eq!(mailbox(&rig, INBOX).await["totalEmails"], 0);
    assert!(received(&rig, "(UID FLAGS X-GM-LABELS) (CHANGEDSINCE") >= 2);
    assert_eq!(lengths(&changes(&rig, "Thread", before).await), (0, 0, 0));
}

#[tokio::test]
async fn a_move_to_spam_and_back_keeps_the_id_and_swaps_the_memberships() {
    let model = Mailboxes::gmail(vec![Message::new(1), Message::new(2)]);
    let rig = start_gmail(model.clone()).await;
    synced(&rig).await;
    let found = emails(&rig, &rig.created(0), &["id", "size"]).await;
    let id = found.iter().find(|email| email["size"] == 1001).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let before = state(&rig);
    model.move_to((ALL_MAIL, 1), SPAM);
    let moved = changes(&rig, "Email", before).await;
    assert_eq!(lengths(&moved), (0, 1, 0));
    assert_eq!(listed(&moved, "updated"), [id.as_str()]);
    assert_eq!(mailboxes_of(&rig, &id).await, names(&[SPAM]));
    assert_eq!(mailbox(&rig, INBOX).await["totalEmails"], 1);
    assert_eq!(mailbox(&rig, SPAM).await["totalEmails"], 1);
    assert_eq!(lengths(&changes(&rig, "Thread", before).await), (0, 0, 0));
    let between = state(&rig);
    model.move_to((SPAM, 1), ALL_MAIL);
    let back = changes(&rig, "Email", between).await;
    assert_eq!(lengths(&back), (0, 1, 0));
    assert_eq!(listed(&back, "updated"), [id.as_str()]);
    assert_eq!(
        mailboxes_of(&rig, &id).await,
        names(&[ALL_MAIL]),
        "the Inbox label left with the move"
    );
    assert_eq!(mailbox(&rig, SPAM).await["totalEmails"], 0);
}

#[tokio::test]
async fn a_message_that_leaves_all_mail_for_nowhere_is_destroyed_at_the_end_of_the_pass() {
    let model = Mailboxes::gmail(vec![Message::new(1), Message::new(2)]);
    let rig = start_gmail(model.clone()).await;
    synced(&rig).await;
    let before = state(&rig);
    model.expunge_uid(ALL_MAIL, 1);
    let gone = changes(&rig, "Email", before).await;
    assert_eq!(lengths(&gone), (0, 0, 1));
    assert_eq!(lengths(&changes(&rig, "Thread", before).await), (0, 0, 1));
    assert_eq!(mailbox(&rig, INBOX).await["totalEmails"], 1);
    let inbox = rig.folder(INBOX).id.to_string();
    assert!(listed(&changes(&rig, "Mailbox", before).await, "updated").contains(&inbox));
}

#[tokio::test]
async fn a_vanished_label_drops_its_memberships_and_the_emails_read_as_updated() {
    let model =
        Mailboxes::gmail(vec![Message::new(1).labeled(&["\\Inbox", WORK])]).with_label(WORK);
    let rig = start_gmail(model.clone()).await;
    assert_eq!(rig.sync(ALL_MAIL).await, (Step::Done, 1));
    let id = rig.created(0).remove(0);
    let work = rig.folder(WORK).id.to_string();
    let before = state(&rig);
    model.remove(WORK);
    assert_eq!(rig.pass().await.unwrap(), before + 1);
    assert_eq!(
        rig.changes(ObjectType::Mailbox, before),
        [(work, ChangeKind::Destroyed)]
    );
    assert_eq!(
        rig.changes(ObjectType::Email, before),
        [(id.clone(), ChangeKind::Updated)]
    );
    assert_eq!(mailboxes_of(&rig, &id).await, names(&[ALL_MAIL, INBOX]));
}

#[tokio::test]
async fn a_server_without_the_extension_runs_the_account_as_folders() {
    let mut model = Mailboxes::gmail(vec![Message::new(1)]);
    model.extensions.remove(&Extension::Gmail);
    let rig = start_gmail(model).await;
    for row in rows(&rig) {
        let facts = &row.facts;
        assert_eq!(facts.store, facts.selectable, "{}", facts.imap_name);
        assert_eq!(facts.gmail_label, None, "{}", facts.imap_name);
    }
    assert_eq!(rig.sync(ALL_MAIL).await, (Step::Done, 1));
    assert_eq!(received(&rig, "X-GM-"), 0);
    let id = rig.created(0).remove(0);
    assert_eq!(mailboxes_of(&rig, &id).await, names(&[ALL_MAIL]));
}

#[tokio::test]
async fn an_account_the_host_calls_a_folder_account_is_never_asked_the_gmail_items() {
    let mut model = inbox(vec![Message::new(1).labeled(&[WORK])]);
    model.extensions.insert(Extension::Gmail);
    let rig = Rig::start(model).await;
    let facts = &rig.folder(INBOX).facts;
    assert!(facts.store && facts.gmail_label.is_none());
    assert_eq!(rig.sync(INBOX).await, (Step::Done, 1));
    assert_eq!(received(&rig, "X-GM-"), 0);
    let id = rig.created(0).remove(0);
    assert_eq!(mailboxes_of(&rig, &id).await, names(&[INBOX]));
}

#[tokio::test]
async fn gmail_items_a_folder_account_did_not_ask_for_are_dropped_at_the_session() {
    let mut model = inbox(vec![
        Message::new(1).labeled(&[WORK]).in_thread(SHARED_THREAD),
        Message::new(2).in_thread(SHARED_THREAD),
    ]);
    model.extensions.insert(Extension::Gmail);
    model.behavior.gmail_items = true;
    let rig = Rig::start(model.clone()).await;
    assert_eq!(rig.sync(INBOX).await, (Step::Done, 1));
    assert_eq!(received(&rig, "X-GM-"), 0);
    let created = rig.created(0);
    assert_eq!(created.len(), 2);
    let found = emails(&rig, &created, &["id", "threadId"]).await;
    assert_ne!(
        found[0]["threadId"], found[1]["threadId"],
        "the threads are computed, not the server's"
    );
    for email in &found {
        let id = email["id"].as_str().unwrap();
        assert_eq!(mailboxes_of(&rig, id).await, names(&[INBOX]));
    }
    let before = state(&rig);
    model.relabel(INBOX, 1, &["\\Inbox", WORK]);
    assert_eq!(lengths(&changes(&rig, "Email", before).await), (0, 0, 0));
    let mut session = rig.session().await;
    session.examine(INBOX).await.unwrap();
    let range = UidRange { low: 1, high: 2 };
    let flagged = session
        .uid_flags(FlagFetch::Range(range), false)
        .await
        .unwrap();
    assert_eq!(flagged.len(), 2);
    assert!(flagged.iter().all(|found| found.labels.is_none()));
    let items = FetchItems {
        structure: false,
        gmail: false,
    };
    let fetched = session.uid_fetch(range, items).await.unwrap();
    assert_eq!(fetched.len(), 2);
    assert!(fetched.iter().all(|message| message.gmail.is_none()));
}

#[tokio::test]
async fn one_row_per_x_gm_msgid_even_when_a_folder_answers_two_uids_under_it() {
    let twin = Message {
        msgid: Message::new(1).msgid,
        ..Message::new(2)
    };
    let rig = start_gmail(Mailboxes::gmail(vec![Message::new(1), twin])).await;
    assert_eq!(rig.sync(ALL_MAIL).await, (Step::Done, 1));
    let created = rig.created(0);
    assert_eq!(created.len(), 1);
    // A batch writes its messages by UID, so the lower one keeps the id.
    assert_eq!(emails(&rig, &created, &["size"]).await[0]["size"], 1001);
    assert_eq!(mailbox(&rig, INBOX).await["totalEmails"], 1);
}

#[tokio::test]
async fn the_session_object_says_no_limit_on_mailboxes_per_email_for_a_gmail_account() {
    let rig = start_gmail(Mailboxes::gmail(Vec::new())).await;
    let urls = Urls {
        api: "/api/jmap/a1".to_owned(),
        download: "/api/jmap/a1/download".to_owned(),
        upload: "/api/jmap/a1/upload".to_owned(),
        event_source: "/api/jmap/a1/events".to_owned(),
    };
    let bytes = session_object(&rig.cache, "sanne@example.test", &urls).unwrap();
    let session: Value = serde_json::from_slice(&bytes).unwrap();
    let mail = &session["accounts"][ACCOUNT]["accountCapabilities"][MAIL_CAPABILITY];
    assert_eq!(mail["maxMailboxesPerEmail"], Value::Null);
}
