// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A search on its own connection under the deadline the searches of
//! its request's round share: past it the call answers
//! `serverUnavailable` and the connection goes, a later search of the
//! round connects nowhere, a request given up on drops its connection,
//! the conversation answers meanwhile, a second search waits its turn or
//! reads the hits of an equal one and the hits answer a repeat until
//! four other searches pushed them out.

use std::pin::pin;
use std::time::Duration;

use huliho_imap_bridge::runtime::{SEARCH_CACHE_ENTRIES, Timing};
use huliho_imap_bridge::store::{ChangeKind, ObjectType};
use huliho_imap_bridge::testing::Behavior;
use serde_json::{Value, json};
use tokio::time::{Instant, sleep, timeout};

use crate::search_rig::{behaving, commands, dovecot, found, id_of, mark, query, quick, synced};
use crate::sync_rig::{ACCOUNT, Rig};

/// How long the server sits on a SEARCH a test waits out: well past the
/// look between two counts, so the search is still held when the test
/// moves on.
const BRIEF: Duration = Duration::from_secs(1);

/// How long the server sits on a SEARCH no test waits out: well past
/// the short deadline.
const LONG: Duration = Duration::from_secs(5);

/// A deadline the server's long sit runs past, with room for the
/// connect ahead of the SEARCH on a loaded runner.
const SHORT: Duration = Duration::from_secs(2);

/// A deadline a brief sit stays inside and the bound on a wait for the
/// server.
const ROOMY: Duration = Duration::from_secs(10);

/// Between two counts of what the server received.
const LOOK: Duration = Duration::from_millis(20);

/// The Dovecot account behind a server that sits on every SEARCH for
/// `delay`, the searches under `deadline`.
async fn slow(delay: Duration, deadline: Duration) -> Rig {
    let mailboxes = behaving(
        dovecot(),
        Behavior {
            search_delay: Some(delay),
            ..Behavior::default()
        },
    );
    let timing = Timing {
        search: deadline,
        ..quick()
    };
    synced(mailboxes, false, timing).await
}

/// A search of the inbox for the word.
fn inbox(rig: &Rig, text: &str) -> Value {
    json!({ "inMailbox": id_of(rig, "INBOX"), "text": text })
}

/// The searches with criteria the server received after the mark; the
/// UID list of a sync or a refresh carries none.
fn searches(rig: &Rig, from: usize) -> usize {
    commands(rig, from, "UID SEARCH CHARSET ").len()
}

fn logins(rig: &Rig, from: usize) -> usize {
    commands(rig, from, "LOGIN ").len()
}

/// Waits until the server holds a search received after the mark.
async fn held(rig: &Rig, from: usize) {
    let holds = async {
        while searches(rig, from) == 0 {
            sleep(LOOK).await;
        }
    };
    timeout(ROOMY, holds)
        .await
        .expect("the server received a search");
}

#[tokio::test]
async fn a_search_past_its_deadline_answers_server_unavailable_and_its_connection_goes() {
    let rig = slow(LONG, SHORT).await;
    let from = mark(&rig);
    for _ in 0..2 {
        let started = Instant::now();
        let answer = query(&rig, inbox(&rig, "plan"), json!({})).await;
        assert_eq!(answer["type"], "serverUnavailable", "{answer}");
        assert!(started.elapsed() < LONG, "{:?}", started.elapsed());
    }
    assert_eq!(logins(&rig, from), 2, "the second search connected afresh");
}

#[tokio::test]
async fn the_searches_of_one_request_share_a_deadline_and_a_call_past_it_connects_nowhere() {
    let rig = slow(LONG, SHORT).await;
    let from = mark(&rig);
    let calls: Vec<Value> = ["plan", "zoë"]
        .into_iter()
        .map(|text| {
            let arguments = json!({ "accountId": ACCOUNT, "filter": inbox(&rig, text) });
            json!(["Email/query", arguments, text])
        })
        .collect();
    let response = rig.calls(&calls).await;
    let answers = response["methodResponses"].as_array().unwrap();
    assert_eq!(answers.len(), 2, "{response}");
    for answer in answers {
        assert_eq!(answer[1]["type"], "serverUnavailable", "{answer}");
    }
    // Under a deadline per call the second call would sign in again, so
    // the bound tells the two apart without a clock.
    assert!(logins(&rig, from) <= 1, "the second call never connected");
    assert!(searches(&rig, from) <= 1, "the second call never searched");
}

