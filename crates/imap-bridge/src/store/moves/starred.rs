// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The Starred rule of a Gmail account at the rows: a starred message of
//! All Mail is a member of the `\Flagged` label mailbox and a member of
//! it is starred, since the server keeps the two together.

use rusqlite::params;

use super::super::changes::{ChangeKind, ObjectType};
use super::super::gmail::{self, Labels};
use super::super::{EmailId, MailboxId, StoreError};
use super::{Writing, row_of};

/// The keyword a starred message carries (RFC 8621 section 4.1.1).
const FLAGGED: &str = "$flagged";

impl Writing<'_, '_> {
    /// When the row on its folder shows labels, the starred label
    /// mailbox follows its keyword.
    pub(super) fn star(&mut self, id: &EmailId, folder: &MailboxId) -> Result<(), StoreError> {
        let labels = Labels::read(self.transaction, self.key, folder)?;
        let Some(starred) = labels.starred() else {
            return Ok(());
        };
        let Some((keywords, received_at)) = row_of(self.transaction, self.key, id)? else {
            return Ok(());
        };
        let current = gmail::current_memberships(self.transaction, self.key, id)?;
        let flagged = keywords.contains_key(FLAGGED);
        if current.contains(starred) == flagged {
            return Ok(());
        }
        let mut wanted = current.clone();
        if flagged {
            wanted.insert(starred.clone());
        } else {
            wanted.remove(starred);
        }
        self.memberships((id, folder, received_at), (&current, &wanted))
    }

    /// Sets or clears `$flagged` on the row.
    pub(super) fn flagged(&mut self, id: &EmailId, flagged: bool) -> Result<(), StoreError> {
        let Some((mut keywords, _)) = row_of(self.transaction, self.key, id)? else {
            return Ok(());
        };
        if keywords.contains_key(FLAGGED) == flagged {
            return Ok(());
        }
        if flagged {
            keywords.insert(FLAGGED.to_owned(), true);
        } else {
            keywords.remove(FLAGGED);
        }
        self.transaction.execute(
            "UPDATE bridge_emails SET keywords = ?3 WHERE account_key = ?1 AND id = ?2",
            params![self.key.as_str(), id, serde_json::to_string(&keywords)?],
        )?;
        self.ledger
            .note(ObjectType::Email, id.as_str(), ChangeKind::Updated);
        Ok(())
    }
}
