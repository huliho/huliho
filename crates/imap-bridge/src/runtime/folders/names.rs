// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The wire name of every mailbox as the call has left it so far: the
//! rows', then what each CREATE and RENAME did, so a later step names
//! what the server holds (RFC 8620 section 5.3).

use std::collections::HashMap;

use super::delimiters::moved;
use crate::store::{MailboxId, MailboxRow};

/// One wire name a RENAME moved: the mailbox, its old name and its new
/// one.
pub(super) struct Moved {
    pub id: MailboxId,
    pub from: String,
    pub to: String,
}

pub(super) struct Names(HashMap<MailboxId, String>);

impl Names {
    pub(super) fn of(rows: &[MailboxRow]) -> Self {
        Self(
            rows.iter()
                .map(|row| (row.id.clone(), row.facts.imap_name.clone()))
                .collect(),
        )
    }

    /// The wire name the mailbox holds now; `None` for one the call did
    /// not make.
    pub(super) fn wire(&self, id: &MailboxId) -> Option<&str> {
        self.0.get(id).map(String::as_str)
    }

    pub(super) fn made(&mut self, id: MailboxId, wire: String) {
        self.0.insert(id, wire);
    }

    /// Every name a RENAME moved, by the delimiter under the old name,
    /// the mailbox itself first and the rest by length.
    pub(super) fn renamed(
        &mut self,
        (from, to): (&str, &str),
        delimiter: Option<char>,
    ) -> Vec<Moved> {
        let mut found: Vec<Moved> = self
            .0
            .iter()
            .filter_map(|(id, wire)| {
                let to = moved(wire, (from, to), delimiter)?;
                Some(Moved {
                    id: id.clone(),
                    from: wire.clone(),
                    to,
                })
            })
            .collect();
        found.sort_by_key(|moved| moved.from.len());
        for moved in &found {
            self.0.insert(moved.id.clone(), moved.to.clone());
        }
        found
    }
}
