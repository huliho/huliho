// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Every checked-in transcript is clean: no address, subject, message id
//! or provider id of a live session remains, no body text either.

use std::path::Path;

use huliho_imap_bridge::testing::Transcript;
use huliho_imap_bridge::testing::record::scan::scan;

/// Where the recorded transcripts live.
const TRANSCRIPTS: &str = "tests/transcripts";

#[test]
fn every_checked_in_transcript_passes_the_scan() {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join(TRANSCRIPTS);
    let mut scanned = 0;
    for entry in std::fs::read_dir(&directory).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let transcript = Transcript::from_json(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let findings = scan(&transcript);
        assert_eq!(findings, Vec::<String>::new(), "{}", path.display());
        assert!(!transcript.sessions.is_empty(), "{}", path.display());
        scanned += 1;
    }
    assert!(scanned > 0, "no transcript under {TRANSCRIPTS}");
}
