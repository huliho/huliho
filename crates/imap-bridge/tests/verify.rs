// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The credential check against a scripted IMAP server: every refusal,
//! then the two ways in.

mod fake_imap;

use std::time::Duration;

use fake_imap::{CAPABILITIES, FakeImap, Greeting, HOST, PASSWORD, Script, Starttls, TOKEN, USER};
use huliho_imap_bridge::session::{Capabilities, SessionError, Target, TlsMode};
use huliho_imap_bridge::verify::{Credential, VerifyError, verify};
use tokio::net::TcpListener;

/// Room for a loopback exchange; the hang tests wait this long once.
const STEP: Duration = Duration::from_secs(1);

fn password(password: &str) -> Credential {
    Credential::Password {
        username: USER.to_owned(),
        password: password.to_owned(),
    }
}

fn token(token: &str) -> Credential {
    Credential::Xoauth2 {
        username: USER.to_owned(),
        token: token.to_owned(),
    }
}

async fn check(
    fake: &FakeImap,
    tls: TlsMode,
    credential: &Credential,
) -> Result<Capabilities, VerifyError> {
    verify(fake.trusting(), &fake.target(HOST, tls), credential, STEP).await
}

fn assert_no_secret(error: &VerifyError, secret: &str) {
    let text = format!("{error} {error:?}");
    assert!(!text.contains(secret), "{text}");
}

#[tokio::test]
async fn a_rejected_password_is_credential_rejected_and_the_error_carries_no_secret() {
    let fake = FakeImap::start(Script::tls()).await;
    let error = check(&fake, TlsMode::Implicit, &password("wrong horse"))
        .await
        .unwrap_err();
    assert!(matches!(error, VerifyError::CredentialRejected), "{error}");
    assert_no_secret(&error, "wrong horse");
    let lines = fake.lines();
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(lines[1].starts_with("tls A0002 LOGIN "), "{lines:?}");
    assert_eq!(lines[2], "tls A0003 LOGOUT");
}

#[tokio::test]
async fn a_rejected_token_ends_in_the_empty_answer_and_is_credential_rejected() {
    let fake = FakeImap::start(Script::tls()).await;
    let error = check(&fake, TlsMode::Implicit, &token("stale"))
        .await
        .unwrap_err();
    assert!(matches!(error, VerifyError::CredentialRejected), "{error}");
    assert_no_secret(&error, "stale");
    assert_eq!(
        fake.lines(),
        [
            "tls A0001 CAPABILITY",
            "tls A0002 AUTHENTICATE XOAUTH2",
            "tls SASL user=sanne\u{1}auth=Bearer stale\u{1}\u{1}",
            "tls SASL \"\"",
            "tls A0003 LOGOUT",
        ]
    );
}

#[tokio::test]
async fn login_disabled_over_tls_is_unsupported_and_no_login_travels_rfc9051_7_2_2() {
    let fake = FakeImap::start(Script {
        capabilities: "IMAP4rev1 LOGINDISABLED AUTH=PLAIN",
        ..Script::tls()
    })
    .await;
    let error = check(&fake, TlsMode::Implicit, &password(PASSWORD))
        .await
        .unwrap_err();
    assert!(
        matches!(error, VerifyError::Unsupported(SessionError::Protocol(_))),
        "{error}"
    );
    assert_eq!(fake.lines(), ["tls A0001 CAPABILITY", "tls A0002 LOGOUT"]);
}

#[tokio::test]
async fn a_server_without_xoauth2_is_unsupported_and_no_token_travels() {
    let fake = FakeImap::start(Script {
        capabilities: "IMAP4rev1 AUTH=PLAIN",
        ..Script::tls()
    })
    .await;
    let error = check(&fake, TlsMode::Implicit, &token(TOKEN))
        .await
        .unwrap_err();
    assert!(
        matches!(error, VerifyError::Unsupported(SessionError::Protocol(_))),
        "{error}"
    );
    assert_eq!(fake.lines(), ["tls A0001 CAPABILITY", "tls A0002 LOGOUT"]);
}

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
    let error = verify(FakeImap::distrusting(), &target, &password(PASSWORD), STEP)
        .await
        .unwrap_err();
    assert!(
        matches!(error, VerifyError::Unreachable(SessionError::Connect(_))),
        "{error}"
    );
}

#[tokio::test]
async fn a_connection_dropped_during_the_handshake_is_unreachable() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            drop(tcp);
        }
    });
    let target = Target {
        host: HOST.to_owned(),
        addresses: vec![address],
        tls: TlsMode::Implicit,
    };
    let error = verify(FakeImap::distrusting(), &target, &password(PASSWORD), STEP)
        .await
        .unwrap_err();
    assert!(matches!(error, VerifyError::Unreachable(_)), "{error}");
}

#[tokio::test]
async fn an_empty_address_list_is_unreachable() {
    let target = Target {
        host: HOST.to_owned(),
        addresses: Vec::new(),
        tls: TlsMode::Implicit,
    };
    let error = verify(FakeImap::distrusting(), &target, &password(PASSWORD), STEP)
        .await
        .unwrap_err();
    assert!(
        matches!(error, VerifyError::Unreachable(SessionError::NoAddress)),
        "{error}"
    );
}

#[tokio::test]
async fn a_server_that_never_greets_is_unreachable() {
    let fake = FakeImap::start(Script {
        greeting: Greeting::Silence,
        ..Script::tls()
    })
    .await;
    let error = check(&fake, TlsMode::Implicit, &password(PASSWORD))
        .await
        .unwrap_err();
    assert!(
        matches!(error, VerifyError::Unreachable(SessionError::Timeout)),
        "{error}"
    );
}

