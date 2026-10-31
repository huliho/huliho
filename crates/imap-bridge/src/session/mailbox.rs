// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The folder commands on the raw path: CREATE, RENAME, DELETE,
//! SUBSCRIBE and UNSUBSCRIBE (RFC 3501 sections 6.3.3 to 6.3.7) and
//! MYRIGHTS (RFC 4314 section 3.5), each read as STATUS is. A name the
//! user typed reaches a command line only as a [`MailboxName`].

use std::fmt;

use async_imap::imap_proto::{AclRight, Response};
use thiserror::Error;
use unicode_normalization::UnicodeNormalization as _;

use super::SessionError;
use super::read::{MAX_WIRE_NAME_BYTES, Room, Selection, quoted};
use super::write::fitting;
use crate::utf7;

/// The bytes of UTF-8 one mailbox name holds at most, its parents left
/// out: what the session object advertises as `maxSizeMailboxName`
/// (RFC 8621 section 1.3.1).
pub const MAX_MAILBOX_NAME_BYTES: usize = 255;

/// The lines of its own one MYRIGHTS answer carries.
const MYRIGHTS_LINES: usize = 1;

/// Why a name the user typed cannot name a mailbox. Nothing is sent
/// for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum NameError {
    #[error("the name is empty")]
    Empty,
    /// The name passes `MAX_MAILBOX_NAME_BYTES` or, with its parents,
    /// `MAX_WIRE_NAME_BYTES`.
    #[error("the name passes the byte limit")]
    TooLong,
    #[error("the name holds a control character")]
    Control,
    /// The delimiter in the name as the wire spells it.
    #[error("the name holds the hierarchy delimiter")]
    Delimiter,
    #[error("the name starts or ends with white space")]
    EdgeSpace,
    /// A parent on a server that keeps no hierarchy.
    #[error("the server keeps no hierarchy")]
    Flat,
}

/// Where a new name goes: under the mailbox of that wire name, or at
/// the top level. `delimiter` is the one LIST gave for that level, the
/// parent's line or the INBOX line's at the top; `None` on a server
/// that keeps no hierarchy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Place<'a> {
    pub parent: Option<&'a str>,
    pub delimiter: Option<char>,
}

/// A name the user typed, fit for a command line: in NFC (RFC 8621
/// section 2 wants Net-Unicode), checked, then in modified UTF-7 under
/// its parent.
#[derive(Clone, PartialEq, Eq)]
pub struct MailboxName {
    leaf: String,
    wire: String,
}

/// The lengths alone, so a debug format never carries the name into a
/// log line.
impl fmt::Debug for MailboxName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MailboxName")
            .field("leaf_bytes", &self.leaf.len())
            .field("wire_bytes", &self.wire.len())
            .finish()
    }
}

impl MailboxName {
    /// The name `typed` at `place`.
    ///
    /// # Errors
    ///
    /// Returns why the name cannot be sent; [`NameError`] names each
    /// case.
    pub fn new(typed: &str, place: Place<'_>) -> Result<Self, NameError> {
        let leaf: String = typed.nfc().collect();
        if leaf.is_empty() {
            return Err(NameError::Empty);
        }
        if leaf.len() > MAX_MAILBOX_NAME_BYTES {
            return Err(NameError::TooLong);
        }
        if leaf.chars().any(char::is_control) {
            return Err(NameError::Control);
        }
        let encoded = utf7::encode(&leaf);
        if place.delimiter.is_some_and(|found| encoded.contains(found)) {
            return Err(NameError::Delimiter);
        }
        if leaf.starts_with(char::is_whitespace) || leaf.ends_with(char::is_whitespace) {
            return Err(NameError::EdgeSpace);
        }
        let wire = match (place.parent, place.delimiter) {
            (None, _) => encoded,
            (Some(parent), Some(delimiter)) => format!("{parent}{delimiter}{encoded}"),
            (Some(_), None) => return Err(NameError::Flat),
        };
        if wire.len() > MAX_WIRE_NAME_BYTES {
            return Err(NameError::TooLong);
        }
        Ok(Self { leaf, wire })
    }

    /// The name as the user sees it, in NFC.
    #[must_use]
    pub fn leaf(&self) -> &str {
        &self.leaf
    }

    /// The whole name as the server spells it, parents included.
    #[must_use]
    pub fn wire(&self) -> &str {
        &self.wire
    }
}

/// The rights the signed-in user holds on one mailbox (RFC 4314 section
/// 2.1): every lowercase letter and digit the server named, once, in
/// ASCII order. Any other character is left out (RFC 4314 section 7), so
/// it never grants a right.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rights(String);

impl Rights {
    pub(crate) fn of(found: &[AclRight]) -> Self {
        let mut letters: Vec<char> = found
            .iter()
            .map(|right| char::from(*right))
            .filter(|letter| letter.is_ascii_lowercase() || letter.is_ascii_digit())
            .collect();
        letters.sort_unstable();
        letters.dedup();
        Self(letters.into_iter().collect())
    }

    /// The letters, as the store keeps them.
    #[must_use]
    pub fn letters(&self) -> &str {
        &self.0
    }

    /// Whether the right of that letter is held.
    #[must_use]
    pub fn has(&self, right: char) -> bool {
        self.0.contains(right)
    }
}

pub(super) fn create_command(name: &MailboxName) -> Result<String, SessionError> {
    fitting(format!("CREATE {}", quoted(name.wire())?))
}

pub(super) fn rename_command(from: &str, to: &MailboxName) -> Result<String, SessionError> {
    fitting(format!("RENAME {} {}", quoted(from)?, quoted(to.wire())?))
}

/// One command whose answer carries nothing the bridge reads. A server
/// that spells a created name its own way sends a LIST line along (RFC
/// 9051 section 6.3.4), which passes by.
async fn run(selection: &mut Selection<'_>, command: &str, room: Room) -> Result<(), SessionError> {
    selection.collect(command, room.bounds(0), |_| Ok(())).await
}

pub(super) async fn create(
    selection: &mut Selection<'_>,
    name: &MailboxName,
    room: Room,
) -> Result<(), SessionError> {
    run(selection, &create_command(name)?, room).await
}

pub(super) async fn rename(
    selection: &mut Selection<'_>,
    (from, to): (&str, &MailboxName),
    room: Room,
) -> Result<(), SessionError> {
    run(selection, &rename_command(from, to)?, room).await
}

/// DELETE, SUBSCRIBE or UNSUBSCRIBE of a mailbox by its wire name.
pub(super) async fn named(
    selection: &mut Selection<'_>,
    verb: &str,
    mailbox: &str,
    room: Room,
) -> Result<(), SessionError> {
    let command = fitting(format!("{verb} {}", quoted(mailbox)?))?;
    run(selection, &command, room).await
}

pub(super) async fn my_rights(
    selection: &mut Selection<'_>,
    mailbox: &str,
    room: Room,
) -> Result<Rights, SessionError> {
    let command = format!("MYRIGHTS {}", quoted(mailbox)?);
    let mut found = None;
    let visit = |response: &Response<'_>| {
        if let Response::MyRights(rights) = response {
            found = Some(Rights::of(&rights.rights));
        }
        Ok(())
    };
    selection
        .collect(&command, room.bounds(MYRIGHTS_LINES), visit)
        .await?;
    found.ok_or(SessionError::Protocol("MYRIGHTS answered without data"))
}

#[cfg(test)]
mod tests;
