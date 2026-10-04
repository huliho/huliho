// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! `Email/set` among the calls of a request and among the requests of
//! an account: every call answers what the calls before it left and
//! nothing of the calls behind it (RFC 8620 section 3.10), a write no
//! pass is left for sends nothing and two requests that name one state
//! never both store.

mod set_rig;
mod sync_rig;

use huliho_imap_bridge::store::ObjectType;
use huliho_imap_bridge::testing::Message;
use serde_json::{Value, json};
use set_rig::{Set, refused, started};
use sync_rig::{ACCOUNT, inbox};

const INBOX: &str = "INBOX";

/// An unseen message and a seen one.
async fn synced() -> Set {
    let mail = vec![Message::new(1).flagged(&[]), Message::new(2)];
    started(inbox(mail), INBOX, false).await
}

/// The arguments of an `Email/set` that patches the email of that UID
/// when the account stands at that state.
fn patching(set: &Set, uid: u32, patch: &Value, state: &str) -> Value {
    json!({
        "accountId": ACCOUNT,
        "ifInState": state,
        "update": { &set.ids[&uid]: patch },
    })
}

/// The patch that gives `$seen` this value.
fn seen(value: bool) -> Value {
    json!({ "keywords/$seen": value })
}

/// The patch that sets `$flagged`.
fn flagged() -> Value {
    json!({ "keywords/$flagged": true })
}

/// An `Email/set` call that gives the email of that UID a keyword.
fn marking(set: &Set, uid: u32, keyword: &str, call: &str) -> Value {
    let update = json!({ &set.ids[&uid]: { format!("keywords/{keyword}"): true } });
    json!(["Email/set", { "accountId": ACCOUNT, "update": update }, call])
}

/// An `Email/get` call for the keywords of both emails.
fn keywords(set: &Set, call: &str) -> Value {
    let arguments = json!({
        "accountId": ACCOUNT,
        "ids": [set.ids[&1], set.ids[&2]],
        "properties": ["keywords"],
    });
    json!(["Email/get", arguments, call])
}

/// The keywords an `Email/get` answer holds for the email of that UID.
fn held<'a>(set: &Set, got: &'a Value, uid: u32) -> &'a Value {
    let list = got["list"].as_array().unwrap();
    let email = list.iter().find(|email| email["id"] == set.ids[&uid]);
    &email.unwrap()["keywords"]
}

#[tokio::test]
async fn a_second_set_of_one_request_meets_the_state_the_first_left() {
    let set = synced().await;
    let state = set.state().to_string();
    let first = patching(&set, 1, &seen(true), &state);
    let second = patching(&set, 2, &flagged(), &state);
    let answered = set
        .rig
        .calls(&[
            json!(["Email/set", first, "c1"]),
            json!(["Email/set", second, "c2"]),
            keywords(&set, "c3"),
        ])
        .await;
    let responses = answered["methodResponses"].as_array().unwrap();
    assert_eq!(responses[0][0], "Email/set", "{answered}");
    assert_eq!(responses[1][1]["type"], "stateMismatch");
    assert_eq!(*held(&set, &responses[2][1], 1), json!({ "$seen": true }));
    assert_eq!(responses[2][1]["state"], responses[0][1]["newState"]);
    assert_eq!(
        set.flags(INBOX, 2),
        ["\\Seen"],
        "the second set sent nothing"
    );
}

#[tokio::test]
async fn a_first_set_that_stores_nothing_leaves_its_state_to_the_second() {
    let set = synced().await;
    let state = set.state().to_string();
    let first = patching(&set, 1, &seen(false), &state);
    let second = patching(&set, 2, &flagged(), &state);
    let answered = set
        .rig
        .calls(&[
            json!(["Email/set", first, "c1"]),
            json!(["Email/set", second, "c2"]),
        ])
        .await;
    let responses = answered["methodResponses"].as_array().unwrap();
    assert_eq!(
        refused(&set, &responses[0][1], 1),
        "invalidProperties",
        "{answered}"
    );
    assert_eq!(responses[0][1]["newState"], state);
    assert_eq!(responses[1][1]["oldState"], state, "{answered}");
    assert_eq!(responses[1][1]["updated"], json!({ &set.ids[&2]: null }));
}

