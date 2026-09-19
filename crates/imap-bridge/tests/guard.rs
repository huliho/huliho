// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A server that sends a deeply nested line nobody asked for, on either
//! side of STARTTLS.

use std::time::Duration;

use huliho_imap_bridge::session::{Capabilities, MAX_NESTING, SessionError, TlsMode};
use huliho_imap_bridge::testing::imap::{FakeImap, HOST, Script};
use huliho_imap_bridge::testing::{PASSWORD, Starttls, password};
use huliho_imap_bridge::verify::{VerifyError, verify};

/// Room for a loopback exchange.
const STEP: Duration = Duration::from_secs(1);

/// Levels that end the process on a worker's stack where nothing stops
/// the line.
const HOSTILE_DEPTH: usize = 400;

/// The credential check against a server that sends a line this deep
/// ahead of every CAPABILITY answer.
async fn check(
    script: Script,
    depth: usize,
    tls: TlsMode,
) -> (FakeImap, Result<Capabilities, VerifyError>) {
    let fake = FakeImap::start(Script {
        nested: Some(depth),
        ..script
    })
    .await;
    let target = fake.target(HOST, tls);
    let outcome = verify(fake.trusting(), &target, &password(PASSWORD), STEP).await;
    (fake, outcome)
}

fn assert_nests_too_deep(outcome: Result<Capabilities, VerifyError>) {
    let error = outcome.unwrap_err();
    assert!(
        matches!(
            error,
            VerifyError::Unsupported(SessionError::Protocol("the answer nests too deep"))
        ),
        "{error}"
    );
}

#[tokio::test]
async fn a_nested_line_over_tls_fails_the_check_before_any_credential_is_sent() {
    let (fake, outcome) = check(Script::tls(), HOSTILE_DEPTH, TlsMode::Implicit).await;
    assert_nests_too_deep(outcome);
    let lines = fake.lines();
    assert!(
        lines.iter().all(|line| !line.contains("LOGIN")),
        "{lines:?}"
    );
}

#[tokio::test]
async fn a_nested_line_before_starttls_fails_the_same_way() {
    let script = Script::plain(Starttls::Offered);
    let (fake, outcome) = check(script, HOSTILE_DEPTH, TlsMode::Starttls).await;
    assert_nests_too_deep(outcome);
    let lines = fake.lines();
    assert!(
        lines.iter().all(|line| line.starts_with("plain ")),
        "{lines:?}"
    );
}

#[tokio::test]
async fn a_line_at_the_bound_leaves_the_check_alone() {
    let (_fake, outcome) = check(Script::tls(), MAX_NESTING, TlsMode::Implicit).await;
    assert!(outcome.unwrap().has("IMAP4rev1"));
}
