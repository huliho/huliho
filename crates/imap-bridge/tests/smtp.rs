// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The SMTP submission check against a scripted server: every refusal,
//! then the ways in.

use std::time::Duration;

use huliho_imap_bridge::session::{SessionError, Target, TlsMode};
use huliho_imap_bridge::smtp::verify;
use huliho_imap_bridge::testing::smtp::{FakeSmtp, HOST, Script};
use huliho_imap_bridge::testing::{Greeting, PASSWORD, Starttls, TOKEN, password, token};
use huliho_imap_bridge::verify::{Credential, VerifyError};
use tokio::net::TcpListener;

/// Room for a loopback exchange; the hang tests wait this long once.
const STEP: Duration = Duration::from_secs(1);

async fn check(fake: &FakeSmtp, tls: TlsMode, credential: &Credential) -> Result<(), VerifyError> {
    verify(fake.trusting(), &fake.target(HOST, tls), credential, STEP).await
}

/// The error a server running `script` answers the right password with
/// over TLS from the first byte.
async fn refusal(script: Script) -> (FakeSmtp, VerifyError) {
    let fake = FakeSmtp::start(script).await;
    let error = check(&fake, TlsMode::Implicit, &password(PASSWORD))
        .await
        .unwrap_err();
    (fake, error)
}

fn assert_no_secret(error: &VerifyError, secret: &str) {
    let text = format!("{error} {error:?}");
    assert!(!text.contains(secret), "{text}");
}

/// The EHLO line carries the host's own name, so only its start is fixed.
fn assert_ehlo(line: &str, phase: &str) {
    assert!(line.starts_with(&format!("{phase} EHLO ")), "{line}");
}

#[tokio::test]
async fn a_rejected_password_is_credential_rejected_and_the_error_carries_no_secret() {
    let fake = FakeSmtp::start(Script::tls()).await;
    let error = check(&fake, TlsMode::Implicit, &password("wrong horse"))
        .await
        .unwrap_err();
    assert!(matches!(error, VerifyError::CredentialRejected), "{error}");
    assert_no_secret(&error, "wrong horse");
    let lines = fake.lines();
    assert_eq!(lines.len(), 4, "{lines:?}");
    assert_ehlo(&lines[0], "tls");
    assert!(lines[1].starts_with("tls AUTH PLAIN "), "{lines:?}");
    assert_eq!(lines[2], "tls SASL \0sanne\0wrong horse");
    assert_eq!(lines[3], "tls QUIT");
}

#[tokio::test]
async fn a_mailbox_with_smtp_auth_off_refuses_the_right_password() {
    let (_fake, error) = refusal(Script {
        accepts: false,
        ..Script::tls()
    })
    .await;
    assert!(matches!(error, VerifyError::CredentialRejected), "{error}");
    assert_no_secret(&error, PASSWORD);
}

#[tokio::test]
async fn a_rejected_token_ends_in_the_error_challenge_and_is_credential_rejected() {
    let fake = FakeSmtp::start(Script::tls()).await;
    let error = check(&fake, TlsMode::Implicit, &token("stale"))
        .await
        .unwrap_err();
    assert!(matches!(error, VerifyError::CredentialRejected), "{error}");
    assert_no_secret(&error, "stale");
    let lines = fake.lines();
    let identity = "tls SASL user=sanne\u{1}auth=Bearer stale\u{1}\u{1}";
    assert_eq!(lines.len(), 5, "{lines:?}");
    assert!(lines[1].starts_with("tls AUTH XOAUTH2 "), "{lines:?}");
    assert_eq!(lines[2], identity);
    assert_eq!(lines[3], identity);
    assert_eq!(lines[4], "tls QUIT");
}

#[tokio::test]
async fn ehlo_without_auth_is_auth_unavailable_and_no_credential_travels_rfc4954_3() {
    let (fake, error) = refusal(Script {
        mechanisms: "",
        ..Script::tls()
    })
    .await;
    assert!(matches!(error, VerifyError::AuthUnavailable), "{error}");
    let lines = fake.lines();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_ehlo(&lines[0], "tls");
    assert_eq!(lines[1], "tls QUIT");
}

