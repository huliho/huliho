// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The credential check against scripted IMAP and SMTP servers: every
//! refusal in its own word and the SMTP check only after the IMAP one
//! passed.

mod fake_dns;

use std::io::Write;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use fake_dns::FakeDns;
use huliho_imap_bridge::testing::imap::{self, FakeImap};
use huliho_imap_bridge::testing::smtp::{self, FakeSmtp};
use huliho_imap_bridge::testing::{Greeting, PASSWORD, Starttls, TOKEN, USER};
use huliho_server::accounts::{AccountSettings, Credential, Endpoint, TlsMode};
use huliho_server::config::UpstreamConfig;
use huliho_server::discovery::Address;
use huliho_server::probe::{Probe, ProbeError};
use huliho_server::upstream::Upstream;
use tempfile::NamedTempFile;
use tokio::net::TcpListener;

const ADDRESS: &str = "sanne@example.test";

/// A name the IMAP fake's certificate does not carry.
const OTHER_HOST: &str = "other.example.test";

/// Room for a loopback exchange; the hang tests wait this long once.
const STEP: Duration = Duration::from_secs(1);

/// Two steps, so a silent server trips the step before the attempt.
const ATTEMPT: Duration = Duration::from_secs(2);

/// The two fakes, resolved by name to the loopback.
struct Servers {
    imap: FakeImap,
    smtp: FakeSmtp,
}

impl Servers {
    async fn start(imap: imap::Script, smtp: smtp::Script) -> Self {
        Self {
            imap: FakeImap::start(imap).await,
            smtp: FakeSmtp::start(smtp).await,
        }
    }

    /// A probe on short budgets that trusts the given CAs; the loopback
    /// is reachable when asked. The resolver answers port 0, so the
    /// target's port applies.
    fn probe(&self, trust: &[&str], allow_loopback: bool) -> Probe {
        let mut ca_file = NamedTempFile::new().unwrap();
        for pem in trust {
            ca_file.write_all(pem.as_bytes()).unwrap();
        }
        let allow_private_networks = if allow_loopback {
            vec!["127.0.0.0/8".parse().unwrap()]
        } else {
            Vec::new()
        };
        let config = UpstreamConfig {
            allow_private_networks,
            additional_ca_file: Some(ca_file.path().to_owned()),
            ..UpstreamConfig::default()
        };
        let mut dns = FakeDns::default();
        dns.addresses.insert(
            imap::HOST.to_owned(),
            vec![SocketAddr::new(self.imap.address.ip(), 0)],
        );
        dns.addresses.insert(
            smtp::HOST.to_owned(),
            vec![SocketAddr::new(self.smtp.address.ip(), 0)],
        );
        dns.addresses.insert(
            OTHER_HOST.to_owned(),
            vec![SocketAddr::new(self.imap.address.ip(), 0)],
        );
        Probe {
            upstream: Arc::new(Upstream::with_dns(&config, Arc::new(dns)).unwrap()),
            step_timeout: STEP,
            attempt_timeout: ATTEMPT,
        }
    }

    fn trusting_both(&self) -> Probe {
        self.probe(&[self.imap.ca_pem(), self.smtp.ca_pem()], true)
    }

    fn target(&self, imap: TlsMode, smtp: TlsMode) -> AccountSettings {
        AccountSettings::Imap {
            username: USER.to_owned(),
            imap: Endpoint {
                host: imap::HOST.to_owned(),
                port: self.imap.address.port(),
                tls: imap,
            },
            smtp: Endpoint {
                host: smtp::HOST.to_owned(),
                port: self.smtp.address.port(),
                tls: smtp,
            },
        }
    }
}

fn password(password: &str) -> Credential {
    Credential::Password {
        password: password.to_owned(),
    }
}

async fn check(
    probe: &Probe,
    target: &AccountSettings,
    credential: &Credential,
) -> Result<(), ProbeError> {
    probe
        .check(&Address::parse(ADDRESS).unwrap(), target, credential)
        .await
}

fn saw(lines: &[String], word: &str) -> bool {
    lines.iter().any(|line| line.contains(word))
}

