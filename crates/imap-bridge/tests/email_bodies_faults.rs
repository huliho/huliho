// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! `Email/get` over a message the server cannot describe, refuses or
//! does not hold, a value nothing decodes, a header field past the
//! fetch bound and every refusal of an ask, against the scripted
//! server.

mod bodies_rig;
mod stable_ids;
mod sync_rig;

use std::collections::HashMap;
use std::time::Duration;

use bodies_rig::{
    BODY, CLIENT_CAP, LONG_HEADER, PLAIN, SIX, ask, dataset, fetches, get, started, started_over,
};
use huliho_imap_bridge::jmap::{MAX_BODIES_IN_GET, MAX_BODY_WINDOWS};
use huliho_imap_bridge::runtime::Link;
use huliho_imap_bridge::session::{MAX_HEADER_BYTES, MAX_NESTING};
use huliho_imap_bridge::testing::parts::CORPUS_PLAIN;
use huliho_imap_bridge::testing::{Behavior, Message, TestConnector};
use serde_json::{Value, json};
use sync_rig::{ACCOUNT, Rig, inbox};

/// The bytes a header field fetch is cut at.
const FIELDS_BYTES: usize = MAX_HEADER_BYTES;

/// The sign-ins the server saw, one per session opened.
fn logins(rig: &Rig) -> usize {
    rig.fake
        .lines()
        .iter()
        .filter(|line| line.contains(" LOGIN "))
        .count()
}

/// A synced rig over a server that misbehaves as `behavior` says.
async fn started_with(
    behavior: Behavior,
) -> (Rig, HashMap<String, String>, HashMap<String, String>) {
    let mut mailboxes = inbox(dataset());
    mailboxes.behavior = behavior;
    started_over(mailboxes).await
}

#[tokio::test]
async fn an_unknown_charset_marks_the_value_and_a_message_the_server_cannot_describe_is_one_attachment()
 {
    let (rig, names, by_name) = started().await;
    let bogus = get(
        &rig,
        ask(&[&by_name["e5"]], &["bodyValues", "textBody"], CLIENT_CAP),
    )
    .await;
    let value = &bogus["list"][0]["bodyValues"]["1"];
    assert_eq!(value["value"], "hello");
    assert_eq!(value["isEncodingProblem"], true);
    let before = logins(&rig);
    let mut properties = BODY.to_vec();
    properties.push("size");
    let answer = get(
        &rig,
        ask(&[&by_name["e6"], &by_name["e7"]], &properties, CLIENT_CAP),
    )
    .await;
    let mut deep = answer["list"][0].clone();
    stable_ids::rename(&mut deep, &names);
    assert_eq!(deep["textBody"], json!([]));
    assert_eq!(deep["htmlBody"], json!([]));
    assert_eq!(deep["bodyValues"], json!({}));
    let attachment = &deep["attachments"][0];
    assert_eq!(attachment["type"], "application/octet-stream");
    assert_eq!(attachment["name"], "message.eml");
    assert_eq!(attachment["disposition"], "attachment");
    assert_eq!(attachment["size"], deep["size"]);
    assert_eq!(attachment["blobId"], "e6");
    assert_eq!(deep["bodyStructure"]["type"], "application/octet-stream");
    // The message after it still answers, on a fresh session.
    assert_eq!(
        answer["list"][1]["bodyValues"]["1"]["value"],
        "Body of message 7."
    );
    assert_eq!(logins(&rig), before + 1, "{:?}", rig.fake.lines());
}

#[tokio::test]
async fn a_structure_the_bridge_cannot_read_answers_as_one_attachment_on_a_fresh_session() {
    let (rig, _, by_name) = started().await;
    let mailboxes = &rig.fake.script().mailboxes;
    mailboxes.expunge_uid("INBOX", PLAIN);
    mailboxes.append(
        "INBOX",
        Message {
            structure: "(\"TEXT\" \"PLAIN\" NIL NIL NIL \"7BIT\" x 1)".to_owned(),
            ..Message::new(PLAIN)
        },
    );
    // The first body ask opens the conversation's session; the count
    // starts after it.
    get(&rig, ask(&[&by_name["e1"]], &BODY, CLIENT_CAP)).await;
    let before = logins(&rig);
    let mut properties = BODY.to_vec();
    properties.push("size");
    let answer = get(
        &rig,
        ask(&[&by_name["e7"], &by_name["e1"]], &properties, CLIENT_CAP),
    )
    .await;
    let unread = &answer["list"][0];
    assert_eq!(unread["attachments"][0]["name"], "message.eml");
    assert_eq!(unread["attachments"][0]["size"], unread["size"]);
    assert_eq!(unread["bodyValues"], json!({}));
    assert_eq!(
        answer["list"][1]["bodyValues"]["1.1"]["value"],
        CORPUS_PLAIN
    );
    assert_eq!(logins(&rig), before + 1, "{:?}", rig.fake.lines());
}

