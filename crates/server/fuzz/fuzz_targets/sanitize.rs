// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Sender HTML through the sanitizer: it never panics and its output
//! settles within one more pass, which may only reorder the attributes
//! of an element the first pass dropped one from.

#![no_main]

use std::sync::LazyLock;

use huliho_server::mail::sanitize::Sanitizer;
use libfuzzer_sys::fuzz_target;

static SANITIZER: LazyLock<Sanitizer> = LazyLock::new(|| Sanitizer::new(None));

fuzz_target!(|html: &str| {
    let settled = SANITIZER.clean(&SANITIZER.clean(html));
    assert_eq!(SANITIZER.clean(&settled), settled);
});
