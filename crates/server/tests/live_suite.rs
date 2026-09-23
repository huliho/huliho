// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The scenario suite against the compose targets through the router:
//! Cyrus over JMAP through the proxy and Dovecot through the bridge run
//! the same steps on the same seeded corpus. Then the latency bound of
//! an inbox window on the synced Dovecot mailbox and one Gmail account
//! through the same routes when the environment names it.

#![cfg(feature = "live-targets")]

mod answers;
mod common;
mod fake_dns;
mod live_rig;
mod signin;

use std::collections::HashMap;
use std::time::{Duration, Instant};

use live_rig::corpus::THREAD_SIZE;
use live_rig::{
    Editor, Live, Mail, Target, Window, clear, corpus_size, editor, expunge, flag, listed,
    number_of, seed, within,
};
use serde_json::{Value, json};

/// The scenario corpus: three threads.
const SCENARIO_CORPUS: u32 = 3 * THREAD_SIZE;

/// The inbox window the suite asks for.
const WINDOW: usize = 5;

/// Enough of the inbox to find the corpus among what else is there; the
/// bridge answers at most this many ids.
const LISTING: usize = 200;

/// How long the first sync of the scenario corpus may take.
const SCENARIO_PATIENCE: Duration = Duration::from_secs(60);

/// The bound on one window of the list: the query and the get of its
/// header properties, from the synced cache. Previews are fetched from
/// the server on demand and stay out of the window measured.
const QUERY_LATENCY_BOUND: Duration = Duration::from_millis(100);

/// The list window the latency bound is measured on and how many
/// windows are measured after the warm-up.
const LATENCY_WINDOW: usize = 50;
const MEASURED_WINDOWS: usize = 5;

/// The properties a list window asks for.
const LIST_PROPERTIES: [&str; 8] = [
    "id",
    "threadId",
    "mailboxIds",
    "keywords",
    "receivedAt",
    "subject",
    "from",
    "hasAttachment",
];

/// How long the first sync of the latency corpus may take: a floor plus
/// a share per message.
const SYNC_FLOOR: Duration = Duration::from_secs(60);
const SYNC_PER_MESSAGE: Duration = Duration::from_millis(10);

/// How long a Gmail account's stores may take to sync.
const GMAIL_PATIENCE: Duration = Duration::from_secs(300);

/// The roles Dovecot lists with the compose defaults.
const DOVECOT_ROLES: [&str; 6] = ["inbox", "drafts", "sent", "archive", "junk", "trash"];

/// The variables that name the Gmail test account.
const GMAIL_ADDRESS: &str = "HULIHO_LIVE_GMAIL_ADDRESS";
const GMAIL_APP_PASSWORD: &str = "HULIHO_LIVE_GMAIL_APP_PASSWORD";

/// Two tests seed the compose Dovecot's inbox, so they take turns.
static DOVECOT: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The email id and thread id of every corpus message, by number.
type Corpus = HashMap<u32, (String, String)>;

fn names(answer: &Value, list: &str, id: &str) -> bool {
    listed(answer, list).iter().any(|found| found == id)
}

/// The corpus messages the inbox holds.
async fn corpus_ids(live: &Live, mail: &Mail, inbox: &str) -> Corpus {
    let window = Window {
        mailbox: inbox,
        position: 0,
        limit: LISTING,
        collapse: false,
        total: false,
    };
    let ids = listed(&live.query(mail, window).await, "ids");
    let mut found = HashMap::new();
    for email in live
        .emails(mail, &ids, &["id", "messageId", "threadId"])
        .await
    {
        if let Some(number) = number_of(&email) {
            let id = email["id"].as_str().unwrap().to_owned();
            let thread = email["threadId"].as_str().unwrap().to_owned();
            found.insert(number, (id, thread));
        }
    }
    found
}

/// The account left as it was found: the corpus gone, the editor out,
/// the row removed.
async fn leave(live: &Live, mail: &Mail, mut editor: Editor) {
    clear(&mut editor).await;
    within(editor.logout()).await.unwrap();
    live.remove(mail).await;
}

