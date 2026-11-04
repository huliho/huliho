// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A property over the whole way out: a create holding CRLF, a NUL, a line
//! past 998 octets, a header value with CRLF or a non-ASCII subject either
//! reaches the scripted server as a well-formed message with its values
//! whole or is refused before anything is sent (RFC 5322 section 2.1.1,
//! RFC 2047, RFC 3501 section 4.3).

use huliho_imap_bridge::testing::Extension;
use mail_parser::MessageParser;
use proptest::prelude::*;
use serde_json::{Value, json};

use crate::draft_rig::{DRAFTS, account, appends, create, drafts, held, synced};
use crate::set_rig::Set;

/// The cases of the property: each one is a create on a rig of its own
/// connection.
const CASES: u32 = 48;

/// The longest line of a message, its line end left out (RFC 5322 section
/// 2.1.1).
const MAX_LINE_BYTES: usize = 998;

/// What opens an encoded word (RFC 2047 section 2).
const ENCODED_WORD: &str = "=?";

fn hostile() -> impl Strategy<Value = String> {
    let pieces = prop_oneof![
        Just("\r\n".to_owned()),
        Just("\n".to_owned()),
        Just("\0".to_owned()),
        Just(".\r\n".to_owned()),
        Just(")\r\nA9 LOGOUT\r\n".to_owned()),
        Just("Bcc: evil@example.test".to_owned()),
        Just("=?utf-8?Q?x?=".to_owned()),
        Just("Caf\u{e9} \u{1f600} ".to_owned()),
        Just("x".repeat(1200)),
        "[ -~]{0,24}",
    ];
    prop::collection::vec(pieces, 0..6).prop_map(|pieces| pieces.concat())
}

/// The object with hostile values where a client may put text.
fn object(mailbox: &str, (subject, name, body): (&str, &str, &str)) -> Value {
    json!({
        "mailboxIds": { (mailbox): true },
        "keywords": { "$draft": true, "$seen": true },
        "from": [{ "email": "sanne@example.test" }],
        "to": [{ "name": name, "email": "mo@example.test" }],
        "subject": subject,
        "bodyStructure": { "partId": "t", "type": "text/plain" },
        "bodyValues": { "t": { "value": body } },
    })
}

/// No NUL, every line end a CRLF and no line past the limit.
fn well_formed(raw: &str) -> Result<(), TestCaseError> {
    prop_assert!(!raw.contains('\0'));
    let bytes = raw.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'\n' {
            prop_assert_eq!(bytes.get(index.wrapping_sub(1)), Some(&b'\r'));
        }
        if *byte == b'\r' {
            prop_assert_eq!(bytes.get(index + 1), Some(&b'\n'));
        }
    }
    for line in raw.split("\r\n") {
        prop_assert!(line.len() <= MAX_LINE_BYTES, "{}", line.len());
    }
    Ok(())
}

async fn one_case(set: &Set, values: (&str, &str, &str)) -> Result<(), TestCaseError> {
    let (subject, name, body) = values;
    let before = appends(set).len();
    let answer = create(set, object(&drafts(set), values)).await;
    let refused_header = subject.contains(['\r', '\n', '\0']) || name.contains(['\r', '\n', '\0']);
    if refused_header || body.contains('\0') {
        prop_assert_eq!(&answer["notCreated"]["d"]["type"], "invalidProperties");
        prop_assert_eq!(appends(set).len(), before, "nothing went out");
        return Ok(());
    }
    prop_assert!(answer["created"]["d"]["id"].is_string(), "{}", answer);
    let stored = held(set, DRAFTS);
    let message = stored.last().unwrap().raw();
    well_formed(&message)?;
    let parsed = MessageParser::default().parse(message.as_bytes()).unwrap();
    // Text that reads as an encoded word reads as what it encodes.
    if !subject.contains(ENCODED_WORD) {
        prop_assert_eq!(parsed.subject().unwrap_or_default().trim(), subject.trim());
    }
    let to = parsed.to().unwrap().first().unwrap();
    prop_assert_eq!(to.address.as_deref(), Some("mo@example.test"));
    let read = parsed
        .body_text(0)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    let wrote = body.replace("\r\n", "\n");
    prop_assert_eq!(read.trim_end(), wrote.trim_end());
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(CASES))]

    #[test]
    fn a_hostile_create_reaches_the_server_well_formed_or_is_refused(
        subject in hostile(),
        name in hostile(),
        body in hostile(),
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let set = synced(account(Extension::all())).await;
            one_case(&set, (&subject, &name, &body)).await
        })?;
    }
}
