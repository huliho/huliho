// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Where the bridge's connections come from: the row read instance
//! wide, the token refreshed under the account's lock, the host resolved
//! through the pinned resolver and the sign-in as the credential check
//! does it. Every IMAP outcome is reported to the gate; a send's SMTP
//! connection reports nothing, since the gate watches the IMAP host.

use std::sync::Arc;

use huliho_imap_bridge::runtime::{ConnectError, Connector};
use huliho_imap_bridge::session::{
    ImapSession, STEP_TIMEOUT, Session, SessionError, Target, TlsMode as BridgeTls,
};
use huliho_imap_bridge::smtp::{self, Submission};
use huliho_imap_bridge::store::AccountKey;
use huliho_imap_bridge::verify::Credential as BridgeCredential;
use tokio::time::timeout;

use crate::accounts::{self, AccountSettings, Credential, Endpoint, TlsMode};
use crate::events::Actor;
use crate::gate::{AttemptError, Fault, Reconnect};
use crate::ids::AccountId;
use crate::probe;
use crate::scope::Scope;
use crate::store::StoreError;
use crate::upstream::ATTEMPT_TIMEOUT;

/// The connector of this server.
pub struct HostConnector {
    wiring: Reconnect,
}

/// The row's side of a sign-in.
struct Stored {
    scope: Scope,
    username: String,
    endpoint: Endpoint,
    /// The SMTP endpoint a send signs in on.
    submission: Endpoint,
    credential: Credential,
}

/// Why a sign-in failed: the error the bridge gets in fixed words and
/// the rule the gate applies.
struct Failure {
    error: SessionError,
    fault: Fault,
}

impl HostConnector {
    #[must_use]
    pub fn new(wiring: Reconnect) -> Self {
        Self { wiring }
    }

    /// The row as the sign-in needs it. `None` holds the account back:
    /// the row left, is stopped or is no IMAP account. A store failure
    /// holds it back as well, which the next connect sees again.
    async fn read(&self, account_id: &AccountId) -> Option<Stored> {
        let store = Arc::clone(self.wiring.gate.store());
        let (keys, id) = (Arc::clone(&self.wiring.keys), account_id.clone());
        let read = tokio::task::spawn_blocking(move || -> Result<Option<Stored>, StoreError> {
            let Some(scope) = accounts::owner_scope(&store, &id)? else {
                return Ok(None);
            };
            if accounts::get(&store, &scope)?.stopped_cause.is_some() {
                return Ok(None);
            }
            let AccountSettings::Imap {
                username,
                imap,
                smtp,
            } = accounts::settings(&store, &scope)?
            else {
                return Ok(None);
            };
            let credential = accounts::credential(&store, &keys, &scope)?;
            Ok(Some(Stored {
                scope,
                username,
                endpoint: imap,
                submission: smtp,
                credential,
            }))
        })
        .await;
        match read {
            Ok(Ok(stored)) => stored,
            Ok(Err(error)) => {
                tracing::warn!(account = account_id.as_str(), %error, "the row could not be read");
                None
            }
            Err(error) => {
                tracing::warn!(account = account_id.as_str(), %error, "the store task failed");
                None
            }
        }
    }

    /// A live token in the bridge's form.
    async fn credential(&self, stored: &Stored) -> Result<BridgeCredential, Failure> {
        let credential = self
            .wiring
            .live_credential(&stored.scope, stored.credential.clone())
            .await
            .map_err(|error| Failure::attempt(&error))?;
        probe::bridge_credential(&stored.username, &credential)
            .map_err(|error| Failure::attempt(&error.into()))
    }

    /// The endpoint's pinned addresses with the name its certificate
    /// carries.
    async fn target(&self, endpoint: &Endpoint) -> Result<Target, Failure> {
        let addresses = self
            .wiring
            .upstream
            .resolve(&endpoint.host, endpoint.port)
            .await
            .map_err(|_| Failure {
                error: SessionError::NoAddress,
                fault: Fault::Connection,
            })?;
        Ok(Target {
            host: endpoint.host.clone(),
            addresses,
            tls: match endpoint.tls {
                TlsMode::Implicit => BridgeTls::Implicit,
                TlsMode::Starttls => BridgeTls::Starttls,
            },
        })
    }

    /// One sign-in: a live token, the pinned addresses, the connection
    /// and LOGIN or XOAUTH2 as the credential asks.
    async fn sign_in(&self, stored: &Stored) -> Result<ImapSession, Failure> {
        let credential = self.credential(stored).await?;
        let target = self.target(&stored.endpoint).await?;
        let mut session = ImapSession::connect(self.wiring.upstream.tls(), &target, STEP_TIMEOUT)
            .await
            .map_err(Failure::session)?;
        match credential {
            BridgeCredential::Password { username, password } => {
                session.login(&username, &password).await
            }
            BridgeCredential::Xoauth2 { username, token } => {
                session.authenticate_xoauth2(&username, &token).await
            }
        }
        .map_err(Failure::session)?;
        Ok(session)
    }

    /// The SMTP sign-in of one send, as the credential check signs in.
    async fn submit_in(&self, stored: &Stored) -> Result<Submission, SessionError> {
        let credential = self.credential(stored).await.map_err(|it| it.error)?;
        let target = self
            .target(&stored.submission)
            .await
            .map_err(|it| it.error)?;
        let tls = self.wiring.upstream.tls();
        smtp::submission(tls, &target, &credential, STEP_TIMEOUT).await
    }
}

