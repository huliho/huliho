// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The credential check against the compose Dovecot.

#![cfg(feature = "live-targets")]

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use huliho_imap_bridge::session::{STEP_TIMEOUT, SessionError, Target, TlsMode};
use huliho_imap_bridge::smtp;
use huliho_imap_bridge::verify::{Credential, VerifyError, verify};
use tokio_rustls::rustls::pki_types::CertificateDer;
use tokio_rustls::rustls::pki_types::pem::PemObject;
use tokio_rustls::rustls::{ClientConfig, RootCertStore};

/// The compose Dovecot on the host's loopback; its certificate names
/// `localhost`.
const DOVECOT_HOST: &str = "localhost";
const IMAPS_PORT: u16 = 31993;
const IMAP_PORT: u16 = 31143;
const SUBMISSION_PORT: u16 = 31587;
const USER: &str = "sanne@huliho.test";
const PASSWORD: &str = "password";

fn dev_ca() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/dev-certs/ca.pem")
}

fn tls(trust_dev_ca: bool) -> Arc<ClientConfig> {
    let mut roots = RootCertStore::empty();
    if trust_dev_ca {
        for certificate in CertificateDer::pem_file_iter(dev_ca()).unwrap() {
            roots.add(certificate.unwrap()).unwrap();
        }
    }
    let provider = Arc::new(tokio_rustls::rustls::crypto::ring::default_provider());
    Arc::new(
        ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    )
}

fn dovecot(port: u16, tls: TlsMode) -> Target {
    Target {
        host: DOVECOT_HOST.to_owned(),
        addresses: vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)],
        tls,
    }
}

fn password(password: &str) -> Credential {
    Credential::Password {
        username: USER.to_owned(),
        password: password.to_owned(),
    }
}

#[tokio::test]
async fn dovecot_accepts_the_password_over_implicit_tls() {
    let target = dovecot(IMAPS_PORT, TlsMode::Implicit);
    let capabilities = verify(tls(true), &target, &password(PASSWORD), STEP_TIMEOUT)
        .await
        .unwrap();
    assert!(capabilities.has("IMAP4rev2"));
    assert!(capabilities.has("IDLE"));
}

#[tokio::test]
async fn dovecot_upgrades_with_starttls_rfc9051_6_2_1() {
    let target = dovecot(IMAP_PORT, TlsMode::Starttls);
    let capabilities = verify(tls(true), &target, &password(PASSWORD), STEP_TIMEOUT)
        .await
        .unwrap();
    assert!(capabilities.has("IMAP4rev2"));
    assert!(!capabilities.has("STARTTLS"));
}

#[tokio::test]
async fn dovecot_rejects_a_wrong_password() {
    let target = dovecot(IMAPS_PORT, TlsMode::Implicit);
    let error = verify(tls(true), &target, &password("wrong"), STEP_TIMEOUT)
        .await
        .unwrap_err();
    assert!(matches!(error, VerifyError::CredentialRejected), "{error}");
}

#[tokio::test]
async fn dovecot_is_insecure_without_the_dev_ca() {
    let target = dovecot(IMAPS_PORT, TlsMode::Implicit);
    let error = verify(tls(false), &target, &password(PASSWORD), STEP_TIMEOUT)
        .await
        .unwrap_err();
    assert!(
        matches!(error, VerifyError::Insecure(SessionError::Tls(_))),
        "{error}"
    );
}

#[tokio::test]
async fn dovecot_accepts_the_password_on_submission_over_starttls_rfc3207_4() {
    let target = dovecot(SUBMISSION_PORT, TlsMode::Starttls);
    smtp::verify(tls(true), &target, &password(PASSWORD), STEP_TIMEOUT)
        .await
        .unwrap();
}

#[tokio::test]
async fn dovecot_rejects_a_wrong_password_on_submission() {
    let target = dovecot(SUBMISSION_PORT, TlsMode::Starttls);
    let error = smtp::verify(tls(true), &target, &password("wrong"), STEP_TIMEOUT)
        .await
        .unwrap_err();
    assert!(matches!(error, VerifyError::CredentialRejected), "{error}");
}
