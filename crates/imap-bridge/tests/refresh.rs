// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The on-demand refresh against the scripted server: what another
//! client did to a folder shows in `/changes` once, a refresh runs at
//! most once per interval and a server that is down costs nothing.

mod sync_rig;

use std::time::Duration;

use huliho_imap_bridge::runtime::Link;
use huliho_imap_bridge::session::{MAX_FLAGGED, TlsMode};
use huliho_imap_bridge::sync::{REFRESH_GAP, SYNC_BATCH};
use huliho_imap_bridge::testing::imap::HOST;
use huliho_imap_bridge::testing::{Behavior, Extension, Folder, Mailboxes, Message, TestConnector};
use serde_json::{Value, json};
use sync_rig::{ACCOUNT, Rig, inbox};

const INBOX: &str = "INBOX";

/// Room for a loopback exchange.
const STEP: Duration = Duration::from_secs(2);

/// An interval no test outlasts.
const PATIENT: Duration = Duration::from_secs(3600);

/// The messages `1..=count`.
fn mail(count: u32) -> Vec<Message> {
    (1..=count).map(Message::new).collect()
}

/// A reply to messages 1 and 3, so it names two threads.
fn reply_to_two(uid: u32) -> Message {
    Message {
        header: format!(
            "From: Sanne <sanne@example.test>\r\nTo: mo@example.test\r\nSubject: Re: Message 1\r\nMessage-ID: <m{uid}@example.test>\r\nReferences: <m1@example.test> <m3@example.test>\r\n\r\n"
        ),
        ..Message::new(uid)
    }
}

