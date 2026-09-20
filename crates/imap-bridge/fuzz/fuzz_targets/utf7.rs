// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A mailbox name through the modified UTF-7 decoder.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    if let Ok(name) = std::str::from_utf8(bytes) {
        let _ = huliho_imap_bridge::utf7::decode(name);
    }
});
