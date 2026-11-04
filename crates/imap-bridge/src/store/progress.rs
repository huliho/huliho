// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! How far a folder stands: its first sync and, once that is done, the
//! values of the server the cache answers for. A batch moves it inside
//! the transaction that writes the batch.

use rusqlite::{OptionalExtension, Transaction, params};

use super::emails::Batch;
use super::refresh::Synced;
use super::{AccountKey, MailboxId, Store, StoreError};

/// How far a folder stands after a batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Advance {
    /// The first sync, newest first.
    Down {
        /// The lowest UID the sync has passed; `None` for an empty
        /// folder.
        lowest_uid: Option<u32>,
        /// Whether the folder holds nothing below `lowest_uid`.
        done: bool,
        /// The server's values when this opening of the sync read the
        /// folder. The first write records them and a later opening keeps
        /// them, so the first refresh meets all that arrived or changed
        /// since the sync began.
        synced: Synced,
    },
    /// New mail of a folder that is done, oldest first: every UID below
    /// `uid_next` has been asked for and the server answered `arrived`
    /// messages for the batch, which the count on record grows by.
    Up { uid_next: u32, arrived: u32 },
    /// Rows read beside both, for a draft: nothing is said about how far
    /// the folder stands, whatever a walk on another connection did.
    Beside,
}

/// How far a folder stands: its first sync and, once that is done, the
/// values of the server the cache answers for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Progress {
    pub lowest_synced_uid: Option<u32>,
    pub done: bool,
    /// Empty until the first sync is done: before that the recorded values
    /// say where the sync began, not how far the folder is synced.
    pub synced: Synced,
}

impl Store {
    /// How far a folder stands.
    ///
    /// # Errors
    ///
    /// Returns the database error.
    pub fn sync_progress(
        &self,
        key: &AccountKey,
        folder: &MailboxId,
    ) -> Result<Progress, StoreError> {
        self.read(|connection| {
            let progress = connection
                .query_row(
                    "SELECT lowest_synced_uid, done, uid_next, highest_modseq, messages
                     FROM bridge_sync WHERE account_key = ?1 AND folder_id = ?2",
                    params![key.as_str(), folder],
                    |row| {
                        let done = row.get(1)?;
                        let synced = if done {
                            Synced {
                                uid_next: row.get(2)?,
                                highest_modseq: row.get(3)?,
                                messages: row.get(4)?,
                            }
                        } else {
                            Synced::default()
                        };
                        Ok(Progress {
                            lowest_synced_uid: row.get(0)?,
                            done,
                            synced,
                        })
                    },
                )
                .optional()?;
            Ok(progress.unwrap_or_default())
        })
    }
}

/// Writes how far the folder stands; whether this write is the one
/// that finishes its first sync, which moves its counts to the
/// memberships. The first write of a first sync records the server's
/// values of its opening and every later one keeps them; a row that holds
/// none takes those of the opening that writes.
pub(super) fn save(
    transaction: &Transaction<'_>,
    key: &AccountKey,
    batch: &Batch<'_>,
) -> Result<bool, StoreError> {
    let folder = params![key.as_str(), batch.folder];
    let (lowest_uid, done, synced) = match batch.advance {
        Advance::Down {
            lowest_uid,
            done,
            synced,
        } => (lowest_uid, done, synced),
        Advance::Up { uid_next, arrived } => {
            // A count never learned stays unknown: NULL plus a number is NULL.
            transaction.execute(
                "UPDATE bridge_sync SET uid_next = MAX(COALESCE(uid_next, 0), ?3),
                        messages = messages + ?4
                 WHERE account_key = ?1 AND folder_id = ?2",
                params![key.as_str(), batch.folder, uid_next, arrived],
            )?;
            return Ok(false);
        }
        Advance::Beside => return Ok(false),
    };
    let was_done: Option<bool> = transaction
        .query_row(
            "SELECT done FROM bridge_sync WHERE account_key = ?1 AND folder_id = ?2",
            folder,
            |row| row.get(0),
        )
        .optional()?;
    // MESSAGES comes with every opening, so a NULL count marks a row that
    // holds no values yet; the three move together, as one EXAMINE gave them.
    transaction.execute(
        "INSERT INTO bridge_sync
           (account_key, folder_id, lowest_synced_uid, done, uid_next, highest_modseq, messages)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT (account_key, folder_id) DO UPDATE SET
           lowest_synced_uid = COALESCE(excluded.lowest_synced_uid, lowest_synced_uid),
           done = excluded.done,
           uid_next = CASE WHEN messages IS NULL THEN excluded.uid_next ELSE uid_next END,
           highest_modseq = CASE WHEN messages IS NULL
                                 THEN excluded.highest_modseq ELSE highest_modseq END,
           messages = COALESCE(messages, excluded.messages)",
        params![
            key.as_str(),
            batch.folder,
            lowest_uid,
            done,
            synced.uid_next,
            synced.highest_modseq,
            synced.messages
        ],
    )?;
    Ok(done && was_done != Some(true))
}
