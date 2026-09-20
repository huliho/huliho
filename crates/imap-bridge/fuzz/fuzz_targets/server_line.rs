// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! One server line through the parser into the LIST, STATUS and FETCH readers.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    huliho_imap_bridge::session::fuzzing::line(bytes);
});