#[tokio::test]
async fn a_server_offering_only_a_mechanism_this_cannot_answer_is_auth_unavailable() {
    let (fake, error) = refusal(Script {
        mechanisms: "CRAM-MD5",
        ..Script::tls()
    })
    .await;
    assert!(matches!(error, VerifyError::AuthUnavailable), "{error}");
    assert_eq!(fake.lines().len(), 2);
    let error = check(&fake, TlsMode::Implicit, &token(TOKEN))
        .await
        .unwrap_err();
    assert!(matches!(error, VerifyError::AuthUnavailable), "{error}");
}

// The refusals below mirror the IMAP suite: one list of causes, two protocols.
// jscpd:ignore-start
#[tokio::test]
async fn a_closed_port_is_unreachable() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let target = Target {
        host: HOST.to_owned(),
        addresses: vec![address],
        tls: TlsMode::Implicit,
    };
    let error = verify(FakeSmtp::distrusting(), &target, &password(PASSWORD), STEP)
        .await
        .unwrap_err();
    assert!(
        matches!(error, VerifyError::Unreachable(SessionError::Connect(_))),
        "{error}"
    );
}

#[tokio::test]
async fn an_empty_address_list_is_unreachable() {
    let target = Target {
        host: HOST.to_owned(),
        addresses: Vec::new(),
        tls: TlsMode::Implicit,
    };
    let error = verify(FakeSmtp::distrusting(), &target, &password(PASSWORD), STEP)
        .await
        .unwrap_err();
    assert!(
        matches!(error, VerifyError::Unreachable(SessionError::NoAddress)),
        "{error}"
    );
}

#[tokio::test]
async fn a_server_that_never_greets_is_unreachable() {
    let (_fake, error) = refusal(Script {
        greeting: Greeting::Silence,
        ..Script::tls()
    })
    .await;
    assert!(
        matches!(error, VerifyError::Unreachable(SessionError::Timeout)),
        "{error}"
    );
}

#[tokio::test]
async fn a_server_that_stops_answering_is_unreachable() {
    let (_fake, error) = refusal(Script {
        answers: false,
        ..Script::tls()
    })
    .await;
    assert!(
        matches!(error, VerifyError::Unreachable(SessionError::Timeout)),
        "{error}"
    );
}

#[tokio::test]
async fn a_closing_greeting_is_unreachable_rfc5321_3_1() {
    let (fake, error) = refusal(Script {
        greeting: Greeting::Bye,
        ..Script::tls()
    })
    .await;
    assert!(
        matches!(error, VerifyError::Unreachable(SessionError::Closed)),
        "{error}"
    );
    assert!(fake.lines().is_empty());
}

#[tokio::test]
async fn a_server_that_does_not_speak_smtp_is_unsupported() {
    let (fake, error) = refusal(Script {
        greeting: Greeting::Garbage,
        ..Script::tls()
    })
    .await;
    assert!(
        matches!(error, VerifyError::Unsupported(SessionError::Protocol(_))),
        "{error}"
    );
    assert!(fake.lines().is_empty());
}
// jscpd:ignore-end

#[tokio::test]
async fn a_certificate_outside_the_roots_is_insecure() {
    let fake = FakeSmtp::start(Script::tls()).await;
    let target = fake.target(HOST, TlsMode::Implicit);
    let error = verify(FakeSmtp::distrusting(), &target, &password(PASSWORD), STEP)
        .await
        .unwrap_err();
    assert!(
        matches!(error, VerifyError::Insecure(SessionError::Tls(_))),
        "{error}"
    );
    assert!(fake.lines().is_empty());
}

#[tokio::test]
async fn a_name_the_certificate_does_not_carry_is_insecure() {
    let fake = FakeSmtp::start(Script::tls()).await;
    let target = fake.target("other.test", TlsMode::Implicit);
    let error = verify(fake.trusting(), &target, &password(PASSWORD), STEP)
        .await
        .unwrap_err();
    assert!(
        matches!(error, VerifyError::Insecure(SessionError::Tls(_))),
        "{error}"
    );
    assert!(fake.lines().is_empty());
}

