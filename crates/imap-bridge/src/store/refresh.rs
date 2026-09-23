// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What a refresh writes on a folder whose first sync is done: the
//! keywords that changed, the messages that left and how far the folder
//! is synced. Every write is one state and goes through only while the
//! folder stands under the UIDVALIDITY the refresh read it under.

use std::collections::BTreeMap;

use rusqlite::{OptionalExtension, Transaction, params};

use super::changes::{ChangeKind, ObjectType, log, next_sequence, prune};
use super::folders::{self, Standing};
use super::gmail;
use super::ledger::Ledger;
use super::threads;
use super::{AccountKey, EmailId, Store, StoreError, ThreadId};

/// The keywords one message carries now, and its labels where the
/// server gave them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlagChange {
    pub uid: u32,
    pub keywords: BTreeMap<String, bool>,
    pub labels: Option<Vec<String>>,
}

/// The values of the server a folder's cache answers for: every UID
/// below `uid_next` has been asked for, the flags stand at
/// `highest_modseq` and `messages` is MESSAGES as the server had it
/// when the folder was last brought up to date. A value never learned is
/// `None`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Synced {
    pub uid_next: Option<u32>,
    pub highest_modseq: Option<u64>,
    pub messages: Option<u32>,
}

impl Store {
    /// The UIDs the folder holds, lowest first; rows that wait for a
    /// match after a renumbering hold none.
    ///
    /// # Errors
    ///
    /// Returns the database error.
    pub fn stored_uids(
        &self,
        key: &AccountKey,
        standing: &Standing<'_>,
    ) -> Result<Vec<u32>, StoreError> {
        self.read(|connection| {
            let mut statement = connection.prepare(
                "SELECT uid FROM bridge_emails
                 WHERE account_key = ?1 AND folder_id = ?2 AND uid > 0 ORDER BY uid",
            )?;
            let uids = statement
                .query_map(params![key.as_str(), standing.folder], |row| row.get(0))?
                .collect::<Result<_, _>>()?;
            Ok(uids)
        })
    }

    /// Writes the keywords that differ from the rows as one state, and
    /// the memberships where the labels say otherwise; a UID the folder
    /// does not hold is skipped. `None` when the folder does not stand
    /// anymore; the state as it was when nothing differs.
    ///
    /// # Errors
    ///
    /// Returns the database error.
    pub fn apply_flags(
        &self,
        key: &AccountKey,
        standing: &Standing<'_>,
        changes: &[FlagChange],
    ) -> Result<Option<u64>, StoreError> {
        self.write(key, |transaction| {
            let mut ledger = Ledger::default();
            if !folders::stands(transaction, key, standing)? {
                return Ok(None);
            }
            let labels = gmail::Labels::read(transaction, key, standing.folder)?;
            for change in changes {
                let keywords = serde_json::to_string(&change.keywords)?;
                let updated: Option<EmailId> = transaction
                    .query_row(
                        "UPDATE bridge_emails SET keywords = ?4
                         WHERE account_key = ?1 AND folder_id = ?2 AND uid = ?3
                           AND keywords <> ?4
                         RETURNING id",
                        params![key.as_str(), standing.folder, change.uid, keywords],
                        |row| row.get(0),
                    )
                    .optional()?;
                if let Some(id) = updated {
                    ledger.note(ObjectType::Email, id.as_str(), ChangeKind::Updated);
                }
                if let Some(found) = &change.labels
                    && let Some((id, received_at)) =
                        gmail::row_at(transaction, key, standing.folder, change.uid)?
                {
                    let row = gmail::Labeled {
                        id: &id,
                        folder: standing.folder,
                        labels: found,
                        received_at,
                    };
                    labels.relabel(transaction, &row, &mut ledger)?;
                }
            }
            close(transaction, key, standing, &ledger).map(Some)
        })
    }

    /// Destroys the messages of these UIDs as one state, each thread
    /// updated or destroyed with its last email. `None` when the folder
    /// does not stand anymore.
    ///
    /// # Errors
    ///
    /// Returns the database error.
    pub fn remove_uids(
        &self,
        key: &AccountKey,
        standing: &Standing<'_>,
        uids: &[u32],
    ) -> Result<Option<u64>, StoreError> {
        self.write(key, |transaction| {
            let mut ledger = Ledger::default();
            if !folders::stands(transaction, key, standing)? {
                return Ok(None);
            }
            for uid in uids {
                let removed: Option<(EmailId, ThreadId)> = transaction
                    .query_row(
                        "DELETE FROM bridge_emails
                         WHERE account_key = ?1 AND folder_id = ?2 AND uid = ?3
                         RETURNING id, thread_id",
                        params![key.as_str(), standing.folder, uid],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .optional()?;
                let Some((id, thread)) = removed else {
                    continue;
                };
                transaction.execute(
                    "DELETE FROM bridge_memberships WHERE account_key = ?1 AND email_id = ?2",
                    params![key.as_str(), id],
                )?;
                ledger.note(ObjectType::Email, id.as_str(), ChangeKind::Destroyed);
                threads::left(transaction, key, &thread, &mut ledger)?;
            }
            close(transaction, key, standing, &ledger).map(Some)
        })
    }

    /// Records how far the refresh brought the folder; a value it did
    /// not learn keeps the one on record. No state moves: none of the
    /// values shows in JMAP. Nothing is written for a folder that does
    /// not stand anymore.
    ///
    /// # Errors
    ///
    /// Returns the database error.
    pub fn advance(
        &self,
        key: &AccountKey,
        standing: &Standing<'_>,
        synced: Synced,
    ) -> Result<(), StoreError> {
        self.write(key, |transaction| {
            if !folders::stands(transaction, key, standing)? {
                return Ok(());
            }
            transaction.execute(
                "UPDATE bridge_sync SET uid_next = COALESCE(?3, uid_next),
                        highest_modseq = COALESCE(?4, highest_modseq),
                        messages = COALESCE(?5, messages)
                 WHERE account_key = ?1 AND folder_id = ?2",
                params![
                    key.as_str(),
                    standing.folder,
                    synced.uid_next,
                    synced.highest_modseq,
                    synced.messages
                ],
            )?;
            Ok(())
        })
    }
}

/// Ends a write on one folder: with anything in the ledger a new state
/// holds it and the mailbox reads as updated, since its counts moved.
/// Answers the state the account stands at.
fn close(
    transaction: &Transaction<'_>,
    key: &AccountKey,
    standing: &Standing<'_>,
    ledger: &Ledger,
) -> Result<u64, StoreError> {
    if ledger.is_empty() {
        return super::changes::state_of(transaction, key);
    }
    let sequence = next_sequence(transaction, key)?;
    ledger.write(transaction, key, sequence)?;
    let mailbox = (standing.folder.as_str(), ChangeKind::Updated);
    log(transaction, key, (sequence, ObjectType::Mailbox), mailbox)?;
    prune(transaction, key)?;
    Ok(sequence)
}
