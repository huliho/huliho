// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The credential check at add time: the JMAP session resource for JMAP
//! accounts, the IMAP sign-in and then the SMTP one through the bridge
//! for the rest. Nothing is stored before it passes.

mod jmap;

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use huliho_imap_bridge::session::{STEP_TIMEOUT, SessionError, Target, TlsMode as BridgeTls};
use huliho_imap_bridge::verify::{Credential as BridgeCredential, VerifyError};
use huliho_imap_bridge::{smtp, verify};
use thiserror::Error;

use crate::accounts::{AccountSettings, Credential, Endpoint, TlsMode};
use crate::discovery::Address;
use crate::upstream::{ATTEMPT_TIMEOUT, Upstream, UpstreamError};

/// Why the check failed, sorted by who can act on it. The text carries
/// no host, address or credential.
#[derive(Debug, Error)]
pub enum ProbeError {
    #[error("the server refused the credential")]
    CredentialRejected,
    #[error("the server cannot be reached: {0}")]
    Unreachable(String),
    #[error("no secure connection: {0}")]
    Insecure(String),
    #[error("the server is not usable: {0}")]
    Unsupported(String),
    #[error("the submission server offers no way to sign in")]
    SmtpAuthUnavailable,
}

/// The check with its time limits; the handler runs it on the defaults.
pub struct Probe {
    /// The connector every check resolves and connects through.
    pub upstream: Arc<Upstream>,
    /// One step inside a connection.
    pub step_timeout: Duration,
    /// One connection, resolve and steps included.
    pub attempt_timeout: Duration,
}

impl Probe {
    /// On the twenty-second budgets.
    #[must_use]
    pub fn new(upstream: Arc<Upstream>) -> Self {
        Self {
            upstream,
            step_timeout: STEP_TIMEOUT,
            attempt_timeout: ATTEMPT_TIMEOUT,
        }
    }

    /// Checks `credential` against `target` the way the account will use
    /// it: the session resource for JMAP, IMAP and then SMTP for the
    /// rest.
    ///
    /// # Errors
    ///
    /// Returns the cause the user or the operator can act on.
    pub async fn check(
        &self,
        address: &Address,
        target: &AccountSettings,
        credential: &Credential,
    ) -> Result<(), ProbeError> {
        let outcome = match (target, credential) {
            (AccountSettings::Jmap { session_url }, _) => {
                jmap::check(&self.upstream, session_url, address, credential).await
            }
            (
                AccountSettings::Imap {
                    username,
                    imap: incoming,
                    smtp: outgoing,
                },
                Credential::Password { password },
            ) => {
                let credential = BridgeCredential::Password {
                    username: username.clone(),
                    password: password.clone(),
                };
                self.imap_then_smtp(incoming, outgoing, &credential).await
            }
            (AccountSettings::Imap { .. }, Credential::Bearer { .. }) => Err(
                ProbeError::Unsupported("a token signs in over JMAP only".to_owned()),
            ),
        };
        if let Err(error) = &outcome {
            tracing::debug!(kind = target.kind().as_str(), %error, "credential check failed");
        }
        outcome
    }

    /// The IMAP sign-in first; the SMTP one only after it passed, so a
    /// wrong password costs one attempt.
    async fn imap_then_smtp(
        &self,
        incoming: &Endpoint,
        outgoing: &Endpoint,
        credential: &BridgeCredential,
    ) -> Result<(), ProbeError> {
        let tls = self.upstream.tls();
        self.attempt(async {
            let target = self.target_of(incoming).await?;
            verify::verify(Arc::clone(&tls), &target, credential, self.step_timeout)
                .await
                .map(|_capabilities| ())
                .map_err(ProbeError::from)
        })
        .await?;
        self.attempt(async {
            let target = self.target_of(outgoing).await?;
            smtp::verify(tls, &target, credential, self.step_timeout)
                .await
                .map_err(smtp_error)
        })
        .await
    }

    /// One connection within its budget; a slow server is unreachable.
    async fn attempt(
        &self,
        check: impl Future<Output = Result<(), ProbeError>>,
    ) -> Result<(), ProbeError> {
        tokio::time::timeout(self.attempt_timeout, check)
            .await
            .unwrap_or_else(|_elapsed| {
                Err(ProbeError::Unreachable(
                    "the attempt ran out of time".to_owned(),
                ))
            })
    }