#[tokio::test]
async fn starttls_not_advertised_is_insecure_and_no_credential_travels_rfc3207_4() {
    let fake = FakeSmtp::start(Script::plain(Starttls::Absent)).await;
    let error = check(&fake, TlsMode::Starttls, &password(PASSWORD))
        .await
        .unwrap_err();
    assert!(
        matches!(error, VerifyError::Insecure(SessionError::StarttlsAbsent)),
        "{error}"
    );
    let lines = fake.lines();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_ehlo(&lines[0], "plain");
}

#[tokio::test]
async fn starttls_refused_is_insecure() {
    let fake = FakeSmtp::start(Script::plain(Starttls::Refused)).await;
    let error = check(&fake, TlsMode::Starttls, &password(PASSWORD))
        .await
        .unwrap_err();
    assert!(
        matches!(error, VerifyError::Insecure(SessionError::StarttlsRefused)),
        "{error}"
    );
    let lines = fake.lines();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[1], "plain STARTTLS");
}

#[tokio::test]
async fn an_implicit_target_never_speaks_plaintext() {
    let (fake, error) = refusal(Script::plain(Starttls::Offered)).await;
    assert!(
        matches!(error, VerifyError::Insecure(SessionError::Tls(_))),
        "{error}"
    );
    assert!(fake.lines().is_empty());
}

#[tokio::test]
async fn starttls_upgrades_before_the_credential_travels_rfc3207_4() {
    let fake = FakeSmtp::start(Script::plain(Starttls::Offered)).await;
    check(&fake, TlsMode::Starttls, &password(PASSWORD))
        .await
        .unwrap();
    let lines = fake.lines();
    assert_eq!(lines.len(), 6, "{lines:?}");
    assert_ehlo(&lines[0], "plain");
    assert_eq!(lines[1], "plain STARTTLS");
    assert_ehlo(&lines[2], "tls");
    assert!(lines[3].starts_with("tls AUTH PLAIN "), "{lines:?}");
    assert_eq!(lines[4], format!("tls SASL \0sanne\0{PASSWORD}"));
    assert_eq!(lines[5], "tls QUIT");
}

#[tokio::test]
async fn a_password_signs_in_with_plain_where_it_is_offered() {
    let fake = FakeSmtp::start(Script::tls()).await;
    check(&fake, TlsMode::Implicit, &password(PASSWORD))
        .await
        .unwrap();
    let lines = fake.lines();
    assert_eq!(lines.len(), 4, "{lines:?}");
    assert!(lines[1].starts_with("tls AUTH PLAIN "), "{lines:?}");
    assert_eq!(lines[2], format!("tls SASL \0sanne\0{PASSWORD}"));
    assert_eq!(lines[3], "tls QUIT");
}

#[tokio::test]
async fn a_password_signs_in_with_login_where_it_is_the_only_mechanism() {
    let fake = FakeSmtp::start(Script {
        mechanisms: "LOGIN",
        ..Script::tls()
    })
    .await;
    check(&fake, TlsMode::Implicit, &password(PASSWORD))
        .await
        .unwrap();
    let lines = fake.lines();
    assert_eq!(lines.len(), 5, "{lines:?}");
    assert_eq!(lines[1], "tls AUTH LOGIN");
    assert_eq!(lines[2], "tls SASL sanne");
    assert_eq!(lines[3], format!("tls SASL {PASSWORD}"));
    assert_eq!(lines[4], "tls QUIT");
}

#[tokio::test]
async fn a_token_signs_in_through_xoauth2() {
    let fake = FakeSmtp::start(Script::tls()).await;
    check(&fake, TlsMode::Implicit, &token(TOKEN))
        .await
        .unwrap();
    let lines = fake.lines();
    assert_eq!(lines.len(), 4, "{lines:?}");
    assert!(lines[1].starts_with("tls AUTH XOAUTH2 "), "{lines:?}");
    assert_eq!(
        lines[2],
        format!("tls SASL user=sanne\u{1}auth=Bearer {TOKEN}\u{1}\u{1}")
    );
    assert_eq!(lines[3], "tls QUIT");
}
