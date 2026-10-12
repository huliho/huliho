// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The rows whose message left its folder. An expunged one is destroyed
//! with its memberships. One a move took where the write cannot follow
//! it waits for the pass on a Gmail account, which claims it by
//! X-GM-MSGID where the message turned up; elsewhere it leaves.

use rusqlite::{OptionalExtension, params};

use super::super::changes::{ChangeKind, ObjectType};
use super::super::{EmailId, MailboxId, StoreError, ThreadId, gmail, threads};
use super::Writing;

/// A row whose message left its folder: expunged, or moved where the
/// bridge cannot follow it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoneRow {
    pub id: EmailId,
    pub folder: MailboxId,
    pub uid: u32,
}

impl Writing<'_, '_> {
    /// A row whose message left: it is destroyed with its memberships,
    /// its thread updated or destroyed and every mailbox it was a member
    /// of updated. Nothing happens to a row that is not where the
    /// message was.
    pub(super) fn leave(&mut self, gone: &GoneRow) -> Result<(), StoreError> {
        let (transaction, key) = (self.transaction, self.key);
        let removed: Option<ThreadId> = transaction
            .query_row(
                "DELETE FROM bridge_emails
                 WHERE account_key = ?1 AND id = ?2 AND folder_id = ?3 AND uid = ?4
                 RETURNING thread_id",
                params![key.as_str(), gone.id, gone.folder, gone.uid],
                |row| row.get(0),
            )
            .optional()?;
        let Some(thread) = removed else {
            return Ok(());
        };
        for mailbox in gmail::current_memberships(transaction, key, &gone.id)? {
            let mailbox = mailbox.as_str();
            self.ledger
                .note(ObjectType::Mailbox, mailbox, ChangeKind::Updated);
        }
        transaction.execute(
            "DELETE FROM bridge_memberships WHERE account_key = ?1 AND email_id = ?2",
            params![key.as_str(), gone.id],
        )?;
        let email = gone.id.as_str();
        self.ledger
            .note(ObjectType::Email, email, ChangeKind::Destroyed);
        threads::left(transaction, key, &thread, &mut self.ledger)
    }

    /// A row whose message a move took where the write cannot follow it.
    /// On a Gmail account it waits for the pass under a negative UID, the
    /// way a row whose UID left a store does. Nothing is logged then,
    /// since nothing a client sees has moved. Elsewhere it leaves.
    pub(super) fn lose(&mut self, lost: &GoneRow) -> Result<(), StoreError> {
        if self.parks {
            self.park(lost)
        } else {
            self.leave(lost)
        }
    }

    /// Negates the UID of the row where the message was, so the next
    /// pass claims it or sweeps it. Nothing happens to a row elsewhere.
    fn park(&self, lost: &GoneRow) -> Result<(), StoreError> {
        self.transaction.execute(
            "UPDATE bridge_emails SET uid = -uid
             WHERE account_key = ?1 AND id = ?2 AND folder_id = ?3 AND uid = ?4",
            params![self.key.as_str(), lost.id, lost.folder, lost.uid],
        )?;
        Ok(())
    }
}
