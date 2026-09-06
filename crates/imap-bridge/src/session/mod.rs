// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The IMAP session layer: one narrow trait over the client library, so
//! a swap costs one module. The causes below serve the SMTP check too.

mod imap;

use std::collections::BTreeSet;
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use thiserror::Error;
use tokio_rustls::rustls::ClientConfig;
use tokio_rustls::rustls::pki_types::InvalidDnsNameError;

pub use imap::ImapSession;

/// One connect attempt or one command gets this long; a server slower
/// than that counts as unreachable.
pub const STEP_TIMEOUT: Duration = Duration::from_secs(20);

/// How a connection is encrypted; plaintext is not an option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsMode {
    /// TLS from the first byte.
    Implicit,
    /// A plaintext greeting, then STARTTLS before any credential.
    Starttls,
}

/// A server to reach. The caller resolves the host and checks the
/// addresses; the bridge connects to them in order and resolves nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// The name the certificate must carry, sent as SNI.
    pub host: String,
    /// Where to connect, port included, tried in order.
    pub addresses: Vec<SocketAddr>,
    /// Whether TLS starts at the first byte or after STARTTLS.
    pub tls: TlsMode,
}

/// What the server advertises, compared without regard to case
/// (RFC 9051 section 9, note 1).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Capabilities {
    names: BTreeSet<String>,
}

impl Capabilities {
    /// Whether `name` was advertised, in any case.
    #[must_use]
    pub fn has(&self, name: &str) -> bool {
        self.names.contains(&name.to_ascii_uppercase())
    }

    /// Every advertised name in upper case, sorted.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.names.iter().map(String::as_str)
    }
}

impl FromIterator<String> for Capabilities {
    fn from_iter<I: IntoIterator<Item = String>>(names: I) -> Self {
        Self {
            names: names
                .into_iter()
                .map(|name| name.to_ascii_uppercase())
                .collect(),
        }
    }
}

/// Why a step of an IMAP or SMTP session failed. No variant carries a
/// credential.
#[derive(Debug, Error)]
pub enum SessionError {
    #[error("no address to connect to")]
    NoAddress,
    #[error("cannot connect: {0}")]
    Connect(#[source] io::Error),
    #[error("the host is not a valid server name")]
    ServerName(#[source] InvalidDnsNameError),
    #[error("TLS failed: {0}")]
    Tls(#[source] io::Error),
    #[error("the server does not offer STARTTLS")]
    StarttlsAbsent,
    #[error("the server refused STARTTLS")]
    StarttlsRefused,
    #[error("the server took too long")]
    Timeout,
    #[error("the server closed the connection")]
    Closed,
    #[error("the server refused the credential")]
    CredentialRejected,
    #[error("the server offers no way to sign in with this credential")]
    AuthUnavailable,
    /// The text is fixed at the call site, never the server's own words.
    #[error("the server does not speak the protocol as expected: {0}")]
    Protocol(&'static str),
    #[error("read or write failed: {0}")]
    Io(#[source] io::Error),
}

/// The seam over the client library. Every step runs within the timeout
/// the connection was opened with.
pub trait Session: Sized + Send {
    /// Opens the connection with validated TLS and reads the greeting; a
    /// STARTTLS target upgrades before any credential is sent.
    ///
    /// # Errors
    ///
    /// Returns an error when no address accepts the connection, TLS
    /// cannot be established or the greeting is not an OK.
    fn connect(
        tls: Arc<ClientConfig>,
        target: &Target,
        step_timeout: Duration,
    ) -> impl Future<Output = Result<Self, SessionError>> + Send;

    /// The LOGIN command (RFC 9051 section 6.2.3).
    ///
    /// # Errors
    ///
    /// Returns `CredentialRejected` when the server answers NO; other
    /// errors are about the connection or the server.
    fn login(
        &mut self,
        username: &str,
        password: &str,
    ) -> impl Future<Output = Result<(), SessionError>> + Send;

    /// AUTHENTICATE with the XOAUTH2 mechanism (RFC 9051 section 6.2.2).
    ///
    /// # Errors
    ///
    /// Returns `CredentialRejected` when the server answers NO; other
    /// errors are about the connection or the server.
    fn authenticate_xoauth2(
        &mut self,
        username: &str,
        token: &str,
    ) -> impl Future<Output = Result<(), SessionError>> + Send;

    /// The CAPABILITY command (RFC 9051 section 6.1.1), in any state.
    ///
    /// # Errors
    ///
    /// Returns an error when the connection fails or the server answers
    /// without capability data.
    fn capabilities(&mut self) -> impl Future<Output = Result<Capabilities, SessionError>> + Send;

    /// The LOGOUT command (RFC 9051 section 6.1.3); the connection is
    /// gone afterwards whatever the answer.
    ///
    /// # Errors
    ///
    /// Returns an error when the server does not answer the command.
    fn logout(self) -> impl Future<Output = Result<(), SessionError>> + Send;
}

/// Bytes a client library cannot parse arrive under the kind `Other`
/// and a connection that ends mid-response as an unexpected end.
pub(crate) fn io_error(error: io::Error) -> SessionError {
    match error.kind() {
        io::ErrorKind::Other => SessionError::Protocol("the answer could not be parsed"),
        io::ErrorKind::UnexpectedEof => SessionError::Closed,
        _ => SessionError::Io(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_capability_matches_without_regard_to_case_rfc9051_9_note_1() {
        let capabilities: Capabilities = ["IMAP4rev2".to_owned(), "auth=xoauth2".to_owned()]
            .into_iter()
            .collect();
        assert!(capabilities.has("imap4rev2"));
        assert!(capabilities.has("AUTH=XOAUTH2"));
        assert!(!capabilities.has("STARTTLS"));
        assert_eq!(
            capabilities.iter().collect::<Vec<_>>(),
            ["AUTH=XOAUTH2", "IMAP4REV2"]
        );
    }
}
