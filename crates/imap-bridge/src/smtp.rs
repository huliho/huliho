// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The SMTP submission check at add time: connect, sign in, quit. The
//! credential the IMAP check accepted must work here too, so a mailbox
//! that refuses SMTP AUTH is caught before anything is stored.

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use mail_send::{Credentials, Error as ClientError, SmtpClient};
use smtp_proto::{AUTH_LOGIN, AUTH_PLAIN, AUTH_XOAUTH2, EXT_START_TLS, EhloResponse};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_rustls::TlsConnector;
use tokio_rustls::rustls::ClientConfig;
use tokio_rustls::rustls::pki_types::InvalidDnsNameError;

use crate::session::{SessionError, Target, TlsMode, io_error};
use crate::verify::{Credential, VerifyError};

/// The name in EHLO when the host has none; RFC 5321 section 4.1.1.1
/// takes an address literal then.
const EHLO_FALLBACK: &str = "[127.0.0.1]";

/// "Service ready", the greeting (RFC 5321 section 4.2.3).
const SERVICE_READY: u16 = 220;

/// "Service not available, closing transmission channel".
const SERVICE_CLOSING: u16 = 421;

/// "Authentication credentials invalid" (RFC 4954 section 6).
const CREDENTIALS_INVALID: u16 = 535;

/// The stream types the client library accepts.
trait Wire: AsyncRead + AsyncWrite + Unpin {}

impl<T: AsyncRead + AsyncWrite + Unpin> Wire for T {}

/// Connects, signs in and quits. Nothing in the error carries the
/// credential.
///
/// # Errors
///
/// Returns the cause the user or the operator can act on;
/// `AuthUnavailable` when the server offers no mechanism this credential
/// can answer.
pub async fn verify(
    tls: Arc<ClientConfig>,
    target: &Target,
    credential: &Credential,
    step_timeout: Duration,
) -> Result<(), VerifyError> {
    let connector = TlsConnector::from(tls);
    let ehlo_host = ehlo_host();
    let tcp = connect(&target.addresses, step_timeout).await?;
    let mut client = match target.tls {
        TlsMode::Implicit => {
            let mut client = tcp
                .into_tls(&connector, &target.host)
                .await
                .map_err(session_error)?;
            greeting(&mut client).await?;
            client
        }
        TlsMode::Starttls => {
            let mut plain = tcp;
            greeting(&mut plain).await?;
            if !ehlo(&mut plain, &ehlo_host)
                .await?
                .has_capability(EXT_START_TLS)
            {
                return Err(SessionError::StarttlsAbsent.into());
            }
            match plain.start_tls(&connector, &target.host).await {
                Ok(client) => client,
                Err(ClientError::UnexpectedReply(_)) => {
                    return Err(SessionError::StarttlsRefused.into());
                }
                Err(error) => return Err(session_error(error).into()),
            }
        }
    };
    let mut offered = ehlo(&mut client, &ehlo_host).await?;
    // Only the mechanisms this credential can answer; a server offering
    // none of them sees no credential.
    offered.auth_mechanisms &= mechanisms(credential);
    let outcome = if offered.auth_mechanisms == 0 {
        Err(VerifyError::AuthUnavailable)
    } else {
        sign_in(&mut client, credential, &offered).await
    };
    // The outcome above is the answer; a QUIT that fails changes nothing.
    let _ended = client.quit().await;
    outcome
}

/// The addresses in order; the last failure is the answer when none
/// accepts.
async fn connect(
    addresses: &[SocketAddr],
    step: Duration,
) -> Result<SmtpClient<TcpStream>, SessionError> {
    let mut failure = SessionError::NoAddress;
    for address in addresses {
        match SmtpClient::connect(*address, step).await {
            Ok(client) => return Ok(client),
            Err(ClientError::Io(error)) => failure = SessionError::Connect(error),
            Err(error) => failure = session_error(error),
        }
    }
    Err(failure)
}

/// Reads the greeting under the step timeout; only a 220 leads
/// anywhere, a 421 says the server is closing (RFC 5321 section 3.1).
async fn greeting<T: Wire>(client: &mut SmtpClient<T>) -> Result<(), SessionError> {
    let step = client.timeout;
    let reply = timeout(step, client.read())
        .await
        .map_err(|_elapsed| SessionError::Timeout)?
        .map_err(session_error)?;
    match reply.code() {
        SERVICE_READY => Ok(()),
        SERVICE_CLOSING => Err(SessionError::Closed),
        _ => Err(SessionError::Protocol("the greeting is not a 220")),
    }
}

/// EHLO; the answer names what the server offers (RFC 5321 section
/// 4.1.1.1).
async fn ehlo<T: Wire>(
    client: &mut SmtpClient<T>,
    host: &str,
) -> Result<EhloResponse<String>, SessionError> {
    match client.ehlo(host).await {
        Ok(offered) => Ok(offered),
        Err(ClientError::UnexpectedReply(_)) => Err(SessionError::Protocol("EHLO was refused")),
        Err(error) => Err(session_error(error)),
    }
}