#[tokio::test]
async fn a_third_message_the_server_cannot_describe_in_one_call_leaves_the_rest_absent() {
    let mut mail = dataset();
    mail.push(Message::new(10).nested(MAX_NESTING + 8));
    mail.push(Message::new(11).nested(MAX_NESTING + 8));
    let (rig, _, by_name) = started_over(inbox(mail)).await;
    get(&rig, ask(&[&by_name["e1"]], &BODY, CLIENT_CAP)).await;
    let before = logins(&rig);
    let ids: [&str; 4] = [
        &by_name["e6"],
        &by_name["e10"],
        &by_name["e11"],
        &by_name["e7"],
    ];
    let answer = get(&rig, ask(&ids, &BODY, CLIENT_CAP)).await;
    assert_eq!(answer["type"], "serverUnavailable", "{answer}");
    assert_eq!(logins(&rig), before + 2, "{:?}", rig.fake.lines());
    let ids: [&str; 3] = [&by_name["e6"], &by_name["e10"], &by_name["e7"]];
    let answer = get(&rig, ask(&ids, &BODY, CLIENT_CAP)).await;
    assert_eq!(
        answer["list"][2]["bodyValues"]["1"]["value"],
        "Body of message 7."
    );
}

#[tokio::test]
async fn a_request_spends_thirty_two_windows_at_most_and_the_last_value_is_cut() {
    let (rig, _, by_name) = started().await;
    let calls_needed = MAX_BODY_WINDOWS / SIX + 1;
    let calls: Vec<Value> = (0..calls_needed)
        .map(|n| {
            let arguments = ask(&[&by_name["e3"]], &["bodyValues", "htmlBody"], CLIENT_CAP);
            json!(["Email/get", arguments, format!("c{n}")])
        })
        .collect();
    let response = rig.calls(&calls).await;
    let responses = response["methodResponses"].as_array().unwrap();
    let first = &responses[0][1]["list"][0]["bodyValues"]["1"];
    let last = &responses[calls_needed - 1][1]["list"][0]["bodyValues"]["1"];
    assert_eq!(first["isTruncated"], false);
    assert_eq!(last["isTruncated"], true);
    let (whole, cut) = (
        first["value"].as_str().unwrap(),
        last["value"].as_str().unwrap(),
    );
    assert!(whole.starts_with(cut));
    assert!(cut.len() < whole.len());
    let windows = fetches(&rig)
        .iter()
        .filter(|line| line.contains("]<"))
        .count();
    assert_eq!(windows, MAX_BODY_WINDOWS);
}

#[tokio::test]
async fn a_get_without_a_row_to_read_fetches_nothing_under_the_default_properties() {
    let (rig, _, _) = started().await;
    let empty = get(&rig, json!({ "accountId": ACCOUNT, "ids": [] })).await;
    assert_eq!(empty["list"], json!([]));
    assert_eq!(empty["notFound"], json!([]));
    let unknown = get(&rig, json!({ "accountId": ACCOUNT, "ids": ["e-nope"] })).await;
    assert_eq!(unknown["list"], json!([]));
    assert_eq!(unknown["notFound"], json!(["e-nope"]));
    assert!(fetches(&rig).is_empty());
}

#[tokio::test]
async fn a_no_on_the_messages_own_fetch_costs_that_call_alone_and_keeps_the_session() {
    let behavior = Behavior {
        refuses_body_of: Some(PLAIN),
        ..Behavior::default()
    };
    let (rig, _, by_name) = started_with(behavior).await;
    get(&rig, ask(&[&by_name["e1"]], &BODY, CLIENT_CAP)).await;
    let before = logins(&rig);
    let calls = [
        json!(["Email/get", ask(&[&by_name["e7"]], &BODY, CLIENT_CAP), "c1"]),
        json!(["Email/get", ask(&[&by_name["e1"]], &BODY, CLIENT_CAP), "c2"]),
    ];
    let response = rig.calls(&calls).await;
    let responses = response["methodResponses"].as_array().unwrap();
    assert_eq!(responses[0][0], "error", "{}", responses[0]);
    assert_eq!(responses[0][1]["type"], "serverUnavailable");
    assert_eq!(
        responses[1][1]["list"][0]["bodyValues"]["1.1"]["value"],
        CORPUS_PLAIN
    );
    assert_eq!(logins(&rig), before, "{:?}", rig.fake.lines());
}