/// `<Type>/changes` since a state.
async fn changes(rig: &Rig, object: &str, since: u64) -> Value {
    let arguments = json!({ "accountId": ACCOUNT, "sinceState": since.to_string() });
    rig.call(&json!([format!("{object}/changes"), arguments, "c1"]))
        .await
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

/// The lengths of the three lists of a `/changes` answer.
fn lengths(answer: &Value) -> (usize, usize, usize) {
    let count = |list: &str| answer[list].as_array().unwrap().len();
    (count("created"), count("updated"), count("destroyed"))
}

/// The state a `/changes` answer reached.
fn new_state(answer: &Value) -> u64 {
    answer["newState"].as_str().unwrap().parse().unwrap()
}

/// One property of the emails with these ids, in the order asked.
async fn property(rig: &Rig, ids: &[String], name: &str) -> Vec<Value> {
    let arguments = json!({ "accountId": ACCOUNT, "ids": ids, "properties": [name] });
    rig.call(&json!(["Email/get", arguments, "c1"])).await["list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|email| email[name].clone())
        .collect()
}

/// `totalEmails` of the INBOX as `Mailbox/get` answers it.
async fn total_emails(rig: &Rig) -> Value {
    let id = rig.folder(INBOX).id;
    let arguments = json!({ "accountId": ACCOUNT, "ids": [id], "properties": ["totalEmails"] });
    rig.call(&json!(["Mailbox/get", arguments, "c1"])).await["list"][0]["totalEmails"].take()
}

/// The mod-sequence every CHANGEDSINCE fetch the server received asked
/// from, in order.
fn changed_since(rig: &Rig) -> Vec<String> {
    rig.fake
        .lines()
        .iter()
        .filter_map(|line| line.split_once("(CHANGEDSINCE "))
        .map(|(_, tail)| tail.trim_end_matches(')').to_owned())
        .collect()
}

/// The ranges of the plain flag scans the server received, in order.
fn scanned(rig: &Rig) -> Vec<String> {
    rig.fake
        .lines()
        .iter()
        .filter_map(|line| line.split_once("UID FETCH "))
        .filter_map(|(_, rest)| rest.strip_suffix(" (UID FLAGS)"))
        .map(str::to_owned)
        .collect()
}

/// How many lines the server received hold `needle`.
fn received(rig: &Rig, needle: &str) -> usize {
    rig.fake
        .lines()
        .iter()
        .filter(|line| line.contains(needle))
        .count()
}

/// The same rig behind a link with this interval.
fn relinked(rig: Rig, interval: Duration) -> Rig {
    let target = rig.fake.target(HOST, TlsMode::Implicit);
    let connector = TestConnector::scripted(rig.fake.trusting(), target, STEP);
    Rig {
        link: Link::with_interval(connector, interval),
        ..rig
    }
}

#[tokio::test]
async fn new_mail_is_created_and_a_flag_change_is_updated_under_condstore_rfc7162_3_1_4_1() {
    let model = inbox(mail(3));
    let rig = Rig::start(model.clone()).await;
    rig.sync(INBOX).await;
    let synced = rig.cache.store.state(&rig.cache.key).unwrap();
    model.append(INBOX, Message::new(4));
    let arrived = changes(&rig, "Email", synced).await;
    assert_eq!(lengths(&arrived), (1, 0, 0));
    // The mailbox pass of the refresh is one state, the new mail another.
    assert_eq!(new_state(&arrived), synced + 2);
    let created = listed(&arrived, "created");
    assert_eq!(
        property(&rig, &created, "subject").await,
        [json!("Message 4")]
    );
    assert_eq!(lengths(&changes(&rig, "Thread", synced).await), (1, 0, 0));
    assert_eq!(total_emails(&rig).await, 4);
    let before = new_state(&arrived);
    model.store_flags(INBOX, 2, &["\\Seen", "\\Flagged"]);
    let flagged = changes(&rig, "Email", before).await;
    assert_eq!(lengths(&flagged), (0, 1, 0));
    let updated = listed(&flagged, "updated");
    assert_eq!(
        property(&rig, &updated, "keywords").await,
        [json!({ "$seen": true, "$flagged": true })]
    );
    assert_eq!(lengths(&changes(&rig, "Thread", before).await), (0, 0, 0));
    // The two refreshes that found a change asked from the mod-sequence
    // the folder stood at before; the two that found none asked nothing.
    assert_eq!(changed_since(&rig), ["1", "2"]);
    assert!(scanned(&rig).is_empty());
}

#[tokio::test]
async fn a_folder_that_stands_as_the_cache_left_it_costs_no_command() {
    let model = inbox(mail(3));
    let rig = Rig::start(model.clone()).await;
    rig.sync(INBOX).await;
    let synced = rig.cache.store.state(&rig.cache.key).unwrap();
    let examined = received(&rig, " EXAMINE ");
    for _ in 0..2 {
        assert_eq!(lengths(&changes(&rig, "Email", synced).await), (0, 0, 0));
    }
    assert_eq!(received(&rig, " EXAMINE "), examined);
    assert!(changed_since(&rig).is_empty());
    model.store_flags(INBOX, 3, &["\\Seen", "\\Flagged"]);
    assert_eq!(lengths(&changes(&rig, "Email", synced).await), (0, 1, 0));
    assert_eq!(received(&rig, " EXAMINE "), examined + 1);
}

#[tokio::test]
async fn a_uidnext_far_above_the_recorded_one_takes_the_uid_list_in_its_windows() {
    let model = inbox(mail(3));
    let rig = Rig::start(model.clone()).await;
    rig.sync(INBOX).await;
    let synced = rig.cache.store.state(&rig.cache.key).unwrap();
    let jumped = 4 + REFRESH_GAP;
    model.append(INBOX, Message::new(jumped));
    let arrived = changes(&rig, "Email", synced).await;
    assert_eq!(lengths(&arrived), (1, 0, 0));
    assert_eq!(
        property(&rig, &listed(&arrived, "created"), "subject").await,
        [json!(format!("Message {jumped}"))]
    );
    assert_eq!(received(&rig, "UID SEARCH 1:4"), 1);
    assert_eq!(received(&rig, &format!("UID FETCH {jumped}:{jumped} ")), 1);
    assert_eq!(received(&rig, "UID FETCH 4:"), 0, "no walk range by range");
}

#[tokio::test]
async fn a_changedsince_answer_past_the_bound_puts_the_folder_on_the_scan_from_then_on() {
    let count = u32::try_from(MAX_FLAGGED).unwrap() + 1;
    let model = inbox(mail(count));
    let rig = Rig::start(model.clone()).await;
    rig.sync(INBOX).await;
    let synced = rig.cache.store.state(&rig.cache.key).unwrap();
    model.mark_all(INBOX, &["\\Seen", "\\Flagged"]);
    let cut = changes(&rig, "Email", synced).await;
    assert_eq!(
        lengths(&cut),
        (0, 0, 0),
        "the answer past the bound ends the refresh"
    );
    assert_eq!(changed_since(&rig), ["1"]);
    assert!(scanned(&rig).is_empty());
    let scans = usize::try_from(count).unwrap().div_ceil(SYNC_BATCH);
    // Ten thousand and one updates pass the horizon of the log, so the
    // client that asks from before them refetches its windows.
    let scanned_once = changes(&rig, "Email", synced).await;
    assert_eq!(scanned_once["type"], "cannotCalculateChanges");
    assert_eq!(scanned(&rig).len(), scans);
    assert_eq!(changed_since(&rig), ["1"]);
    let inbox_id = rig.folder(INBOX).id;
    let newest = json!({ "accountId": ACCOUNT, "filter": { "inMailbox": inbox_id }, "limit": 1 });
    let window = rig.call(&json!(["Email/query", newest, "c1"])).await;
    let sample = listed(&window, "ids");
    let arguments =
        json!({ "accountId": ACCOUNT, "ids": sample, "properties": ["keywords", "size"] });
    let email = rig.call(&json!(["Email/get", arguments, "c1"])).await["list"][0].take();
    assert_eq!(
        email["keywords"],
        json!({ "$seen": true, "$flagged": true })
    );
    let uid = u32::try_from(email["size"].as_u64().unwrap() - 1000).unwrap();
    let scanned_state = rig.cache.store.state(&rig.cache.key).unwrap();
    model.store_flags(INBOX, uid, &["\\Seen"]);
    let again = changes(&rig, "Email", scanned_state).await;
    assert_eq!(lengths(&again), (0, 1, 0));
    assert_eq!(listed(&again, "updated"), sample);
    assert_eq!(
        scanned(&rig).len(),
        2 * scans,
        "the folder is scanned from then on"
    );
    assert_eq!(changed_since(&rig), ["1"]);
}

#[tokio::test]
async fn without_condstore_only_the_folder_the_client_looks_at_gets_its_flags_scanned() {
    let mut extensions = Extension::all();
    extensions.remove(&Extension::Condstore);
    let folders = vec![
        Folder::new(INBOX).with_mail(mail(3)),
        Folder::new("Work").with_mail((11..=13).map(Message::new).collect()),
    ];
    let model = Mailboxes::new(folders, extensions);
    let rig = Rig::start(model.clone()).await;
    rig.sync(INBOX).await;
    rig.sync("Work").await;
    let synced = rig.cache.store.state(&rig.cache.key).unwrap();
    model.store_flags(INBOX, 2, &["\\Seen", "\\Flagged"]);
    model.store_flags("Work", 12, &["\\Seen", "\\Flagged"]);
    let unseen = changes(&rig, "Email", synced).await;
    assert_eq!(lengths(&unseen), (0, 0, 0), "no folder is looked at yet");
    assert!(scanned(&rig).is_empty());
    let inbox_id = rig.folder(INBOX).id;
    let query = json!({ "accountId": ACCOUNT, "filter": { "inMailbox": inbox_id } });
    rig.call(&json!(["Email/query", query, "c1"])).await;
    let seen = changes(&rig, "Email", synced).await;
    assert_eq!(lengths(&seen), (0, 1, 0));
    assert_eq!(
        property(&rig, &listed(&seen, "updated"), "subject").await,
        [json!("Message 2")]
    );
    assert_eq!(scanned(&rig), ["1:3"]);
    assert!(changed_since(&rig).is_empty());
}

#[tokio::test]
async fn a_count_that_falls_short_is_an_expunge_and_one_out_one_in_is_told_by_the_sum() {
    let model = inbox(mail(3));
    let rig = Rig::start(model.clone()).await;
    rig.sync(INBOX).await;
    let synced = rig.cache.store.state(&rig.cache.key).unwrap();
    model.expunge_uid(INBOX, 1);
    let gone = changes(&rig, "Email", synced).await;
    assert_eq!(lengths(&gone), (0, 0, 1));
    assert_eq!(lengths(&changes(&rig, "Thread", synced).await), (0, 0, 1));
    assert_eq!(total_emails(&rig).await, 2);
    let destroyed = listed(&gone, "destroyed");
    let arguments = json!({ "accountId": ACCOUNT, "ids": destroyed, "properties": ["id"] });
    let left = rig.call(&json!(["Email/get", arguments, "c1"])).await;
    assert_eq!(left["notFound"], json!(destroyed));
    let before = rig.cache.store.state(&rig.cache.key).unwrap();
    model.expunge_uid(INBOX, 2);
    model.append(INBOX, Message::new(4));
    let swapped = changes(&rig, "Email", before).await;
    assert_eq!(lengths(&swapped), (1, 0, 1));
    assert_eq!(
        property(&rig, &listed(&swapped, "created"), "subject").await,
        [json!("Message 4")]
    );
    assert_eq!(total_emails(&rig).await, 2);
    assert_eq!(
        received(&rig, "UID SEARCH 1:2"),
        2,
        "the UID list came in the windows of the first sync"
    );
}

#[tokio::test]
async fn a_refresh_runs_at_most_once_per_interval() {
    let model = inbox(mail(3));
    let rig = Rig::start(model.clone()).await;
    rig.sync(INBOX).await;
    let synced = rig.cache.store.state(&rig.cache.key).unwrap();
    let patient = relinked(rig, PATIENT);
    let listed_before = received(&patient, " LIST ");
    changes(&patient, "Email", synced).await;
    changes(&patient, "Email", synced).await;
    assert_eq!(received(&patient, " LIST ") - listed_before, 1);
    model.append(INBOX, Message::new(4));
    let waiting = changes(&patient, "Email", synced).await;
    assert_eq!(
        lengths(&waiting),
        (0, 0, 0),
        "the next refresh waits for the interval"
    );
    assert_eq!(received(&patient, " LIST ") - listed_before, 1);
}

#[tokio::test]
async fn a_connection_the_connector_refuses_fails_no_call() {
    let rig = Rig::start(inbox(mail(2))).await;
    rig.sync(INBOX).await;
    let synced = rig.cache.store.state(&rig.cache.key).unwrap();
    let ids = rig.created(0);
    let down = Rig {
        link: Link::new(TestConnector::Refusing),
        ..rig
    };
    let answer = changes(&down, "Email", synced).await;
    assert_eq!(lengths(&answer), (0, 0, 0));
    assert_eq!(answer["newState"], synced.to_string());
    let arguments = json!({ "accountId": ACCOUNT, "ids": ids });
    let emails = down.call(&json!(["Email/get", arguments, "c1"])).await;
    assert_eq!(emails["state"], synced.to_string());
    let previews: Vec<&Value> = emails["list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|email| &email["preview"])
        .collect();
    assert_eq!(previews, [&json!(""), &json!("")]);
    assert_eq!(down.cache.store.state(&down.cache.key).unwrap(), synced);
}

#[tokio::test]
async fn a_session_that_ends_inside_a_refresh_costs_that_refresh_alone() {
    let mut model = inbox(mail(3));
    model.behavior = Behavior {
        drops_after: Some(1),
        ..Behavior::default()
    };
    let rig = Rig::start(model.clone()).await;
    rig.sync(INBOX).await;
    let synced = rig.cache.store.state(&rig.cache.key).unwrap();
    model.append(INBOX, Message::new(4));
    model.store_flags(INBOX, 2, &["\\Seen", "\\Flagged"]);
    // The new mail is the one fetch the connection answers; the flag
    // fetch closes it.
    let first = changes(&rig, "Email", synced).await;
    assert_eq!(lengths(&first), (1, 0, 0));
    // A message leaves before the next refresh, which the count on
    // record tells although the refresh before it recorded nothing.
    model.expunge_uid(INBOX, 1);
    let second = changes(&rig, "Email", new_state(&first)).await;
    assert_eq!(lengths(&second), (0, 1, 1));
    assert_eq!(
        property(&rig, &listed(&second, "updated"), "subject").await,
        [json!("Message 2")]
    );
    assert_eq!(changed_since(&rig), ["1", "1"]);
    assert_eq!(total_emails(&rig).await, 3);
}

#[tokio::test]
async fn a_reply_that_arrives_and_names_two_threads_shows_the_merge_in_both_logs_rfc8621_3_2() {
    let model = inbox(mail(3));
    let rig = Rig::start(model.clone()).await;
    rig.sync(INBOX).await;
    let synced = rig.cache.store.state(&rig.cache.key).unwrap();
    model.append(INBOX, reply_to_two(4));
    let emails = changes(&rig, "Email", synced).await;
    assert_eq!(lengths(&emails), (1, 1, 0));
    let threads = changes(&rig, "Thread", synced).await;
    assert_eq!(lengths(&threads), (0, 1, 1));
    let survivor = listed(&threads, "updated").remove(0);
    let arguments = json!({ "accountId": ACCOUNT, "ids": [survivor] });
    let thread = rig.call(&json!(["Thread/get", arguments, "c1"])).await;
    let email_ids: Vec<String> = thread["list"][0]["emailIds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| id.as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        property(&rig, &email_ids, "subject").await,
        [
            json!("Message 1"),
            json!("Message 3"),
            json!("Re: Message 1")
        ]
    );
    assert_eq!(listed(&emails, "created"), [email_ids[2].clone()]);
    let moved = listed(&emails, "updated");
    assert_eq!(
        property(&rig, &moved, "threadId").await,
        [json!(survivor)],
        "the email of the thread that ended moved to the survivor"
    );
}
