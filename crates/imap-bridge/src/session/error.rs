// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Why a step of a session failed, in fixed words that never carry a
//! credential or the server's own text.

use std::io;

use thiserror::Error;
use tokio_rustls::rustls::pki_types::InvalidDnsNameError;

use super::guard;

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
    /// The server could not judge the credential: a subsystem behind it
    /// is down (RFC 5530 section 3, `UNAVAILABLE`).
    #[error("the server cannot sign anyone in right now")]
    Unavailable,
    /// A tagged NO outside the sign-in (RFC 3501 section 7.1.2).
    #[error("the server answered NO")]
    Refused,
    /// The text is fixed at the call site, never the server's own words.
    #[error("the server does not speak the protocol as expected: {0}")]
    Protocol(&'static str),
    #[error("read or write failed: {0}")]
    Io(#[source] io::Error),
}

/// Bytes a client library cannot parse arrive under the kind `Other`
/// and a connection that ends mid-response as an unexpected end; a
/// bound of the guard travels inside the error.
pub(crate) fn io_error(error: io::Error) -> SessionError {
    let limit = error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<guard::Limit>());
    if let Some(limit) = limit {
        return SessionError::Protocol(limit.words());
    }
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
    fn a_bound_of_the_guard_reads_as_a_protocol_failure_in_fixed_words() {
        for (limit, words) in [
            (guard::Limit::Nesting, "the answer nests too deep"),
            (guard::Limit::Size, "the answer passes the byte limit"),
            (
                guard::Limit::Structure,
                "the answer passes the structure limit",
            ),
            (
                guard::Limit::Literal,
                "the answer holds a literal in free text",
            ),
        ] {
            let error = io_error(limit.into());
            assert!(
                matches!(error, SessionError::Protocol(found) if found == words),
                "{error}"
            );
        }
        let other = io_error(io::Error::other("garbage"));
        assert!(matches!(other, SessionError::Protocol(_)));
    }
}