#[tokio::test]
async fn a_server_that_stops_answering_is_unreachable() {
    let fake = FakeImap::start(Script {
        answers: false,
        ..Script::tls()
    })
    .await;
    let error = check(&fake, TlsMode::Implicit, &password(PASSWORD))
        .await
        .unwrap_err();
    assert!(
        matches!(error, VerifyError::Unreachable(SessionError::Timeout)),
        "{error}"
    );
}

#[tokio::test]
async fn a_bye_greeting_is_unreachable() {
    let fake = FakeImap::start(Script {
        greeting: Greeting::Bye,
        ..Script::tls()
    })
    .await;
    let error = check(&fake, TlsMode::Implicit, &password(PASSWORD))
        .await
        .unwrap_err();
    assert!(
        matches!(error, VerifyError::Unreachable(SessionError::Closed)),
        "{error}"
    );
    assert!(fake.lines().is_empty());
}

#[tokio::test]
async fn a_server_that_does_not_speak_imap_is_unsupported() {
    let fake = FakeImap::start(Script {
        greeting: Greeting::Garbage,
        ..Script::tls()
    })
    .await;
    let error = check(&fake, TlsMode::Implicit, &password(PASSWORD))
        .await
        .unwrap_err();
    assert!(
        matches!(error, VerifyError::Unsupported(SessionError::Protocol(_))),
        "{error}"
    );
    assert!(fake.lines().is_empty());
}

#[tokio::test]
async fn a_certificate_outside_the_roots_is_insecure() {
    let fake = FakeImap::start(Script::tls()).await;
    let target = fake.target(HOST, TlsMode::Implicit);
    let error = verify(FakeImap::distrusting(), &target, &password(PASSWORD), STEP)
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
    let fake = FakeImap::start(Script::tls()).await;
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
async fn starttls_not_advertised_is_insecure_and_no_credential_travels_rfc9051_6_2_1() {
    let fake = FakeImap::start(Script::plain(Starttls::Absent)).await;
    let error = check(&fake, TlsMode::Starttls, &password(PASSWORD))
        .await
        .unwrap_err();
    assert!(
        matches!(error, VerifyError::Insecure(SessionError::StarttlsAbsent)),
        "{error}"
    );
    assert_eq!(fake.lines(), ["plain A0001 CAPABILITY"]);
}

#[tokio::test]
async fn starttls_refused_is_insecure() {
    let fake = FakeImap::start(Script::plain(Starttls::Refused)).await;
    let error = check(&fake, TlsMode::Starttls, &password(PASSWORD))
        .await
        .unwrap_err();
    assert!(
        matches!(error, VerifyError::Insecure(SessionError::StarttlsRefused)),
        "{error}"
    );
    assert_eq!(
        fake.lines(),
        ["plain A0001 CAPABILITY", "plain A0002 STARTTLS"]
    );
}

#[tokio::test]
async fn an_implicit_target_never_speaks_plaintext() {
    let fake = FakeImap::start(Script::plain(Starttls::Offered)).await;
    let error = check(&fake, TlsMode::Implicit, &password(PASSWORD))
        .await
        .unwrap_err();
    assert!(
        matches!(error, VerifyError::Insecure(SessionError::Tls(_))),
        "{error}"
    );
    assert!(fake.lines().is_empty());
}

#[tokio::test]
async fn starttls_upgrades_before_the_credential_travels_rfc9051_6_2_1() {
    let fake = FakeImap::start(Script::plain(Starttls::Offered)).await;
    let capabilities = check(&fake, TlsMode::Starttls, &password(PASSWORD))
        .await
        .unwrap();
    assert!(capabilities.has("IMAP4rev2"));
    assert_eq!(
        fake.lines(),
        [
            "plain A0001 CAPABILITY".to_owned(),
            "plain A0002 STARTTLS".to_owned(),
            "tls A0001 CAPABILITY".to_owned(),
            format!("tls A0002 LOGIN \"{USER}\" \"{PASSWORD}\""),
            "tls A0003 CAPABILITY".to_owned(),
            "tls A0004 LOGOUT".to_owned(),
        ]
    );
}

#[tokio::test]
async fn a_password_signs_in_reads_capability_and_logs_out() {
    let fake = FakeImap::start(Script::tls()).await;
    let capabilities = check(&fake, TlsMode::Implicit, &password(PASSWORD))
        .await
        .unwrap();
    let expected: Capabilities = CAPABILITIES.split(' ').map(str::to_owned).collect();
    assert_eq!(capabilities, expected);
    assert!(capabilities.has("auth=xoauth2"));
    assert_eq!(
        fake.lines(),
        [
            "tls A0001 CAPABILITY".to_owned(),
            format!("tls A0002 LOGIN \"{USER}\" \"{PASSWORD}\""),
            "tls A0003 CAPABILITY".to_owned(),
            "tls A0004 LOGOUT".to_owned(),
        ]
    );
}

#[tokio::test]
async fn a_token_signs_in_through_xoauth2() {
    let fake = FakeImap::start(Script::tls()).await;
    let capabilities = check(&fake, TlsMode::Implicit, &token(TOKEN))
        .await
        .unwrap();
    assert!(capabilities.has("IDLE"));
    assert_eq!(
        fake.lines(),
        [
            "tls A0001 CAPABILITY".to_owned(),
            "tls A0002 AUTHENTICATE XOAUTH2".to_owned(),
            format!("tls SASL user={USER}\u{1}auth=Bearer {TOKEN}\u{1}\u{1}"),
            "tls A0003 CAPABILITY".to_owned(),
            "tls A0004 LOGOUT".to_owned(),
        ]
    );
}