#[tokio::test]
async fn a_window_answered_at_another_offset_costs_that_message_alone_and_no_fresh_session() {
    let behavior = Behavior {
        misplaced_origin: Some(0),
        ..Behavior::default()
    };
    let (rig, _, by_name) = started_with(behavior).await;
    // One window at offset zero reads as asked; the count starts after it.
    get(&rig, ask(&[&by_name["e1"]], &BODY, CLIENT_CAP)).await;
    let before = logins(&rig);
    let answer = get(
        &rig,
        ask(&[&by_name["e3"]], &["bodyValues", "htmlBody"], CLIENT_CAP),
    )
    .await;
    assert_eq!(answer["type"], "serverUnavailable", "{answer}");
    assert_eq!(logins(&rig), before, "{:?}", rig.fake.lines());
    let again = get(&rig, ask(&[&by_name["e1"]], &BODY, CLIENT_CAP)).await;
    assert_eq!(again["list"][0]["bodyValues"]["1.1"]["value"], CORPUS_PLAIN);
    assert_eq!(logins(&rig), before + 1, "{:?}", rig.fake.lines());
}

#[tokio::test]
async fn a_header_field_past_the_fetch_bound_is_cut_and_never_fails_the_call() {
    let (rig, _, by_name) = started().await;
    let properties = ["header:X-Long", "header:X-Long:asText"];
    let answer = get(
        &rig,
        json!({ "accountId": ACCOUNT, "ids": [by_name["e8"]], "properties": properties }),
    )
    .await;
    let email = &answer["list"][0];
    let raw = email["header:X-Long"].as_str().unwrap();
    assert!(raw.len() <= FIELDS_BYTES, "{}", raw.len());
    assert!(raw.len() > FIELDS_BYTES / 2, "{}", raw.len());
    assert!(raw.trim_start().chars().all(|c| c == 'x'));
    let text = email["header:X-Long:asText"].as_str().unwrap();
    assert!(text.len() <= FIELDS_BYTES);
    assert!(text.starts_with("xxx"));
    assert_eq!(
        fetches(&rig),
        [format!(
            "{LONG_HEADER} (UID BODY.PEEK[HEADER.FIELDS (X-Long)]<0.{FIELDS_BYTES}>)"
        )]
    );
}

#[tokio::test]
async fn a_body_ask_the_bridge_cannot_serve_is_refused_rfc8620_3_6_2() {
    let (rig, _, by_name) = started().await;
    let id = by_name["e1"].as_str();
    let many: Vec<String> = (0..=MAX_BODIES_IN_GET).map(|n| format!("e{n}")).collect();
    let cases = [
        (
            json!({ "accountId": ACCOUNT, "ids": [id], "bodyProperties": ["headers"] }),
            "invalidArguments",
        ),
        (
            json!({ "accountId": ACCOUNT, "ids": [id], "properties": ["id", "header:From:asText"] }),
            "invalidArguments",
        ),
        (
            json!({ "accountId": ACCOUNT, "ids": [id], "properties": ["id", "header:Subject:asDate"] }),
            "invalidArguments",
        ),
        (
            json!({ "accountId": ACCOUNT, "ids": [id], "properties": ["textBody", "value"] }),
            "invalidArguments",
        ),
        (
            json!({ "accountId": ACCOUNT, "ids": many, "properties": ["bodyValues"], "fetchHTMLBodyValues": true }),
            "requestTooLarge",
        ),
        (
            json!({ "accountId": ACCOUNT, "ids": many, "properties": ["bodyStructure"] }),
            "requestTooLarge",
        ),
        (
            json!({ "accountId": ACCOUNT, "ids": many, "properties": ["header:Subject"] }),
            "requestTooLarge",
        ),
    ];
    for (arguments, expected) in cases {
        assert_eq!(get(&rig, arguments).await["type"], expected, "{expected}");
    }
    assert!(fetches(&rig).is_empty());
}

#[tokio::test]
async fn a_message_the_server_does_not_hold_is_not_found_and_a_server_out_of_reach_is_unavailable()
{
    let (rig, _, by_name) = started().await;
    rig.fake.script().mailboxes.expunge_uid("INBOX", PLAIN);
    let answer = get(&rig, ask(&[&by_name["e7"]], &BODY, CLIENT_CAP)).await;
    assert_eq!(answer["list"], json!([]));
    assert_eq!(answer["notFound"], json!([by_name["e7"]]));
    let refusing = Rig {
        link: Link::with_interval(TestConnector::Refusing, Duration::ZERO),
        ..rig
    };
    let answer = get(&refusing, ask(&[&by_name["e1"]], &BODY, CLIENT_CAP)).await;
    assert_eq!(answer["type"], "serverUnavailable");
    let headers = get(
        &refusing,
        json!({ "accountId": ACCOUNT, "ids": [by_name["e1"]], "properties": ["subject"] }),
    )
    .await;
    assert_eq!(headers["list"][0]["subject"], "Message 1");
}
