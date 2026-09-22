// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The IMAP session layer: one narrow trait over the client library, so
//! a swap costs one module. The causes below serve the SMTP check too.

mod capability;
#[cfg(feature = "test-support")]
pub mod fuzzing;
mod guard;
mod imap;
mod message;
mod read;

use std::collections::BTreeSet;
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use thiserror::Error;
use tokio_rustls::rustls::ClientConfig;
use tokio_rustls::rustls::pki_types::InvalidDnsNameError;

pub use guard::{MAX_NESTING, MAX_RESPONSE_BYTES, MAX_STRUCTURED_BYTES};
pub use imap::ImapSession;
pub use message::{
    BodyPart, FetchItems, FetchedMessage, FlagFetch, Flagged, GmailItems, MAX_FETCH_MESSAGES,
    MAX_FLAGGED, MAX_HEADER_BYTES, MAX_PREVIEW_TEXT_BYTES, MAX_PREVIEWS, MESSAGE_LIMIT,
    PREVIEW_HEADER_BYTES, PreviewAsk, PreviewBytes, Selected, UidRange,
};

/// One connect attempt or one step of a command gets this long: the
/// whole command where the client library runs it, each response where
/// the bridge reads the answer itself. A slower server counts as
/// unreachable.
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

impl Extend<String> for Capabilities {
    fn extend<I: IntoIterator<Item = String>>(&mut self, names: I) {
        self.names
            .extend(names.into_iter().map(|name| name.to_ascii_uppercase()));
    }
}

impl FromIterator<String> for Capabilities {
    fn from_iter<I: IntoIterator<Item = String>>(names: I) -> Self {
        let mut found = Self::default();
        found.extend(names);
        found
    }
}

/// The RETURN options of one LIST (RFC 5258 section 3), each behind the
/// capability that defines it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ListReturn {
    /// `SUBSCRIBED`, so every line carries `\Subscribed` where it
    /// applies (RFC 5258 section 3.1).
    pub subscribed: bool,
    /// `SPECIAL-USE`, the role attributes (RFC 6154 section 2).
    pub special_use: bool,
    /// `STATUS (...)` after every selectable name (RFC 5819).
    pub status: Option<StatusItems>,
}

/// The items one STATUS asks for: MESSAGES, UNSEEN, UIDNEXT and
/// UIDVALIDITY always, HIGHESTMODSEQ where CONDSTORE is advertised
/// (RFC 7162 section 3.1.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusItems {
    pub modseq: bool,
}

/// One LIST or LSUB line: the name as the server spells it, the
/// hierarchy delimiter and every attribute in upper case with its
/// backslash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListEntry {
    pub name: String,
    pub delimiter: Option<char>,
    pub attributes: Vec<String>,
}

/// One STATUS line; an item the server left out is `None`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatusEntry {
    pub mailbox: String,
    pub messages: Option<u32>,
    pub unseen: Option<u32>,
    pub uid_next: Option<u32>,
    pub uid_validity: Option<u32>,
    pub highest_modseq: Option<u64>,
}