/// The steps every target runs: the mailbox list with its roles, the
/// inbox window, thread grouping, then a delivery, a flag and an
/// expunge from the second connection.
async fn suite(target: Target) {
    let live = Live::start().await;
    let mut editor = editor(target).await;
    clear(&mut editor).await;
    seed(&mut editor, 1, SCENARIO_CORPUS).await;
    let mail = live.add(&target.body(), target.is_bridge()).await;
    let mut roles: Vec<&str> = Vec::new();
    let mailboxes = live.wait_listed(&mail, SCENARIO_PATIENCE).await;
    roles.extend(
        mailboxes
            .iter()
            .filter_map(|mailbox| mailbox["role"].as_str()),
    );
    assert_eq!(
        roles.iter().filter(|role| **role == "inbox").count(),
        1,
        "{roles:?}"
    );
    if target == Target::Dovecot {
        roles.sort_unstable();
        let mut expected = DOVECOT_ROLES.to_vec();
        expected.sort_unstable();
        assert_eq!(roles, expected);
    }
    let inbox = live.wait_synced(&mail, "inbox", SCENARIO_PATIENCE).await;
    let inbox = inbox["id"].as_str().unwrap().to_owned();
    let window = Window {
        mailbox: &inbox,
        position: 0,
        limit: WINDOW,
        collapse: false,
        total: true,
    };
    let newest = live.query(&mail, window).await;
    let ids = listed(&newest, "ids");
    assert_eq!(ids.len(), WINDOW, "{newest}");
    assert!(newest["total"].as_u64().unwrap() >= u64::from(SCENARIO_CORPUS));
    let emails = live.emails(&mail, &ids, &["messageId", "receivedAt"]).await;
    let numbers: Vec<Option<u32>> = emails.iter().map(number_of).collect();
    let expected: Vec<Option<u32>> = (0..WINDOW)
        .map(|offset| Some(SCENARIO_CORPUS - u32::try_from(offset).unwrap()))
        .collect();
    assert_eq!(numbers, expected, "{emails:?}");
    let corpus = corpus_ids(&live, &mail, &inbox).await;
    assert_eq!(corpus.len(), usize::try_from(SCENARIO_CORPUS).unwrap());
    threads(&live, &mail, &inbox, &corpus).await;
    delivery(&live, &mail, &inbox, &mut editor).await;
    flagging(&live, &mail, &corpus, &mut editor).await;
    expunging(&live, &mail, &corpus, &mut editor).await;
    leave(&live, &mail, editor).await;
}

/// The four of the first thread share a thread the next root does not;
/// the thread lists them; the collapsed window shows one per thread.
async fn threads(live: &Live, mail: &Mail, inbox: &str, corpus: &Corpus) {
    let first = &corpus[&1].1;
    for number in 2..=THREAD_SIZE {
        assert_eq!(&corpus[&number].1, first, "{number}");
    }
    assert_ne!(&corpus[&(THREAD_SIZE + 1)].1, first);
    let found = live
        .call(
            mail,
            "Thread/get",
            json!({ "accountId": mail.account, "ids": [first] }),
        )
        .await;
    let members = listed(&found["list"][0], "emailIds");
    assert_eq!(
        members.len(),
        usize::try_from(THREAD_SIZE).unwrap(),
        "{found}"
    );
    for number in 1..=THREAD_SIZE {
        assert!(members.contains(&corpus[&number].0), "{number}");
    }
    let window = Window {
        mailbox: inbox,
        position: 0,
        limit: LISTING,
        collapse: true,
        total: false,
    };
    let collapsed = listed(&live.query(mail, window).await, "ids");
    let of_corpus = collapsed
        .iter()
        .filter(|id| corpus.values().any(|(own, _)| own == *id))
        .count();
    assert_eq!(
        of_corpus,
        usize::try_from(SCENARIO_CORPUS / THREAD_SIZE).unwrap(),
        "one email per thread"
    );
}

/// A delivery from the second connection reaches the inbox.
async fn delivery(live: &Live, mail: &Mail, inbox: &str, editor: &mut Editor) {
    let before = live.state(mail).await;
    let delivered = SCENARIO_CORPUS + 1;
    seed(editor, delivered, delivered).await;
    let changed = live
        .changes_showing(mail, &before, |answer| {
            !listed(answer, "created").is_empty()
        })
        .await;
    let created = listed(&changed, "created");
    let arrived = live
        .emails(mail, &created, &["messageId", "mailboxIds"])
        .await;
    let ours = arrived
        .iter()
        .find(|email| number_of(email) == Some(delivered))
        .unwrap_or_else(|| panic!("{changed}"));
    assert_eq!(ours["mailboxIds"][inbox], true, "{ours}");
}

/// A flag set from the second connection shows as a keyword.
async fn flagging(live: &Live, mail: &Mail, corpus: &Corpus, editor: &mut Editor) {
    let before = live.state(mail).await;
    flag(editor, 1, "\\Flagged").await;
    let first = corpus[&1].0.clone();
    let changed = live
        .changes_showing(mail, &before, |answer| names(answer, "updated", &first))
        .await;
    assert!(names(&changed, "updated", &first), "{changed}");
    let flagged = live.emails(mail, &[first], &["keywords"]).await.remove(0);
    assert_eq!(flagged["keywords"]["$flagged"], true, "{flagged}");
}

