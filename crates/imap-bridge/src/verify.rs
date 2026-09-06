// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The credential check at add time: connect, sign in, read the
//! capabilities, log out.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use thiserror::Error;
use tokio_rustls::rustls::ClientConfig;

use crate::session::{Capabilities, ImapSession, Session, SessionError, Target};

/// What an account signs in with.
#[derive(Clone)]
pub enum Credential {
    /// The LOGIN command.
    Password { username: String, password: String },
    /// AUTHENTICATE with an OAuth access token.
    Xoauth2 { username: String, token: String },
}

impl Credential {
    /// The one word a log line may carry.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Password { .. } => "password",
            Self::Xoauth2 { .. } => "xoauth2",
        }
    }
}

/// Prints the kind only, so a credential in a log line stays one word.
impl fmt::Debug for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Credential({})", self.kind())
    }
}

/// Why the check failed, sorted by who can act on it.
#[derive(Debug, Error)]
pub enum VerifyError {
    #[error("the server refused the credential")]
    CredentialRejected,
    #[error("the server cannot be reached: {0}")]
    Unreachable(#[source] SessionError),
    #[error("no secure connection: {0}")]
    Insecure(#[source] SessionError),
    #[error("the server is not usable: {0}")]
    Unsupported(#[source] SessionError),
}

impl From<SessionError> for VerifyError {
    fn from(error: SessionError) -> Self {
        match error {
            SessionError::CredentialRejected => Self::CredentialRejected,
            SessionError::NoAddress
            | SessionError::Connect(_)
            | SessionError::ServerName(_)
            | SessionError::Timeout
            | SessionError::Closed
            | SessionError::Io(_) => Self::Unreachable(error),
            SessionError::Tls(_) | SessionError::StarttlsAbsent | SessionError::StarttlsRefused => {
                Self::Insecure(error)
            }
            SessionError::Protocol(_) => Self::Unsupported(error),
        }
    }
}

/// Connects, signs in, reads CAPABILITY and logs out. Nothing in the
/// answer or the error carries the credential.
///
/// # Errors
///
/// Returns the cause the user or the operator can act on.
pub async fn verify(
    tls: Arc<ClientConfig>,
    target: &Target,
    credential: &Credential,
    step_timeout: Duration,
) -> Result<Capabilities, VerifyError> {
    let mut session = ImapSession::connect(tls, target, step_timeout).await?;
    let signed_in = match credential {
        Credential::Password { username, password } => session.login(username, password).await,
        Credential::Xoauth2 { username, token } => {
            session.authenticate_xoauth2(username, token).await
        }
    };
    let outcome = match signed_in {
        Ok(()) => session.capabilities().await,
        Err(error) => Err(error),
    };
    // The outcome above is the answer; a LOGOUT that fails changes nothing.
    let _ended = session.logout().await;
    outcome.map_err(VerifyError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_names_the_kind_only() {
        let password = Credential::Password {
            username: "sanne".to_owned(),
            password: "hunter2 but longer".to_owned(),
        };
        let token = Credential::Xoauth2 {
            username: "sanne".to_owned(),
            token: "ya29.secret".to_owned(),
        };
        assert_eq!(format!("{password:?}"), "Credential(password)");
        assert_eq!(format!("{token:?}"), "Credential(xoauth2)");
    }

    #[test]
    fn every_session_error_has_one_verify_cause() {
        let closed: VerifyError = SessionError::Closed.into();
        assert!(matches!(
            closed,
            VerifyError::Unreachable(SessionError::Closed)
        ));
        let absent: VerifyError = SessionError::StarttlsAbsent.into();
        assert!(matches!(
            absent,
            VerifyError::Insecure(SessionError::StarttlsAbsent)
        ));
        let odd: VerifyError = SessionError::Protocol("x").into();
        assert!(matches!(
            odd,
            VerifyError::Unsupported(SessionError::Protocol(_))
        ));
        let rejected: VerifyError = SessionError::CredentialRejected.into();
        assert!(matches!(rejected, VerifyError::CredentialRejected));
    }
}