#[tokio::test]
async fn a_get_ahead_of_a_set_answers_what_stood_before_it_rfc8620_3_10() {
    let set = synced().await;
    let get = json!({
        "accountId": ACCOUNT,
        "ids": [set.ids[&1]],
        "properties": ["keywords", "textBody", "bodyValues"],
        "fetchTextBodyValues": true,
    });
    let answered = set
        .rig
        .calls(&[
            json!(["Email/get", get, "c1"]),
            marking(&set, 1, "$seen", "c2"),
            json!(["Email/get", get, "c3"]),
        ])
        .await;
    let responses = answered["methodResponses"].as_array().unwrap();
    let before = &responses[0][1]["list"][0];
    assert_eq!(before["keywords"], json!({}), "{answered}");
    assert!(before["bodyValues"]["1"]["value"].is_string());
    assert_eq!(responses[1][1]["updated"], json!({ &set.ids[&1]: null }));
    let after = &responses[2][1]["list"][0];
    assert_eq!(after["keywords"], json!({ "$seen": true }));
    assert_eq!(responses[2][1]["state"], responses[1][1]["newState"]);
    assert_eq!(set.written().len(), 2, "one SELECT and one STORE");
}

#[tokio::test]
async fn a_get_between_two_sets_answers_what_the_first_left_rfc8620_3_10() {
    let set = synced().await;
    let before = set.state();
    let state_of_the_get = json!({ "resultOf": "c2", "name": "Email/get", "path": "/state" });
    let second = json!({
        "accountId": ACCOUNT,
        "#ifInState": state_of_the_get,
        "update": { &set.ids[&2]: { "keywords/$flagged": true } },
    });
    let answered = set
        .rig
        .calls(&[
            marking(&set, 1, "$seen", "c1"),
            keywords(&set, "c2"),
            json!(["Email/set", second, "c3"]),
            keywords(&set, "c4"),
        ])
        .await;
    let responses = answered["methodResponses"].as_array().unwrap();
    let between = &responses[1][1];
    assert_eq!(between["state"], (before + 1).to_string(), "{answered}");
    assert_eq!(*held(&set, between, 1), json!({ "$seen": true }));
    assert_eq!(*held(&set, between, 2), json!({ "$seen": true }));
    let stored = &responses[2][1];
    assert_eq!(stored["oldState"], between["state"], "{answered}");
    assert_eq!(stored["newState"], (before + 2).to_string());
    assert_eq!(stored["updated"], json!({ &set.ids[&2]: null }));
    let both = json!({ "$seen": true, "$flagged": true });
    assert_eq!(*held(&set, &responses[3][1], 2), both);
    assert_eq!(responses[3][1]["state"], stored["newState"]);
    assert_eq!(set.updated(ObjectType::Email, before).len(), 2);
}

#[tokio::test]
async fn a_write_no_pass_is_left_for_is_unavailable_and_sends_nothing() {
    let set = synced().await;
    let before = set.state();
    let answered = set
        .rig
        .calls(&[
            marking(&set, 1, "$seen", "c1"),
            keywords(&set, "c2"),
            marking(&set, 1, "$flagged", "c3"),
            keywords(&set, "c4"),
            marking(&set, 1, "$answered", "c5"),
        ])
        .await;
    let responses = answered["methodResponses"].as_array().unwrap();
    let flagged = json!({ "$seen": true, "$flagged": true });
    assert_eq!(*held(&set, &responses[1][1], 1), json!({ "$seen": true }));
    assert_eq!(*held(&set, &responses[3][1], 1), flagged, "{answered}");
    assert_eq!(responses[4][0], "error", "{answered}");
    assert_eq!(responses[4][1]["type"], "serverUnavailable");
    assert_eq!(set.state(), before + 2);
    assert_eq!(set.keywords(1).await, flagged);
    assert_eq!(set.flags(INBOX, 1), ["\\Seen", "\\Flagged"]);
    let stores = set.written();
    assert_eq!(stores.len(), 4, "two SELECTs and two STOREs: {stores:?}");
    // A request of its own stores what was left.
    let alone = set
        .update(&[(1, json!({ "keywords/$answered": true }))])
        .await;
    assert_eq!(alone["updated"], json!({ &set.ids[&1]: null }), "{alone}");
}

#[tokio::test]
async fn two_requests_that_name_one_state_never_both_store_rfc8620_5_3() {
    let set = synced().await;
    let state = set.state();
    let named = state.to_string();
    let first = patching(&set, 1, &seen(true), &named);
    let second = patching(&set, 2, &flagged(), &named);
    let (one, other) = tokio::join!(set.set(first), set.set(second));
    let kinds = [one, other].map(|answer| {
        let kind = answer["type"].as_str().unwrap_or("stored");
        kind.to_owned()
    });
    let stored = kinds.iter().filter(|kind| *kind == "stored").count();
    let behind = kinds.iter().filter(|kind| *kind == "stateMismatch").count();
    assert_eq!((stored, behind), (1, 1), "{kinds:?}");
    assert_eq!(set.state(), state + 1);
    assert_eq!(set.written().len(), 2, "one SELECT and one STORE");
}