#[tokio::test]
async fn the_conversation_answers_while_a_slow_search_runs() {
    let rig = slow(LONG, ROOMY).await;
    let from = mark(&rig);
    let mut search = pin!(found(&rig, inbox(&rig, "plan")));
    tokio::select! {
        found = &mut search => panic!("the search answered {found:?}"),
        () = held(&rig, from) => {}
    }
    let changes = json!({ "accountId": ACCOUNT, "sinceState": "0" });
    let refresh = json!(["Email/changes", changes, "c"]);
    tokio::select! {
        found = &mut search => panic!("the search answered {found:?}"),
        answer = rig.call(&refresh) => {
            assert!(answer["newState"].is_string(), "{answer}");
        }
    }
}

#[tokio::test]
async fn a_search_given_up_on_drops_its_connection_and_the_next_one_connects_again() {
    let rig = slow(BRIEF, ROOMY).await;
    let from = mark(&rig);
    tokio::select! {
        answer = query(&rig, inbox(&rig, "plan"), json!({})) => {
            panic!("the search answered {answer}");
        }
        () = held(&rig, from) => {}
    }
    assert_eq!(found(&rig, inbox(&rig, "plan")).await, [3, 1]);
    assert_eq!(logins(&rig, from), 2);
    assert_eq!(searches(&rig, from), 2);
}

#[tokio::test]
async fn a_second_search_waits_for_the_first_on_the_one_connection_and_both_answer() {
    let rig = slow(BRIEF, ROOMY).await;
    let from = mark(&rig);
    let (plan, zoe) = tokio::join!(
        found(&rig, inbox(&rig, "plan")),
        found(&rig, inbox(&rig, "zoë"))
    );
    assert_eq!((plan, zoe), (vec![3, 1], vec![3, 1]));
    assert_eq!(logins(&rig, from), 1);
    assert_eq!(searches(&rig, from), 2);
}

#[tokio::test]
async fn two_equal_searches_at_once_cost_one_search_and_the_second_reads_its_hits() {
    let rig = slow(BRIEF, ROOMY).await;
    let from = mark(&rig);
    let (first, second) = tokio::join!(
        found(&rig, inbox(&rig, "plan")),
        found(&rig, inbox(&rig, "plan"))
    );
    assert_eq!((first, second), (vec![3, 1], vec![3, 1]));
    assert_eq!(logins(&rig, from), 1);
    assert_eq!(searches(&rig, from), 1);
}

#[tokio::test]
async fn a_repeat_answers_from_the_hits_until_four_other_searches_pushed_them_out() {
    let rig = synced(dovecot(), false, quick()).await;
    let from = mark(&rig);
    assert_eq!(found(&rig, inbox(&rig, "plan")).await, [3, 1]);
    assert_eq!(found(&rig, inbox(&rig, "plan")).await, [3, 1]);
    assert_eq!(searches(&rig, from), 1);
    let others = ["zoë", "thanks", "quarterly", "sends"];
    assert_eq!(others.len(), SEARCH_CACHE_ENTRIES);
    for word in others {
        found(&rig, inbox(&rig, word)).await;
    }
    assert_eq!(searches(&rig, from), 1 + SEARCH_CACHE_ENTRIES);
    found(&rig, inbox(&rig, "sends")).await;
    assert_eq!(searches(&rig, from), 1 + SEARCH_CACHE_ENTRIES);
    assert_eq!(found(&rig, inbox(&rig, "plan")).await, [3, 1]);
    assert_eq!(searches(&rig, from), 2 + SEARCH_CACHE_ENTRIES);
}

#[tokio::test]
async fn a_row_gone_since_the_search_drops_out_of_its_window_with_no_search_again() {
    let mailboxes = dovecot();
    let rig = synced(mailboxes.clone(), false, quick()).await;
    assert_eq!(found(&rig, inbox(&rig, "plan")).await, [3, 1]);
    let state = rig.cache.store.state(&rig.cache.key).unwrap();
    let from = mark(&rig);
    mailboxes.expunge_uid("INBOX", 3);
    let changes = json!({ "accountId": ACCOUNT, "sinceState": state.to_string() });
    rig.call(&json!(["Email/changes", changes, "c"])).await;
    let destroyed = rig
        .changes(ObjectType::Email, state)
        .into_iter()
        .filter(|(_, kind)| *kind == ChangeKind::Destroyed)
        .count();
    assert_eq!(destroyed, 1);
    assert_eq!(found(&rig, inbox(&rig, "plan")).await, [1]);
    assert_eq!(searches(&rig, from), 0);
}