    /// The endpoint with its addresses resolved and checked, as the
    /// bridge takes it.
    async fn target_of(&self, endpoint: &Endpoint) -> Result<Target, ProbeError> {
        let addresses = self
            .upstream
            .resolve(&endpoint.host, endpoint.port)
            .await
            .map_err(|error| unreachable(&error))?;
        Ok(Target {
            host: endpoint.host.clone(),
            addresses,
            tls: match endpoint.tls {
                TlsMode::Implicit => BridgeTls::Implicit,
                TlsMode::Starttls => BridgeTls::Starttls,
            },
        })
    }
}

impl From<VerifyError> for ProbeError {
    fn from(error: VerifyError) -> Self {
        match error {
            VerifyError::CredentialRejected => Self::CredentialRejected,
            VerifyError::Unreachable(cause) => Self::Unreachable(cause.to_string()),
            // The TLS library names the host it expected in its own text.
            VerifyError::Insecure(SessionError::Tls(_)) => {
                Self::Insecure("the certificate was refused".to_owned())
            }
            VerifyError::Insecure(cause) => Self::Insecure(cause.to_string()),
            VerifyError::Unsupported(cause) => Self::Unsupported(cause.to_string()),
            VerifyError::AuthUnavailable => Self::SmtpAuthUnavailable,
        }
    }
}

/// On the submission server a credential the IMAP server accepted is
/// refused only when SMTP AUTH is off for the mailbox.
fn smtp_error(error: VerifyError) -> ProbeError {
    match error {
        VerifyError::CredentialRejected => ProbeError::SmtpAuthUnavailable,
        other => ProbeError::from(other),
    }
}

/// A resolve failure in fixed words; the error's own text names the
/// host.
fn unreachable(error: &UpstreamError) -> ProbeError {
    let cause = match error {
        UpstreamError::PrivateNetwork { .. } => "inside a network this instance does not reach",
        _ => "the host does not resolve",
    };
    ProbeError::Unreachable(cause.to_owned())
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::net::{IpAddr, Ipv4Addr};

    use super::*;

    #[test]
    fn every_verify_cause_has_one_probe_word() {
        let rejected: ProbeError = VerifyError::CredentialRejected.into();
        assert!(matches!(rejected, ProbeError::CredentialRejected));
        let closed: ProbeError = VerifyError::Unreachable(SessionError::Closed).into();
        assert!(matches!(closed, ProbeError::Unreachable(_)));
        let absent: ProbeError = VerifyError::Insecure(SessionError::StarttlsAbsent).into();
        assert!(matches!(absent, ProbeError::Insecure(_)));
        let odd: ProbeError = VerifyError::Unsupported(SessionError::Protocol("x")).into();
        assert!(matches!(odd, ProbeError::Unsupported(_)));
        let unavailable: ProbeError = VerifyError::AuthUnavailable.into();
        assert!(matches!(unavailable, ProbeError::SmtpAuthUnavailable));
    }

    #[test]
    fn a_refused_certificate_names_no_host() {
        let text = "certificate not valid for name \"secret.example.test\"";
        let refused = io::Error::new(io::ErrorKind::InvalidData, text);
        let mapped: ProbeError = VerifyError::Insecure(SessionError::Tls(refused)).into();
        assert!(matches!(mapped, ProbeError::Insecure(_)));
        assert!(!format!("{mapped} {mapped:?}").contains("secret.example.test"));
    }

    #[test]
    fn a_refused_smtp_credential_means_smtp_auth_is_off() {
        assert!(matches!(
            smtp_error(VerifyError::CredentialRejected),
            ProbeError::SmtpAuthUnavailable
        ));
        assert!(matches!(
            smtp_error(VerifyError::Unreachable(SessionError::Timeout)),
            ProbeError::Unreachable(_)
        ));
    }

    #[test]
    fn a_resolve_failure_names_no_host() {
        let refused = UpstreamError::PrivateNetwork {
            host: "secret.example.test".to_owned(),
            address: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
        };
        let mapped = unreachable(&refused);
        let text = format!("{mapped} {mapped:?}");
        assert!(matches!(mapped, ProbeError::Unreachable(_)));
        assert!(!text.contains("secret.example.test"));
        assert!(!text.contains("10.0.0.1"));
    }

    #[test]
    fn the_defaults_are_the_twenty_second_budgets() {
        let upstream = Upstream::new(&crate::config::UpstreamConfig::default()).unwrap();
        let probe = Probe::new(Arc::new(upstream));
        assert_eq!(probe.step_timeout, STEP_TIMEOUT);
        assert_eq!(probe.attempt_timeout, ATTEMPT_TIMEOUT);
    }
}