/// AUTH through the client library; a 535 answers the credential
/// (RFC 4954 section 6), any other refusal is about the server.
async fn sign_in<T: Wire>(
    client: &mut SmtpClient<T>,
    credential: &Credential,
    offered: &EhloResponse<String>,
) -> Result<(), VerifyError> {
    let credentials = match credential {
        Credential::Password { username, password } => Credentials::Plain {
            username: username.as_str(),
            secret: password.as_str(),
        },
        Credential::Xoauth2 { username, token } => Credentials::XOauth2 {
            username: username.as_str(),
            secret: token.as_str(),
        },
    };
    match client.authenticate(&credentials, offered).await {
        Ok(_signed_in) => Ok(()),
        Err(ClientError::AuthenticationFailed(reply)) if reply.code() == CREDENTIALS_INVALID => {
            Err(VerifyError::CredentialRejected)
        }
        Err(ClientError::AuthenticationFailed(_)) => {
            Err(SessionError::Protocol("the sign-in was not accepted").into())
        }
        Err(error) => Err(session_error(error).into()),
    }
}

/// The mechanisms a credential can answer (RFC 4616 for PLAIN, the
/// LOGIN draft and Google's XOAUTH2).
fn mechanisms(credential: &Credential) -> u64 {
    match credential {
        Credential::Password { .. } => AUTH_PLAIN | AUTH_LOGIN,
        Credential::Xoauth2 { .. } => AUTH_XOAUTH2,
    }
}

/// The name this side says in EHLO: the host's own, as every mail
/// client does.
fn ehlo_host() -> String {
    gethostname::gethostname()
        .into_string()
        .ok()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| EHLO_FALLBACK.to_owned())
}

/// The client library's error in the bridge's words; the server's own
/// text never travels, since it could echo a credential.
fn session_error(error: ClientError) -> SessionError {
    match error {
        ClientError::Io(error) => io_error(error),
        ClientError::Tls(error) => {
            SessionError::Tls(io::Error::new(io::ErrorKind::InvalidData, *error))
        }
        ClientError::Timeout => SessionError::Timeout,
        ClientError::InvalidTLSName => SessionError::ServerName(InvalidDnsNameError),
        ClientError::UnsupportedAuthMechanism => SessionError::AuthUnavailable,
        ClientError::UnparseableReply => SessionError::Protocol("the answer could not be parsed"),
        ClientError::Base64(_) | ClientError::Auth(_) => {
            SessionError::Protocol("the challenge could not be read")
        }
        ClientError::UnexpectedReply(_)
        | ClientError::AuthenticationFailed(_)
        | ClientError::MissingCredentials
        | ClientError::MissingMailFrom
        | ClientError::MissingRcptTo
        | ClientError::MissingStartTls => SessionError::Protocol("the command was not accepted"),
    }
}

#[cfg(test)]
mod tests {
    use smtp_proto::Response;
    use tokio_rustls::rustls;

    use super::*;

    #[test]
    fn a_password_answers_plain_or_login_and_a_token_xoauth2() {
        let password = Credential::Password {
            username: "sanne".to_owned(),
            password: "hunter2 but longer".to_owned(),
        };
        let token = Credential::Xoauth2 {
            username: "sanne".to_owned(),
            token: "ya29.secret".to_owned(),
        };
        assert_eq!(mechanisms(&password), AUTH_PLAIN | AUTH_LOGIN);
        assert_eq!(mechanisms(&token), AUTH_XOAUTH2);
    }

    #[test]
    fn the_client_library_errors_map_to_fixed_words() {
        assert!(matches!(
            session_error(ClientError::Timeout),
            SessionError::Timeout
        ));
        assert!(matches!(
            session_error(ClientError::InvalidTLSName),
            SessionError::ServerName(_)
        ));
        assert!(matches!(
            session_error(ClientError::UnsupportedAuthMechanism),
            SessionError::AuthUnavailable
        ));
        assert!(matches!(
            session_error(ClientError::UnparseableReply),
            SessionError::Protocol(_)
        ));
        let refused = rustls::Error::General("handshake refused".to_owned());
        assert!(matches!(
            session_error(ClientError::Tls(Box::new(refused))),
            SessionError::Tls(_)
        ));
        let reply = Response {
            code: 550,
            esc: [5, 1, 1],
            message: "the secret".to_owned(),
        };
        let mapped = session_error(ClientError::UnexpectedReply(reply));
        assert!(!format!("{mapped} {mapped:?}").contains("the secret"));
    }

    #[test]
    fn the_ehlo_name_is_never_empty() {
        assert!(!ehlo_host().is_empty());
    }
}
