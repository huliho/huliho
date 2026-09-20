// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The CAPABILITY read against a server that sends more than was asked:
//! lines ahead of the answer and names without end.

use std::fmt::Write as _;
use std::time::Duration;

use huliho_imap_bridge::session::{Capabilities, SessionError, TlsMode};
use huliho_imap_bridge::testing::imap::{FakeImap, HOST, Script};
use huliho_imap_bridge::testing::{PASSWORD, password};
use huliho_imap_bridge::verify::{VerifyError, verify};

/// Room for a loopback exchange.
const STEP: Duration = Duration::from_secs(1);

/// The untagged lines one CAPABILITY answer may carry, its own included.
const CAPABILITY_LINES: usize = 8;

/// The names one connection may advertise.
const MAX_CAPABILITIES: usize = 256;

async fn check(script: Script) -> Result<Capabilities, VerifyError> {
    let fake = FakeImap::start(script).await;
    let target = fake.target(HOST, TlsMode::Implicit);
    verify(fake.trusting(), &target, &password(PASSWORD), STEP).await
}

fn assert_unsupported(outcome: Result<Capabilities, VerifyError>, words: &str) {
    let error = outcome.unwrap_err();
    assert!(
        matches!(
            &error,
            VerifyError::Unsupported(SessionError::Protocol(found)) if *found == words
        ),
        "{error}"
    );
}

/// A server that volunteers this many lines ahead of every CAPABILITY
/// answer.
fn volunteering(lines: usize) -> Script {
    Script {
        ahead: "* 1 FETCH (UID 1)\r\n".repeat(lines),
        ..Script::tls()
    }
}

/// A server that advertises `IMAP4rev1`, `AUTH=PLAIN` and further names
/// up to `names` in all, numbered when `distinct` and all alike when not.
fn advertising(names: usize, distinct: bool) -> Script {
    let mut capabilities = "IMAP4rev1 AUTH=PLAIN".to_owned();
    for number in 2..names {
        let number = if distinct { number } else { 0 };
        write!(capabilities, " X{number}").unwrap();
    }
    Script {
        capabilities: capabilities.leak(),
        ..Script::tls()
    }
}

#[tokio::test]
async fn lines_nobody_asked_for_fit_the_answer_up_to_the_line_limit() {
    let inside = check(volunteering(CAPABILITY_LINES - 1)).await;
    assert!(inside.unwrap().has("IDLE"));
    let past = check(volunteering(CAPABILITY_LINES)).await;
    assert_unsupported(past, "the answer passes the line limit");
}

#[tokio::test]
async fn an_answer_without_a_capability_line_fails_the_check_rfc9051_6_1_1() {
    let silent = Script {
        capabilities: "",
        ..Script::tls()
    };
    assert_unsupported(check(silent).await, "CAPABILITY answered without data");
}

#[tokio::test]
async fn names_up_to_the_limit_are_kept_and_one_more_fails_the_check() {
    let inside = check(advertising(MAX_CAPABILITIES, true)).await.unwrap();
    assert_eq!(inside.iter().count(), MAX_CAPABILITIES);
    for distinct in [true, false] {
        let past = check(advertising(MAX_CAPABILITIES + 1, distinct)).await;
        assert_unsupported(past, "the server advertises too many capabilities");
    }
}