impl Connector for HostConnector {
    type Session = ImapSession;

    async fn connect(&self, key: &AccountKey) -> Result<ImapSession, ConnectError> {
        let account_id = AccountId::from(key.as_str().to_owned());
        let Some(stored) = self.read(&account_id).await else {
            return Err(ConnectError::Held);
        };
        let _held = self.wiring.gate.hold(&account_id).await;
        let outcome = match timeout(ATTEMPT_TIMEOUT, self.sign_in(&stored)).await {
            Ok(outcome) => outcome,
            Err(_elapsed) => Err(Failure {
                error: SessionError::Timeout,
                fault: Fault::Connection,
            }),
        };
        let fault = outcome.as_ref().err().map(|failure| failure.fault);
        let observed = self
            .wiring
            .gate
            .observe(&stored.scope, &Actor::System, fault)
            .await;
        if let Err(error) = observed {
            tracing::warn!(account = key.as_str(), %error, "the gate could not record the sign-in");
        }
        outcome.map_err(|failure| ConnectError::Failed(failure.error))
    }

    async fn submission(&self, key: &AccountKey) -> Result<Submission, ConnectError> {
        let account_id = AccountId::from(key.as_str().to_owned());
        let Some(stored) = self.read(&account_id).await else {
            return Err(ConnectError::Held);
        };
        let _held = self.wiring.gate.hold(&account_id).await;
        match timeout(ATTEMPT_TIMEOUT, self.submit_in(&stored)).await {
            Ok(outcome) => Ok(outcome?),
            Err(_elapsed) => Err(SessionError::Timeout.into()),
        }
    }
}

impl Failure {
    fn session(error: SessionError) -> Self {
        Self {
            fault: fault_of(&error),
            error,
        }
    }

    /// A failure before the connection, a refused refresh for one: the
    /// gate's rule from the attempt, the bridge told in the nearest
    /// words.
    fn attempt(error: &AttemptError) -> Self {
        let fault = error.fault();
        Self {
            error: match fault {
                Fault::Credential => SessionError::CredentialRejected,
                Fault::Connection => SessionError::Unavailable,
                Fault::Undecided => SessionError::AuthUnavailable,
            },
            fault,
        }
    }
}

/// The rule a session failure falls under, as the credential check
/// sorts it: the server's verdict on the credential, a connection that
/// did not come about or one nothing here can tell apart.
fn fault_of(error: &SessionError) -> Fault {
    match error {
        SessionError::CredentialRejected => Fault::Credential,
        SessionError::NoAddress
        | SessionError::Connect(_)
        | SessionError::ServerName(_)
        | SessionError::Timeout
        | SessionError::Closed
        | SessionError::Io(_)
        | SessionError::Unavailable
        | SessionError::Tls(_)
        | SessionError::StarttlsAbsent
        | SessionError::StarttlsRefused => Fault::Connection,
        SessionError::AuthUnavailable
        | SessionError::Protocol(_)
        | SessionError::Refused
        | SessionError::BadCharset => Fault::Undecided,
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use huliho_imap_bridge::verify::VerifyError;
    use rustls::pki_types::ServerName;

    use super::*;
    use crate::probe::ProbeError;

    fn refused_name() -> SessionError {
        let error = ServerName::try_from("not a name!".to_owned()).unwrap_err();
        SessionError::ServerName(error)
    }

    /// Every failure a session can report, one of each.
    fn every_failure() -> Vec<SessionError> {
        vec![
            SessionError::NoAddress,
            SessionError::Connect(io::Error::from(io::ErrorKind::ConnectionRefused)),
            refused_name(),
            SessionError::Tls(io::Error::from(io::ErrorKind::InvalidData)),
            SessionError::StarttlsAbsent,
            SessionError::StarttlsRefused,
            SessionError::Timeout,
            SessionError::Closed,
            SessionError::CredentialRejected,
            SessionError::AuthUnavailable,
            SessionError::Unavailable,
            SessionError::Refused,
            SessionError::BadCharset,
            SessionError::Protocol("odd"),
            SessionError::Io(io::Error::from(io::ErrorKind::BrokenPipe)),
        ]
    }

    /// The rule the credential check applies to the same failure.
    fn as_the_check_sorts_it(error: SessionError) -> Fault {
        AttemptError::from(ProbeError::from(VerifyError::from(error))).fault()
    }

    #[test]
    fn every_failure_falls_under_the_rule_the_credential_check_applies() {
        for error in every_failure() {
            let fault = fault_of(&error);
            let words = error.to_string();
            assert_eq!(fault, as_the_check_sorts_it(error), "{words}");
        }
    }

    #[test]
    fn a_failure_before_the_connection_keeps_its_rule_and_names_no_server() {
        let refused = Failure::attempt(&ProbeError::CredentialRejected.into());
        assert_eq!(refused.fault, Fault::Credential);
        assert!(matches!(refused.error, SessionError::CredentialRejected));
        let down =
            Failure::attempt(&ProbeError::Unreachable("secret.example.test".to_owned()).into());
        assert_eq!(down.fault, Fault::Connection);
        assert!(!down.error.to_string().contains("secret.example.test"));
        let odd = Failure::attempt(&ProbeError::Unsupported("odd".to_owned()).into());
        assert_eq!(odd.fault, Fault::Undecided);
    }
}
