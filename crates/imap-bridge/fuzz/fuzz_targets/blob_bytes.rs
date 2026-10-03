// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A blob id and the windows of a part, through the id reader and the transfer decoders.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    huliho_imap_bridge::blob::fuzzing::blob_bytes(bytes);
});
