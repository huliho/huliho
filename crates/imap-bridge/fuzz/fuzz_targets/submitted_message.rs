// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The message of a submission as fetched back, through the recipient
//! reader and the header rewrite behind the check of the send path; a
//! wire form that check lets through holds no Bcc field and names the
//! draft's fields as the recipient reader's parser reads them.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    assert!(
        huliho_imap_bridge::jmap::fuzzing::submitted_message(bytes),
        "a wire form read otherwise than the draft"
    );
});
