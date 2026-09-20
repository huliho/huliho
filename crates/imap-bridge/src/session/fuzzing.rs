// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What the fuzz targets call: the guard's lexer and the readers of
//! server lines, which nothing outside the session layer reaches
//! otherwise.

use super::{guard, read};

/// The lexer on the bytes whole and on the two chunks a cut makes; the
/// first byte picks the cut.
///
/// # Panics
///
/// When a chunk border changes the verdict.
pub fn lexer(bytes: &[u8]) {
    let cut = bytes.first().map_or(0, |byte| usize::from(*byte));
    assert_eq!(guard::admits(bytes, 0), guard::admits(bytes, cut));
}

/// One server line through the protocol parser into the LIST, STATUS
/// and FETCH readers. As on a session, the parser sees only what the
/// guard admits.
pub fn line(bytes: &[u8]) {
    if guard::admits(bytes, 0) {
        read::line(bytes);
    }
}
