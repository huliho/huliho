// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What a LIST, an LSUB and a STATUS ask for and answer, in the
//! bridge's own types.

use super::Rights;

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
    /// `MYRIGHTS`, the user's rights after every name the server can
    /// look them up for (RFC 8440).
    pub my_rights: bool,
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

/// One MYRIGHTS line: the user's rights on the mailbox of that wire
/// name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RightsEntry {
    pub mailbox: String,
    pub rights: Rights,
}

/// What one LIST answered: the names and the STATUS and MYRIGHTS lines
/// that rode along.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listing {
    pub entries: Vec<ListEntry>,
    pub statuses: Vec<StatusEntry>,
    pub rights: Vec<RightsEntry>,
}
