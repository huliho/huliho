// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! `Email/get` over a synced folder: the objects as a snapshot and
//! every refusal the method answers.

mod sync_rig;

use std::collections::HashMap;
use std::sync::Arc;

use huliho_imap_bridge::jmap::MAX_OBJECTS_IN_GET;
use huliho_imap_bridge::sync::Cache;
use huliho_imap_bridge::testing::Message;
use huliho_imap_bridge::testing::seal::TestSealer;
use serde_json::{Value, json};
use sync_rig::{ACCOUNT, Rig, inbox};

const THREAD_HEADER: &str = "From: \"Visser, Sanne\" <sanne@example.test>\r\n\
    Sender: desk@example.test\r\n\
    Reply-To: replies@example.test\r\n\
    To: team: mo@example.test, Li <li@example.test>;\r\n\
    Cc: kim@example.test\r\n\
    Bcc: audit@example.test\r\n\
    Subject: =?UTF-8?Q?Caf=C3=A9_om_drie_uur?=\r\n\
    Date: Tue, 1 Sep 2026 09:30:00 +0200\r\n\
    Message-ID: <m2@example.test>\r\n\
    In-Reply-To: <m1@example.test>\r\n\
    References: <m0@example.test> <m1@example.test>\r\n\r\n";

/// Three messages that between them fill every served property.
fn dataset() -> Vec<Message> {
    vec![
        Message::new(1),
        Message {
            header: THREAD_HEADER.to_owned(),
            ..Message::new(2)
                .flagged(&["\\Answered", "$Forwarded", "\\Recent"])
                .with_attachment()
        },
        Message {
            header: "\r\n".to_owned(),
            ..Message::new(3).flagged(&["\\Flagged", "\\Draft"])
        },
    ]
}

/// The list by UID with every random id swapped for a name that holds
/// across runs. The scripted size of a message is a thousand
/// above its UID, so email `e2` with thread `t2` is the message of UID
/// 2.
fn stable(rig: &Rig, list: &Value) -> Value {
    let mut emails = list.as_array().unwrap().clone();
    emails.sort_by_key(|email| email["size"].as_u64());
    let mut names = HashMap::from([(rig.folder("INBOX").id.to_string(), "INBOX".to_owned())]);
    for email in &emails {
        let uid = email["size"].as_u64().unwrap() - 1000;
        names.insert(email["id"].as_str().unwrap().to_owned(), format!("e{uid}"));
        let thread = email["threadId"].as_str().unwrap().to_owned();
        names.insert(thread, format!("t{uid}"));
    }
    let mut list = Value::Array(emails);
    rename(&mut list, &names);
    list
}

fn rename(value: &mut Value, names: &HashMap<String, String>) {
    match value {
        Value::String(text) => {
            if let Some(name) = names.get(text.as_str()) {
                text.clone_from(name);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|item| rename(item, names)),
        Value::Object(fields) => {
            let renamed = std::mem::take(fields)
                .into_iter()
                .map(|(key, mut field)| {
                    rename(&mut field, names);
                    (names.get(&key).cloned().unwrap_or(key), field)
                })
                .collect();
            *fields = renamed;
        }
        _ => {}
    }
}

fn get(rig: &Rig, arguments: &Value) -> Value {
    rig.call(&json!(["Email/get", arguments, "c1"]))
}

#[tokio::test]
async fn email_get_answers_the_metadata_and_the_header_properties_rfc8621_4_2() {
    let rig = Rig::start(inbox(dataset())).await;
    rig.sync("INBOX").await;
    let ids = rig.created(0);
    let answer = get(&rig, &json!({ "accountId": ACCOUNT, "ids": ids }));
    assert_eq!(answer["state"], "2");
    assert_eq!(answer["notFound"], json!([]));
    insta::assert_json_snapshot!(stable(&rig, &answer["list"]));
}

#[tokio::test]
async fn email_get_cuts_to_the_properties_and_names_what_it_does_not_hold_rfc8620_5_1() {
    let rig = Rig::start(inbox(dataset())).await;
    rig.sync("INBOX").await;
    let ids = rig.created(0);
    let asked = json!([ids[0], "e-unknown", ids[0]]);
    let answer = get(
        &rig,
        &json!({ "accountId": ACCOUNT, "ids": asked, "properties": ["subject"] }),
    );
    assert_eq!(answer["notFound"], json!(["e-unknown"]));
    let list = answer["list"].as_array().unwrap();
    assert_eq!(list.len(), 1);
    let fields: Vec<&String> = list[0].as_object().unwrap().keys().collect();
    assert_eq!(fields, ["id", "subject"]);
}

#[tokio::test]
async fn email_get_refuses_what_it_cannot_serve_rfc8620_3_6_2() {
    let rig = Rig::start(inbox(dataset())).await;
    rig.sync("INBOX").await;
    let ids = rig.created(0);
    let cases = [
        (
            json!({ "accountId": ACCOUNT, "ids": null }),
            "requestTooLarge",
        ),
        (
            json!({ "accountId": ACCOUNT, "ids": vec!["e"; MAX_OBJECTS_IN_GET + 1] }),
            "requestTooLarge",
        ),
        (
            json!({ "accountId": ACCOUNT, "ids": ids, "properties": ["textBody"] }),
            "invalidArguments",
        ),
        (
            json!({ "accountId": ACCOUNT, "ids": ids, "properties": ["preview"] }),
            "invalidArguments",
        ),
        (
            json!({ "accountId": ACCOUNT, "ids": ids, "fetchAllBodyValues": true }),
            "invalidArguments",
        ),
        (
            json!({ "accountId": "other", "ids": ids }),
            "accountNotFound",
        ),
    ];
    for (arguments, expected) in cases {
        assert_eq!(get(&rig, &arguments)["type"], expected, "{expected}");
    }
}

#[tokio::test]
async fn a_blob_the_host_does_not_open_is_a_server_failure_for_that_call() {
    let rig = Rig::start(inbox(dataset())).await;
    rig.sync("INBOX").await;
    let ids = rig.created(0);
    let locked = Rig {
        cache: Cache {
            sealer: Arc::new(TestSealer { locked: true }),
            ..rig.cache.clone()
        },
        ..rig
    };
    let answer = get(&locked, &json!({ "accountId": ACCOUNT, "ids": ids }));
    assert_eq!(answer["type"], "serverFail");
    let plain = get(
        &locked,
        &json!({ "accountId": ACCOUNT, "ids": [], "properties": ["id"] }),
    );
    assert_eq!(plain["list"], json!([]));
}

#[tokio::test]
async fn ids_travel_from_an_earlier_call_into_email_get_rfc8620_3_7() {
    let rig = Rig::start(inbox(dataset())).await;
    rig.sync("INBOX").await;
    let ids = rig.created(0);
    let body = json!({
        "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
        "methodCalls": [
            ["Core/echo", { "ids": ids }, "c0"],
            ["Email/get", {
                "accountId": ACCOUNT,
                "#ids": { "resultOf": "c0", "name": "Core/echo", "path": "/ids" },
                "properties": ["threadId"]
            }, "c1"]
        ]
    });
    let bytes = huliho_imap_bridge::jmap::handle(
        &rig.cache.store,
        rig.cache.sealer.as_ref(),
        &rig.cache.key,
        &serde_json::to_vec(&body).unwrap(),
    )
    .unwrap();
    let response: Value = serde_json::from_slice(&bytes).unwrap();
    let list = response["methodResponses"][1][1]["list"]
        .as_array()
        .unwrap();
    assert_eq!(list.len(), 3);
    assert!(list.iter().all(|email| email["threadId"].is_string()));
}
