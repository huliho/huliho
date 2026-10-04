// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What the fuzz target calls: the reader of a blob id and the decoders
//! the windows of a blob pass through.

use super::address::Address;
use super::decode::Decoder;

/// The bytes as a blob id, then as the windows of a part under every
/// encoding. The first byte sizes the windows, so an edge falls at
/// every distance from an escape, a quartet and a line break.
///
/// # Panics
///
/// Panics when a decoder gives a window up.
pub fn blob_bytes(bytes: &[u8]) {
    let _ = Address::parse(&String::from_utf8_lossy(bytes));
    let size = usize::from(bytes.first().copied().unwrap_or_default()) + 1;
    for encoding in ["base64", "quoted-printable", "7bit"] {
        let mut decoder = Decoder::of(encoding);
        let mut windows = bytes.chunks(size).peekable();
        while let Some(window) = windows.next() {
            let content = decoder.push(window.to_vec(), windows.peek().is_none());
            assert!(content.is_some(), "{encoding} gave a window up");
        }
    }
}
