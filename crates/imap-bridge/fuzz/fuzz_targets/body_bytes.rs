// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The bytes a body fetch brings back, through the header forms and the value decoder.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    huliho_imap_bridge::jmap::fuzzing::body_bytes(bytes);
});