/// What one LIST answered: the names and the STATUS lines that rode
/// along.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listing {
    pub entries: Vec<ListEntry>,
    pub statuses: Vec<StatusEntry>,
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

    /// `LIST "" "*"` with the given RETURN options (RFC 9051 section
    /// 6.3.9, RFC 5258), signed in. Every untagged line up to the tagged
    /// answer is read here, so a STATUS line rides along and an alert
    /// in between is skipped. A name that holds a control character or
    /// is too long to send back is left out.
    ///
    /// # Errors
    ///
    /// Returns `Refused` when the server answers NO and `Protocol` when
    /// the session is not signed in, the server answers BAD, the answer
    /// passes the line limit or the listing passes the mailbox limit;
    /// other errors are about the connection. Past either limit, as
    /// after `Timeout`, the stream holds unread lines and the session
    /// must be dropped.
    fn list(
        &mut self,
        options: ListReturn,
    ) -> impl Future<Output = Result<Listing, SessionError>> + Send;

    /// `LSUB "" "*"` (RFC 3501 section 6.3.9): the subscribed names of a
    /// server without LIST-EXTENDED.
    ///
    /// # Errors
    ///
    /// As [`Session::list`].
    fn lsub(&mut self) -> impl Future<Output = Result<Vec<String>, SessionError>> + Send;

    /// STATUS of one mailbox (RFC 9051 section 6.3.11), by its wire name.
    ///
    /// # Errors
    ///
    /// As [`Session::list`]; `Protocol` as well when no STATUS line
    /// comes back and, before anything is sent, when the name holds a
    /// control character or is too long.
    fn status(
        &mut self,
        mailbox: &str,
        items: StatusItems,
    ) -> impl Future<Output = Result<StatusEntry, SessionError>> + Send;

    /// EXAMINE of one mailbox by its wire name (RFC 3501 section 6.3.2):
    /// read-only, so nothing the read path does changes a flag. From
    /// here on every answer has room for the lines other clients cause.
    ///
    /// # Errors
    ///
    /// As [`Session::status`]; `Protocol` as well when the answer lacks
    /// EXISTS or UIDVALIDITY. A server whose NO carries bytes outside
    /// ASCII fails the parse, which reads as `Protocol`.
    fn examine(
        &mut self,
        mailbox: &str,
    ) -> impl Future<Output = Result<Selected, SessionError>> + Send;

    /// Every UID of the selected mailbox, highest first. It asks `UID
    /// SEARCH` for one window of sequence numbers at a time (RFC 3501
    /// section 6.4.4), so no answer grows with the folder.
    ///
    /// # Errors
    ///
    /// As [`Session::list`]; `Protocol` as well when no mailbox is
    /// selected and past the UID limit of one folder.
    fn uid_list(&mut self) -> impl Future<Output = Result<Vec<u32>, SessionError>> + Send;

    /// `UID FETCH` of the header items for a range of the selected
    /// mailbox, with BODYSTRUCTURE and the Gmail items as `items` asks;
    /// the messages by UID. Lines for other UIDs and flag updates are
    /// skipped.
    ///
    /// # Errors
    ///
    /// As [`Session::list`]; `Protocol` as well for more than
    /// `MAX_FETCH_MESSAGES` messages, a date the format refuses or no
    /// `UTCDate` can render, a mod-sequence past 63 bits and a response
    /// past a bound of the guard. After any of them the session must be
    /// dropped. Before anything is sent, `Protocol` when no mailbox is
    /// selected or the range runs backward.
    fn uid_fetch(
        &mut self,
        range: UidRange,
        items: FetchItems,
    ) -> impl Future<Output = Result<Vec<FetchedMessage>, SessionError>> + Send;

    /// `UID FETCH` of the flags alone, X-GM-LABELS next to them when
    /// `labels` is set: what changed since a mod-sequence (CONDSTORE,
    /// RFC 7162 section 3.1.4.1) or every message of a range. The
    /// messages by UID.
    ///
    /// # Errors
    ///
    /// As [`Session::uid_fetch`], the limit being `MAX_FLAGGED`.
    fn uid_flags(
        &mut self,
        fetch: FlagFetch,
        labels: bool,
    ) -> impl Future<Output = Result<Vec<Flagged>, SessionError>> + Send;

    /// `UID FETCH` of the start of one text part for several messages,
    /// every item a partial fetch (RFC 3501 section 6.4.5). A message
    /// that did not answer is left out.
    ///
    /// # Errors
    ///
    /// As [`Session::list`]; `Protocol` as well, before anything is
    /// sent, when no mailbox is selected, the ask names no message or
    /// more than `MAX_PREVIEWS` or its part number holds other bytes
    /// than digits and dots.
    fn uid_previews(
        &mut self,
        ask: &PreviewAsk<'_>,
    ) -> impl Future<Output = Result<Vec<PreviewBytes>, SessionError>> + Send;

    /// NOOP (RFC 3501 section 6.1.2): whether a connection kept from an
    /// earlier use still answers.
    ///
    /// # Errors
    ///
    /// As [`Session::list`].
    fn noop(&mut self) -> impl Future<Output = Result<(), SessionError>> + Send;

    /// The LOGOUT command (RFC 9051 section 6.1.3); the connection is
    /// gone afterwards whatever the answer.
    ///
    /// # Errors
    ///
    /// Returns an error when the server does not answer the command.
    fn logout(self) -> impl Future<Output = Result<(), SessionError>> + Send;
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
    }

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
