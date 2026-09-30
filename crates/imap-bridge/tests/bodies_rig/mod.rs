// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The messages the body tests read and the calls they make: a corpus
//! with every kind of part, a base64 part of six windows, a part past
//! the cap, a charset nobody decodes, a structure the guard refuses, a
//! plain message and one with a header field past the fetch bound.

use std::collections::HashMap;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use huliho_imap_bridge::jmap::MAX_BODY_VALUE_BYTES;
use huliho_imap_bridge::session::{BODY_WINDOW_BYTES, MAX_HEADER_BYTES, MAX_NESTING};
use huliho_imap_bridge::testing::parts::corpus;
use huliho_imap_bridge::testing::{Mailboxes, Message};
use serde_json::{Value, json};

use crate::stable_ids;
use crate::sync_rig::{ACCOUNT, Rig, inbox};

/// The cap the client asks for a body.
pub const CLIENT_CAP: u64 = 4 * 1024 * 1024;

/// The windows the base64 part takes.
pub const SIX: usize = 6;

/// The five body properties.
pub const BODY: [&str; 5] = [
    "bodyStructure",
    "textBody",
    "htmlBody",
    "attachments",
    "bodyValues",
];

/// The UIDs of the dataset, by what each message is for.
pub const CORPUS: u32 = 1;
pub const BASE64_HTML: u32 = 3;
pub const PAST_CAP: u32 = 4;
pub const BOGUS_CHARSET: u32 = 5;
pub const TOO_DEEP: u32 = 6;
pub const PLAIN: u32 = 7;
pub const LONG_HEADER: u32 = 8;
/// A base64 part of three two-byte characters, eight symbols on the wire.
pub const MULTIBYTE: u32 = 9;

/// A header field past what a field fetch is cut at.
const LONG_FIELD_BYTES: usize = MAX_HEADER_BYTES + 4096;

/// The bytes of text behind a base64 part of exactly six windows: four
/// encoded bytes per three of text.
pub fn six_windows_of_text() -> String {
    let bytes = SIX * usize::try_from(BODY_WINDOW_BYTES).unwrap() * 3 / 4;
    let unit = "<p>Six windows of text.</p>\r\n";
    let mut text = unit.repeat(bytes.div_ceil(unit.len()));
    text.truncate(bytes);
    text
}

/// A plain message with one header field of `LONG_FIELD_BYTES`.
fn long_header(uid: u32) -> Message {
    let plain = Message::new(uid);
    let lines = plain.header.trim_end_matches("\r\n");
    Message {
        header: format!(
            "{lines}\r\nX-Long: {}\r\n\r\n",
            "x".repeat(LONG_FIELD_BYTES)
        ),
        ..plain
    }
}

/// A message of one text part under the given subtype and charset,
/// encoding and bytes.
fn one_part(
    uid: u32,
    (subtype, charset): (&str, &str),
    encoding: Option<&str>,
    body: String,
) -> Message {
    let word = encoding.unwrap_or("7bit").to_ascii_uppercase();
    let structure = format!(
        "(\"TEXT\" \"{}\" (\"CHARSET\" \"{charset}\") NIL NIL \"{word}\" {} 1)",
        subtype.to_ascii_uppercase(),
        body.len()
    );
    Message {
        structure,
        body,
        content_type: format!("text/{subtype}; charset={charset}"),
        transfer_encoding: encoding.map(str::to_owned),
        ..Message::new(uid)
    }
}

/// The messages of the inbox, by the UIDs above.
pub fn dataset() -> Vec<Message> {
    let encoded = BASE64.encode(six_windows_of_text());
    let past_cap = "x".repeat(usize::try_from(MAX_BODY_VALUE_BYTES).unwrap() + 1024);
    vec![
        corpus(CORPUS),
        Message::new(2),
        one_part(BASE64_HTML, ("html", "utf-8"), Some("base64"), encoded),
        one_part(PAST_CAP, ("plain", "utf-8"), None, past_cap),
        one_part(
            BOGUS_CHARSET,
            ("plain", "x-bogus"),
            None,
            "hello".to_owned(),
        ),
        Message::new(TOO_DEEP).nested(MAX_NESTING + 8),
        Message::new(PLAIN),
        long_header(LONG_HEADER),
        one_part(
            MULTIBYTE,
            ("plain", "utf-8"),
            Some("base64"),
            BASE64.encode("\u{e9}\u{e9}\u{e9}"),
        ),
    ]
}

/// The body request the client sends, with these properties.
pub fn ask(ids: &[&str], properties: &[&str], cap: u64) -> Value {
    json!({
        "accountId": ACCOUNT,
        "ids": ids,
        "properties": properties,
        "fetchTextBodyValues": true,
        "fetchHTMLBodyValues": true,
        "maxBodyValueBytes": cap,
    })
}

pub async fn get(rig: &Rig, arguments: Value) -> Value {
    rig.call(&json!(["Email/get", arguments, "c1"])).await
}

/// A synced rig over the dataset with the names of its emails,
/// `e<uid>`, both ways.
pub async fn started() -> (Rig, HashMap<String, String>, HashMap<String, String>) {
    started_over(inbox(dataset())).await
}

/// The same over the given server.
pub async fn started_over(
    mailboxes: Mailboxes,
) -> (Rig, HashMap<String, String>, HashMap<String, String>) {
    let rig = Rig::start(mailboxes).await;
    rig.sync("INBOX").await;
    let ids = rig.created(0);
    let listed = get(
        &rig,
        json!({ "accountId": ACCOUNT, "ids": ids, "properties": ["size", "threadId"] }),
    )
    .await;
    let names = stable_ids::names(&rig, listed["list"].as_array().unwrap());
    let by_name: HashMap<String, String> = names
        .iter()
        .map(|(id, name)| (name.clone(), id.clone()))
        .collect();
    (rig, names, by_name)
}

/// Every body fetch the server received, without its tag: neither the
/// sync's header fetches nor the preview fetches.
pub fn fetches(rig: &Rig) -> Vec<String> {
    rig.fake
        .lines()
        .iter()
        .filter(|line| line.contains("UID FETCH"))
        .filter(|line| !line.contains("INTERNALDATE") && !line.contains("CONTENT-TYPE"))
        .filter(|line| !line.contains(".MIME]"))
        .filter_map(|line| line.split_once("UID FETCH "))
        .map(|(_, rest)| rest.to_owned())
        .collect()
}