#[tokio::test]
async fn a_rejected_imap_password_stops_before_smtp_and_names_no_secret() {
    let servers = Servers::start(imap::Script::tls(), smtp::Script::tls()).await;
    let probe = servers.trusting_both();
    let target = servers.target(TlsMode::Implicit, TlsMode::Implicit);
    let error = check(&probe, &target, &password("wrong horse"))
        .await
        .unwrap_err();
    assert!(matches!(error, ProbeError::CredentialRejected), "{error}");
    assert!(!format!("{error} {error:?}").contains("wrong horse"));
    assert!(saw(&servers.imap.lines(), "LOGIN"));
    assert!(servers.smtp.lines().is_empty());
}

#[tokio::test]
async fn a_closed_imap_port_is_unreachable() {
    let servers = Servers::start(imap::Script::tls(), smtp::Script::tls()).await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let closed = listener.local_addr().unwrap().port();
    drop(listener);
    let probe = servers.trusting_both();
    let AccountSettings::Imap {
        username,
        mut imap,
        smtp,
    } = servers.target(TlsMode::Implicit, TlsMode::Implicit)
    else {
        unreachable!()
    };
    imap.port = closed;
    let target = AccountSettings::Imap {
        username,
        imap,
        smtp,
    };
    let error = check(&probe, &target, &password(PASSWORD))
        .await
        .unwrap_err();
    assert!(matches!(error, ProbeError::Unreachable(_)), "{error}");
    assert!(servers.smtp.lines().is_empty());
}

#[tokio::test]
async fn a_silent_imap_server_is_unreachable_within_the_step() {
    let silent = imap::Script {
        greeting: Greeting::Silence,
        ..imap::Script::tls()
    };
    let servers = Servers::start(silent, smtp::Script::tls()).await;
    let probe = servers.trusting_both();
    let started = Instant::now();
    let error = check(
        &probe,
        &servers.target(TlsMode::Implicit, TlsMode::Implicit),
        &password(PASSWORD),
    )
    .await
    .unwrap_err();
    assert!(matches!(error, ProbeError::Unreachable(_)), "{error}");
    assert!(started.elapsed() < ATTEMPT);
}

#[tokio::test]
async fn a_slow_attempt_ends_within_its_own_budget() {
    let silent = imap::Script {
        greeting: Greeting::Silence,
        ..imap::Script::tls()
    };
    let servers = Servers::start(silent, smtp::Script::tls()).await;
    let probe = Probe {
        step_timeout: Duration::from_secs(5),
        attempt_timeout: STEP,
        ..servers.trusting_both()
    };
    let started = Instant::now();
    let error = check(
        &probe,
        &servers.target(TlsMode::Implicit, TlsMode::Implicit),
        &password(PASSWORD),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(&error, ProbeError::Unreachable(cause) if cause.contains("ran out of time")),
        "{error}"
    );
    assert!(started.elapsed() < ATTEMPT);
}

#[tokio::test]
async fn an_imap_certificate_outside_the_roots_is_insecure() {
    let servers = Servers::start(imap::Script::tls(), smtp::Script::tls()).await;
    let probe = servers.probe(&[servers.smtp.ca_pem()], true);
    let error = check(
        &probe,
        &servers.target(TlsMode::Implicit, TlsMode::Implicit),
        &password(PASSWORD),
    )
    .await
    .unwrap_err();
    assert!(matches!(error, ProbeError::Insecure(_)), "{error}");
    assert!(servers.imap.lines().is_empty());
    assert!(servers.smtp.lines().is_empty());
}

#[tokio::test]
async fn a_name_the_certificate_does_not_carry_is_insecure_and_names_no_host() {
    let servers = Servers::start(imap::Script::tls(), smtp::Script::tls()).await;
    let probe = servers.trusting_both();
    let AccountSettings::Imap {
        username,
        mut imap,
        smtp,
    } = servers.target(TlsMode::Implicit, TlsMode::Implicit)
    else {
        unreachable!()
    };
    imap.host = OTHER_HOST.to_owned();
    let target = AccountSettings::Imap {
        username,
        imap,
        smtp,
    };
    let error = check(&probe, &target, &password(PASSWORD))
        .await
        .unwrap_err();
    assert!(matches!(error, ProbeError::Insecure(_)), "{error}");
    assert!(!format!("{error} {error:?}").contains(OTHER_HOST));
    assert!(servers.imap.lines().is_empty());
}

