// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A download template an upstream wrote through the expansion: it
//! never panics, and with every variable in a slot of its own no value
//! reaches past its slot.

#![no_main]

use huliho_server::jmap::{BlobAsk, expand};
use libfuzzer_sys::fuzz_target;

/// One literal between the four variables, so every value sits between
/// separators an encoded value never carries.
const SLOTTED: &str = "{accountId}/{blobId}/{name}?{type}";

/// The unreserved set of RFC 3986 section 2.3 plus the escape.
fn unreserved_or_escaped(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'%')
}

fuzz_target!(|input: (&str, [&str; 4])| {
    let (template, [account_id, blob_id, name, media_type]) = input;
    let ask = BlobAsk {
        account_id,
        blob_id,
        name,
        media_type,
    };
    let _ = expand(template, &ask);
    let expanded = expand(SLOTTED, &ask).expect("the slotted template names every variable");
    let separators: String = expanded.matches(['/', '?']).collect();
    assert_eq!(separators, "//?", "{expanded}");
    assert!(
        expanded
            .split(['/', '?'])
            .all(|slot| slot.bytes().all(unreserved_or_escaped)),
        "{expanded}"
    );
});
