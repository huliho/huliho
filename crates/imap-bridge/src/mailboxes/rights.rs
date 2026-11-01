// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What the signed-in user may do with a mailbox (RFC 8621 section 2):
//! the letters MYRIGHTS named where the server advertises ACL, every
//! right the server's commands allow where it does not, then the rules
//! that hold whatever the letters say.

use serde_json::{Map, Value};

use super::INBOX;
use crate::session::Rights;
use crate::store::{MailboxRow, MailboxSnapshot};

/// Every right of RFC 4314 section 2.1 as `kept` spells them: what a
/// mailbox holds while the server cannot say which (RFC 4314 section
/// 3.5, RFC 8440 section 3).
pub const EVERY_RIGHT: &str = "aeiklprstwx";

/// One right of RFC 8621 section 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Right {
    ReadItems,
    AddItems,
    RemoveItems,
    SetSeen,
    SetKeywords,
    CreateChild,
    Rename,
    Delete,
    Submit,
}

impl Right {
    /// Every right in the order of RFC 8621 section 2.
    pub const ALL: [Self; 9] = [
        Self::ReadItems,
        Self::AddItems,
        Self::RemoveItems,
        Self::SetSeen,
        Self::SetKeywords,
        Self::CreateChild,
        Self::Rename,
        Self::Delete,
        Self::Submit,
    ];

    /// The property of the `MailboxRights` object.
    #[must_use]
    pub fn property(self) -> &'static str {
        match self {
            Self::ReadItems => "mayReadItems",
            Self::AddItems => "mayAddItems",
            Self::RemoveItems => "mayRemoveItems",
            Self::SetSeen => "maySetSeen",
            Self::SetKeywords => "maySetKeywords",
            Self::CreateChild => "mayCreateChild",
            Self::Rename => "mayRename",
            Self::Delete => "mayDelete",
            Self::Submit => "maySubmit",
        }
    }

    /// The letters of RFC 4314 section 4 the right takes, all of them;
    /// none for submission, which no IMAP right grants.
    fn letters(self) -> Option<&'static str> {
        match self {
            Self::ReadItems => Some("r"),
            Self::AddItems => Some("i"),
            Self::RemoveItems => Some("te"),
            Self::SetSeen => Some("s"),
            Self::SetKeywords => Some("w"),
            Self::CreateChild => Some("k"),
            Self::Rename | Self::Delete => Some("x"),
            Self::Submit => None,
        }
    }
}

/// Whether the letters a mailbox's row keeps grant the right, the
/// server's commands and the fixed rules aside. A mailbox without letters
/// grants every right but submission (RFC 4314 section 3.5).
#[must_use]
pub fn allows(letters: Option<&str>, right: Right) -> bool {
    let Some(needed) = right.letters() else {
        return false;
    };
    letters.is_none_or(|held| needed.chars().all(|letter| held.contains(letter)))
}

/// The rights the user holds on one mailbox.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MailboxRights([bool; Right::ALL.len()]);

impl MailboxRights {
    /// Whether the right is held.
    #[must_use]
    pub fn may(self, right: Right) -> bool {
        self.0[right as usize]
    }

    fn with(mut self, right: Right, held: bool) -> Self {
        self.0[right as usize] = held;
        self
    }

    /// The `MailboxRights` object, every right named.
    #[must_use]
    pub fn object(self) -> Value {
        let rights: Map<String, Value> = Right::ALL
            .iter()
            .map(|right| (right.property().to_owned(), Value::Bool(self.may(*right))))
            .collect();
        Value::Object(rights)
    }
}

/// The letters a mailbox's row keeps: the ones MYRIGHTS named, with the
/// `c` and `d` of a server that predates RFC 4314 read as the rights
/// they cover in either reading of its section 2.1.1, so they never
/// grant more than the server does.
#[must_use]
pub fn kept(rights: &Rights, obsolete: bool) -> String {
    let mut letters: Vec<char> = rights.letters().chars().collect();
    if obsolete {
        let (create, delete) = (rights.has('c'), rights.has('d'));
        letters.extend(create.then_some('k'));
        letters.extend(delete.then_some('t'));
        letters.extend(delete.then_some('e'));
        letters.extend((create && delete).then_some('x'));
    }
    letters.sort_unstable();
    letters.dedup();
    letters.into_iter().collect()
}

/// The rights on one row of the snapshot.
#[must_use]
pub fn of(row: &MailboxRow, snapshot: &MailboxSnapshot) -> MailboxRights {
    let facts = &row.facts;
    let letters = snapshot.rights.get(&row.id).map(String::as_str);
    let held = |right: Right| allows(letters, right);
    let fixed = snapshot.gmail && is_gmail_own(row);
    let none = MailboxRights::default();
    let child = held(Right::CreateChild) && !fixed;
    if !facts.selectable {
        return none.with(Right::CreateChild, child);
    }
    let moves = if snapshot.gmail && facts.store {
        snapshot.moving.move_command
    } else {
        snapshot.moving.moves()
    };
    let editable = held(Right::Delete) && !fixed && !facts.imap_name.eq_ignore_ascii_case(INBOX);
    none.with(Right::ReadItems, held(Right::ReadItems))
        .with(Right::AddItems, moves && held(Right::AddItems))
        .with(Right::RemoveItems, moves && held(Right::RemoveItems))
        .with(Right::SetSeen, held(Right::SetSeen))
        .with(Right::SetKeywords, held(Right::SetKeywords))
        .with(Right::CreateChild, child)
        .with(Right::Rename, editable)
        .with(Right::Delete, editable && facts.role.is_none())
}

/// A mailbox Gmail keeps for itself: a store, a system label or a name
/// nothing can select, such as `[Gmail]`.
fn is_gmail_own(row: &MailboxRow) -> bool {
    let facts = &row.facts;
    let system = facts
        .gmail_label
        .as_deref()
        .is_some_and(|label| label.starts_with('\\'));
    facts.store || system || !facts.selectable
}

#[cfg(test)]
mod tests;