/// An expunge from the second connection destroys the email.
async fn expunging(live: &Live, mail: &Mail, corpus: &Corpus, editor: &mut Editor) {
    let before = live.state(mail).await;
    expunge(editor, 2).await;
    let second = corpus[&2].0.clone();
    let changed = live
        .changes_showing(mail, &before, |answer| names(answer, "destroyed", &second))
        .await;
    assert!(names(&changed, "destroyed", &second), "{changed}");
}

#[tokio::test]
async fn cyrus_runs_the_scenario_suite_through_the_proxy() {
    suite(Target::Cyrus).await;
}

#[tokio::test]
async fn dovecot_runs_the_scenario_suite_through_the_bridge() {
    let _dovecot = DOVECOT.lock().await;
    suite(Target::Dovecot).await;
}

#[tokio::test]
async fn dovecot_answers_an_inbox_window_within_the_latency_bound() {
    let _dovecot = DOVECOT.lock().await;
    let corpus = corpus_size();
    let live = Live::start().await;
    let mut editor = editor(Target::Dovecot).await;
    clear(&mut editor).await;
    seed(&mut editor, 1, corpus).await;
    let mail = live.add(&Target::Dovecot.body(), true).await;
    let patience = SYNC_FLOOR + SYNC_PER_MESSAGE * corpus;
    let inbox = live.wait_synced(&mail, "inbox", patience).await;
    assert!(inbox["totalEmails"].as_u64().unwrap() >= u64::from(corpus));
    let inbox = inbox["id"].as_str().unwrap().to_owned();
    // The first window warms the cache and asks the total, as a client
    // opening the list does; the measured ones page on without it.
    for round in 0..=MEASURED_WINDOWS {
        let window = Window {
            mailbox: &inbox,
            position: round * LATENCY_WINDOW,
            limit: LATENCY_WINDOW,
            collapse: true,
            total: round == 0,
        };
        let started = Instant::now();
        let ids = listed(&live.query(&mail, window).await, "ids");
        let queried = started.elapsed();
        let emails = live.emails(&mail, &ids, &LIST_PROPERTIES).await;
        let took = started.elapsed();
        assert_eq!(emails.len(), ids.len());
        assert!(
            round == 0 || took <= QUERY_LATENCY_BOUND,
            "window {round} took {took:?}, the query {queried:?} of it"
        );
    }
    leave(&live, &mail, editor).await;
}

/// The Gmail test account, when the environment names it.
fn gmail_body() -> Option<Value> {
    let named = |variable: &str| {
        std::env::var(variable)
            .ok()
            .filter(|value| !value.is_empty())
    };
    let address = named(GMAIL_ADDRESS)?;
    let password = named(GMAIL_APP_PASSWORD)?;
    Some(json!({
        "address": address,
        "provider": "gmail",
        "target": {
            "kind": "imap",
            "username": address,
            "imap": { "host": "imap.gmail.com", "port": 993, "tls": "implicit" },
            "smtp": { "host": "smtp.gmail.com", "port": 465, "tls": "implicit" },
        },
        "credential": { "kind": "password", "password": password },
    }))
}

#[tokio::test]
async fn a_gmail_account_lists_its_roles_syncs_its_stores_and_answers_a_window() {
    let Some(body) = gmail_body() else {
        return;
    };
    let live = Live::public().await;
    let mail = live.add(&body, true).await;
    let roles: Vec<String> = live
        .wait_listed(&mail, GMAIL_PATIENCE)
        .await
        .iter()
        .filter_map(|mailbox| mailbox["role"].as_str().map(str::to_owned))
        .collect();
    for role in ["inbox", "archive", "junk", "trash"] {
        assert!(roles.iter().any(|found| found == role), "{roles:?}");
    }
    let all_mail = live.wait_synced(&mail, "archive", GMAIL_PATIENCE).await;
    let inbox = live.wait_synced(&mail, "inbox", GMAIL_PATIENCE).await;
    let window = Window {
        mailbox: inbox["id"].as_str().unwrap(),
        position: 0,
        limit: WINDOW,
        collapse: true,
        total: true,
    };
    let answered = live.query(&mail, window).await;
    assert!(answered["ids"].is_array(), "{answered}");
    assert!(
        answered["total"].as_u64().unwrap() <= all_mail["totalEmails"].as_u64().unwrap(),
        "{answered}"
    );
    live.remove(&mail).await;
}