#[tokio::test]
async fn starttls_absent_on_imap_is_insecure() {
    let servers = Servers::start(imap::Script::plain(Starttls::Absent), smtp::Script::tls()).await;
    let probe = servers.trusting_both();
    let error = check(
        &probe,
        &servers.target(TlsMode::Starttls, TlsMode::Implicit),
        &password(PASSWORD),
    )
    .await
    .unwrap_err();
    assert!(matches!(error, ProbeError::Insecure(_)), "{error}");
    assert!(servers.smtp.lines().is_empty());
}

#[tokio::test]
async fn a_private_address_is_refused_before_the_connect() {
    let servers = Servers::start(imap::Script::tls(), smtp::Script::tls()).await;
    let probe = servers.probe(&[servers.imap.ca_pem(), servers.smtp.ca_pem()], false);
    let error = check(
        &probe,
        &servers.target(TlsMode::Implicit, TlsMode::Implicit),
        &password(PASSWORD),
    )
    .await
    .unwrap_err();
    assert!(matches!(error, ProbeError::Unreachable(_)), "{error}");
    assert!(!format!("{error}").contains(imap::HOST));
    assert!(servers.imap.lines().is_empty());
}

#[tokio::test]
async fn a_submission_server_without_auth_is_smtp_auth_unavailable_after_imap_passed() {
    let no_auth = smtp::Script {
        mechanisms: "",
        ..smtp::Script::tls()
    };
    let servers = Servers::start(imap::Script::tls(), no_auth).await;
    let probe = servers.trusting_both();
    let error = check(
        &probe,
        &servers.target(TlsMode::Implicit, TlsMode::Implicit),
        &password(PASSWORD),
    )
    .await
    .unwrap_err();
    assert!(matches!(error, ProbeError::SmtpAuthUnavailable), "{error}");
    assert!(saw(&servers.imap.lines(), "LOGIN"));
    assert!(saw(&servers.smtp.lines(), "EHLO"));
    assert!(!saw(&servers.smtp.lines(), "AUTH"));
}

#[tokio::test]
async fn a_submission_server_refusing_the_accepted_password_is_smtp_auth_unavailable() {
    let refusing = smtp::Script {
        accepts: false,
        ..smtp::Script::tls()
    };
    let servers = Servers::start(imap::Script::tls(), refusing).await;
    let probe = servers.trusting_both();
    let error = check(
        &probe,
        &servers.target(TlsMode::Implicit, TlsMode::Implicit),
        &password(PASSWORD),
    )
    .await
    .unwrap_err();
    assert!(matches!(error, ProbeError::SmtpAuthUnavailable), "{error}");
    assert!(saw(&servers.smtp.lines(), "AUTH PLAIN"));
}

#[tokio::test]
async fn a_token_on_an_imap_target_is_unsupported_without_a_connection() {
    let servers = Servers::start(imap::Script::tls(), smtp::Script::tls()).await;
    let probe = servers.trusting_both();
    let bearer = Credential::Bearer {
        token: TOKEN.to_owned(),
    };
    let error = check(
        &probe,
        &servers.target(TlsMode::Implicit, TlsMode::Implicit),
        &bearer,
    )
    .await
    .unwrap_err();
    assert!(matches!(error, ProbeError::Unsupported(_)), "{error}");
    assert!(servers.imap.lines().is_empty());
    assert!(servers.smtp.lines().is_empty());
}

#[tokio::test]
async fn a_password_passes_imap_and_then_smtp() {
    let servers = Servers::start(imap::Script::tls(), smtp::Script::tls()).await;
    let probe = servers.trusting_both();
    check(
        &probe,
        &servers.target(TlsMode::Implicit, TlsMode::Implicit),
        &password(PASSWORD),
    )
    .await
    .unwrap();
    assert!(saw(&servers.imap.lines(), "LOGIN"));
    assert!(saw(&servers.smtp.lines(), "AUTH PLAIN"));
}

#[tokio::test]
async fn a_password_passes_with_starttls_on_both_servers() {
    let servers = Servers::start(
        imap::Script::plain(Starttls::Offered),
        smtp::Script::plain(Starttls::Offered),
    )
    .await;
    let probe = servers.trusting_both();
    check(
        &probe,
        &servers.target(TlsMode::Starttls, TlsMode::Starttls),
        &password(PASSWORD),
    )
    .await
    .unwrap();
    assert!(saw(&servers.imap.lines(), "plain A0002 STARTTLS"));
    assert!(saw(&servers.smtp.lines(), "plain STARTTLS"));
}
