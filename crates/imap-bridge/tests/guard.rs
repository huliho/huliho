// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A server that passes a bound of the guard ahead of its CAPABILITY
//! answer, on either side of STARTTLS: a deeply nested line nobody asked
//! for, a CAPABILITY line filled to a size and a response code that
//! carries literals.

use std::time::Duration;

use huliho_imap_bridge::session::{
    Capabilities, MAX_NESTING, MAX_STRUCTURED_BYTES, SessionError, TlsMode,
};
use huliho_imap_bridge::testing::imap::{FakeImap, HOST, Script, nested_fetch};
use huliho_imap_bridge::testing::{PASSWORD, Starttls, password};
use huliho_imap_bridge::verify::{VerifyError, verify};

/// Room for a loopback exchange.
const STEP: Duration = Duration::from_secs(1);

/// Levels that end the process on a worker's stack where nothing stops
/// the line.
const HOSTILE_DEPTH: usize = 400;

/// The credential check against a scripted server.
async fn check(script: Script, tls: TlsMode) -> (FakeImap, Result<Capabilities, VerifyError>) {
    let fake = FakeImap::start(script).await;
    let target = fake.target(HOST, tls);
    let outcome = verify(fake.trusting(), &target, &password(PASSWORD), STEP).await;
    (fake, outcome)
}

/// A status line whose response code holds literals, which the parser
/// reads as one response across every line break (RFC 3501 section 7.1).
const LITERALS_IN_A_CODE: &str = "* OK [BADCHARSET ({1}\r\na {1}\r\nb)] x\r\n";

/// `script` in the three ways it passes a bound, each with the words the
/// failure carries.
fn hostile(script: Script) -> [(Script, &'static str); 3] {
    let nested = Script {
        ahead: nested_fetch(HOSTILE_DEPTH),
        ..script.clone()
    };
    let literals = Script {
        ahead: LITERALS_IN_A_CODE.to_owned(),
        ..script.clone()
    };
    let filled = Script {
        capability_bytes: Some(MAX_STRUCTURED_BYTES + 1),
        ..script
    };
    [
        (nested, "the answer nests too deep"),
        (literals, "the answer holds a literal in free text"),
        (filled, "the answer passes the structure limit"),
    ]
}

fn assert_trips(outcome: Result<Capabilities, VerifyError>, words: &str) {
    let error = outcome.unwrap_err();
    assert!(
        matches!(
            &error,
            VerifyError::Unsupported(SessionError::Protocol(found)) if *found == words
        ),
        "{error}"
    );
}

#[tokio::test]
async fn a_bound_passed_over_tls_fails_the_check_before_any_credential_is_sent() {
    for (script, words) in hostile(Script::tls()) {
        let (fake, outcome) = check(script, TlsMode::Implicit).await;
        assert_trips(outcome, words);
        let lines = fake.lines();
        assert!(
            lines.iter().all(|line| !line.contains("LOGIN")),
            "{lines:?}"
        );
    }
}

#[tokio::test]
async fn a_bound_passed_before_starttls_fails_the_same_way() {
    for (script, words) in hostile(Script::plain(Starttls::Offered)) {
        let (fake, outcome) = check(script, TlsMode::Starttls).await;
        assert_trips(outcome, words);
        let lines = fake.lines();
        assert!(
            lines.iter().all(|line| line.starts_with("plain ")),
            "{lines:?}"
        );
    }
}

#[tokio::test]
async fn lines_at_the_bounds_leave_the_check_alone_on_both_legs() {
    let script = Script {
        ahead: nested_fetch(MAX_NESTING),
        capability_bytes: Some(MAX_STRUCTURED_BYTES),
        ..Script::plain(Starttls::Offered)
    };
    let (_fake, outcome) = check(script, TlsMode::Starttls).await;
    assert!(outcome.unwrap().has("IMAP4rev1"));
}
