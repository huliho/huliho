// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The previews of a synced folder: fetched once per email in partial
//! fetches grouped by part and length, decoded and cut, taken from HTML
//! where no plain part exists and served empty for a message stored
//! without its structure.

mod stable_ids;
mod sync_rig;

use std::collections::BTreeMap;

use huliho_imap_bridge::jmap::PREVIEW_BATCH;
use huliho_imap_bridge::session::MAX_NESTING;
use huliho_imap_bridge::sync::preview::PREVIEW_CHARS;
use huliho_imap_bridge::testing::Message;
use serde_json::{Value, json};
use sync_rig::{ACCOUNT, Rig, inbox};

/// Words in the plain body that runs past the preview.
const LONG_WORDS: usize = 400;

/// CSS declarations a newsletter opens with, more than the two KiB a
/// plain part is asked for.
const CSS_DECLARATIONS: usize = 130;

/// Paragraphs of the HTML body that passes 64 KiB.
const LARGE_PARAGRAPHS: usize = 5200;

/// The header a preview fetch of a message of one part asks for.
const ONE_PART_HEADER: &str =
    "BODY.PEEK[HEADER.FIELDS (CONTENT-TYPE CONTENT-TRANSFER-ENCODING)]<0.4096>";

fn dataset() -> Vec<Message> {
    let newsletter = format!(
        "<html><head><style>{}</style></head><body><p>Newsletter text</p></body></html>",
        "p { color: red } ".repeat(CSS_DECLARATIONS)
    );
    let large = format!(
        "<html><body><p>Large newsletter</p>{}</body></html>",
        "<p>filler</p>".repeat(LARGE_PARAGRAPHS)
    );
    vec![
        Message::new(1),
        Message::new(2).encoded("quoted-printable", "Caf=C3=A9 om drie uur."),
        Message {
            content_type: "text/plain; charset=iso-8859-1".to_owned(),
            ..Message::new(3).encoded("base64", "Q2Fm6SBvbSBkcmll")
        },
        Message {
            body: "word ".repeat(LONG_WORDS),
            ..Message::new(4)
        },
        Message::new(5).html(&newsletter),
        Message::new(6).html(&large),
        Message::new(7).with_attachment(),
        Message::new(8).nested(MAX_NESTING + 8),
    ]
}

/// `Email/get` with the preview and what names an email.
async fn get(rig: &Rig, ids: &[String]) -> Value {
    let arguments = json!({
        "accountId": ACCOUNT,
        "ids": ids,
        "properties": ["preview", "size", "threadId"]
    });
    rig.call(&json!(["Email/get", arguments, "c1"])).await
}

/// Every preview fetch the server received, without its tag.
fn asks(rig: &Rig) -> Vec<String> {
    rig.fake
        .lines()
        .iter()
        .filter(|line| line.contains("BODY.PEEK[") && !line.contains("INTERNALDATE"))
        .filter_map(|line| line.split_once("UID FETCH "))
        .map(|(_, rest)| rest.to_owned())
        .collect()
}

#[tokio::test]
async fn previews_are_fetched_once_in_partial_fetches_and_decoded_rfc8621_4_1_4() {
    let rig = Rig::start(inbox(dataset())).await;
    rig.sync("INBOX").await;
    let synced = rig.cache.store.state(&rig.cache.key).unwrap();
    let ids = rig.created(0);
    let first = get(&rig, &ids).await;
    let names = stable_ids::names(&rig, first["list"].as_array().unwrap());
    let by_name = |answer: &Value| -> BTreeMap<String, String> {
        answer["list"]
            .as_array()
            .unwrap()
            .iter()
            .map(|email| {
                let name = names[email["id"].as_str().unwrap()].clone();
                (name, email["preview"].as_str().unwrap().to_owned())
            })
            .collect()
    };
    let previews = by_name(&first);
    assert_eq!(previews["e1"], "Body of message 1.");
    assert_eq!(previews["e2"], "Caf\u{e9} om drie uur.");
    assert_eq!(previews["e3"], "Caf\u{e9} om drie");
    assert_eq!(previews["e4"].chars().count(), PREVIEW_CHARS);
    assert!(previews["e4"].starts_with("word word"));
    assert_eq!(previews["e5"], "Newsletter text");
    assert!(previews["e6"].starts_with("Large newsletter filler filler"));
    assert_eq!(previews["e6"].chars().count(), PREVIEW_CHARS);
    assert_eq!(previews["e7"], "Body of message 7.");
    assert_eq!(
        previews["e8"], "",
        "a message stored without its structure has no part to read"
    );
    assert_eq!(first["state"], (synced + 1).to_string());
    let since = json!({ "accountId": ACCOUNT, "sinceState": synced.to_string() });
    let mut changed = rig.call(&json!(["Email/changes", since, "c1"])).await;
    stable_ids::rename(&mut changed, &names);
    let mut updated: Vec<&str> = changed["updated"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| id.as_str().unwrap())
        .collect();
    updated.sort_unstable();
    assert_eq!(updated, ["e1", "e2", "e3", "e4", "e5", "e6", "e7"]);
    assert_eq!(
        asks(&rig),
        [
            format!("1:4 (UID {ONE_PART_HEADER} BODY.PEEK[TEXT]<0.2048>)"),
            format!("6 (UID {ONE_PART_HEADER} BODY.PEEK[TEXT]<0.16384>)"),
            format!("5 (UID {ONE_PART_HEADER} BODY.PEEK[TEXT]<0.65536>)"),
            "7 (UID BODY.PEEK[1.MIME]<0.4096> BODY.PEEK[1]<0.2048>)".to_owned(),
        ]
    );
    let again = get(&rig, &ids).await;
    assert_eq!(again["state"], first["state"]);
    assert_eq!(by_name(&again), previews);
    assert_eq!(asks(&rig).len(), 4, "a second call asks nothing more");
}

#[tokio::test]
async fn one_call_fetches_a_hundred_previews_and_the_next_one_the_rest() {
    let count = u32::try_from(PREVIEW_BATCH).unwrap() + 1;
    let rig = Rig::start(inbox((1..=count).map(Message::new).collect())).await;
    rig.sync("INBOX").await;
    let ids = rig.created(0);
    let empty = |answer: &Value| {
        answer["list"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|email| email["preview"] == "")
            .count()
    };
    let first = get(&rig, &ids).await;
    assert_eq!(empty(&first), 1);
    assert_eq!(asks(&rig).len(), 1);
    let again = get(&rig, &ids).await;
    assert_eq!(empty(&again), 0);
    assert_eq!(asks(&rig).len(), 2);
    let first_state: u64 = first["state"].as_str().unwrap().parse().unwrap();
    assert_eq!(again["state"], (first_state + 1).to_string());
}
