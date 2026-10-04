// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The messages the blob tests read and what they share: a corpus with
//! every kind of part, a base64 part and a quoted-printable part past a
//! window, plain messages of several windows, one of exactly one window
//! and one whose structure the guard refuses; a synced rig with a link
//! of its own for the blobs and the fetches the server saw.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Duration;

use huliho_imap_bridge::blob::{Blob, BlobError};
use huliho_imap_bridge::runtime::{CONVERSATION_DEADLINE, Link, Timing};
use huliho_imap_bridge::session::{BODY_WINDOW_BYTES, MAX_NESTING, TlsMode};
use huliho_imap_bridge::testing::imap::HOST;
use huliho_imap_bridge::testing::messages::SIZE_ABOVE_UID;
use huliho_imap_bridge::testing::parts::{base64_lines, corpus, varied};
use huliho_imap_bridge::testing::{Mailboxes, Message, TestConnector};
use serde_json::json;
use tokio::time::sleep;

use crate::sync_rig::{ACCOUNT, Rig, inbox};

/// Room for a loopback exchange.
const STEP: Duration = Duration::from_secs(2);

/// How often and how long a test looks for the server to sit still.
const LOOK: Duration = Duration::from_millis(20);
const LOOKS: usize = 250;

/// The looks without a new fetch after which the server sits still.
const STILL_LOOKS: u32 = 10;

/// A limit above every blob of the dataset.
pub const ROOMY: u64 = 64 * 1024 * 1024;

/// The UIDs of the dataset, by what each message is for.
pub const CORPUS: u32 = 1;
pub const PLAIN: u32 = 2;
pub const BASE64: u32 = 3;
pub const QUOTED: u32 = 4;
/// Two windows and a half.
pub const LONG: u32 = 5;
pub const DEEP: u32 = 6;
/// Eight windows and a rest.
pub const WIDE: u32 = 7;
/// Exactly one window.
pub const EXACT: u32 = 8;

/// The bytes one window asks for.
pub fn window() -> usize {
    usize::try_from(BODY_WINDOW_BYTES).unwrap()
}

/// The content behind the base64 part, past three windows once encoded.
pub fn base64_content() -> Vec<u8> {
    varied(window() * 5 / 2)
}

/// The content behind the quoted-printable part and the part as a
/// sender writes it, a soft break inside every line, past one window.
pub fn quoted() -> (String, String) {
    let (mut content, mut encoded) = (String::new(), String::new());
    let mut line = 0;
    while encoded.len() < window() * 3 / 2 {
        let _ = write!(content, "Caf\u{e9} number {line} continues here\r\n");
        let _ = write!(encoded, "Caf=C3=A9 number {line}=\r\n continues here\r\n");
        line += 1;
    }
    (content, encoded)
}

/// A message of one text part under an encoding, its structure stating
/// the length of the body.
fn one_part(uid: u32, encoding: &str, body: String) -> Message {
    let structure = format!(
        "(\"TEXT\" \"PLAIN\" (\"CHARSET\" \"utf-8\") NIL NIL \"{}\" {} 1)",
        encoding.to_ascii_uppercase(),
        body.len()
    );
    Message {
        structure,
        body,
        ..Message::new(uid)
    }
}

/// The messages of the inbox, by the UIDs above.
pub fn dataset() -> Vec<Message> {
    let header = Message::new(EXACT).header.len();
    vec![
        corpus(CORPUS),
        Message::new(PLAIN),
        one_part(BASE64, "base64", base64_lines(&base64_content())),
        one_part(QUOTED, "quoted-printable", quoted().1),
        one_part(LONG, "7bit", "long ".repeat(window() / 2)),
        Message::new(DEEP).nested(MAX_NESTING + 8),
        one_part(WIDE, "7bit", "wide ".repeat(window() * 8 / 5 + 7)),
        one_part(EXACT, "7bit", "x".repeat(window() - header)),
    ]
}

/// The message of the dataset under that UID.
pub fn message(uid: u32) -> Message {
    dataset()
        .into_iter()
        .find(|message| message.uid == uid)
        .unwrap()
}

/// A synced rig, the link the blobs take their turns on and the email
/// id of every UID.
pub struct Blobs {
    pub rig: Rig,
    pub link: Arc<Link<TestConnector>>,
    pub ids: HashMap<u32, String>,
}

/// The rig over the dataset.
pub async fn started() -> Blobs {
    started_over(inbox(dataset())).await
}

/// The same over the given server.
pub async fn started_over(mailboxes: Mailboxes) -> Blobs {
    let rig = Rig::start(mailboxes).await;
    rig.sync("INBOX").await;
    let get = json!([
        "Email/get",
        { "accountId": ACCOUNT, "ids": rig.created(0), "properties": ["size"] },
        "c1"
    ]);
    let listed = rig.call(&get).await;
    let ids = listed["list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|email| {
            let uid = email["size"].as_u64().unwrap() - u64::from(SIZE_ABOVE_UID);
            (
                u32::try_from(uid).unwrap(),
                email["id"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    let link = linked(&rig, CONVERSATION_DEADLINE);
    Blobs { rig, link, ids }
}

/// A link to the server of the rig for the blobs to take their turns
/// on, each within `deadline`: a conversation apart from the one the
/// requests of the rig take.
pub fn linked(rig: &Rig, deadline: Duration) -> Arc<Link<TestConnector>> {
    let target = rig.fake.target(HOST, TlsMode::Implicit);
    let connector = TestConnector::scripted(rig.fake.trusting(), target, STEP);
    let timing = Timing {
        interval: Duration::ZERO,
        deadline,
        ..Timing::default()
    };
    Arc::new(Link::with_timing(connector, timing))
}

/// Every byte of a blob; the error that ended it otherwise.
pub async fn drained(mut blob: Blob) -> Result<Vec<u8>, BlobError> {
    let mut bytes = Vec::new();
    while let Some(chunk) = blob.next().await {
        bytes.extend(chunk?);
    }
    Ok(bytes)
}

/// Every structure and window fetch the server received, without its
/// tag; the header fetches of the sync are not among them.
pub fn fetches(rig: &Rig) -> Vec<String> {
    rig.fake
        .lines()
        .iter()
        .filter(|line| !line.contains("INTERNALDATE"))
        .filter_map(|line| line.split_once("UID FETCH "))
        .map(|(_, rest)| rest.to_owned())
        .collect()
}

/// Waits until the server received `count` fetches; whether it then
/// received no more for a while.
pub async fn settled(rig: &Rig, count: usize) -> bool {
    for _ in 0..LOOKS {
        if fetches(rig).len() >= count {
            sleep(LOOK * STILL_LOOKS).await;
            return fetches(rig).len() == count;
        }
        sleep(LOOK).await;
    }
    false
}
