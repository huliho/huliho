// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Where the message of an email lies on the server, for the fetch of
//! its preview, and the write that keeps a fetched preview in the
//! sealed blob of its row.

use rusqlite::{OptionalExtension, params};

use super::changes::{ChangeKind, ObjectType, next_sequence, prune, state_of};
use super::ledger::Ledger;
use super::personal::Personal;
use super::{AccountKey, EmailId, MailboxId, Store, StoreError};
use crate::seal::Sealer;

/// An email by the folder and the UID its message lies under, the blob
/// still sealed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located {
    pub id: EmailId,
    pub folder: MailboxId,
    pub uid: u32,
    pub sealed: Vec<u8>,
}

impl Store {
    /// Where the messages of these emails lie. An email the account does
    /// not hold or one that waits for a match after a renumbering is
    /// left out, since no UID names its message.
    ///
    /// # Errors
    ///
    /// Returns the database error.
    pub fn locate(&self, key: &AccountKey, ids: &[&str]) -> Result<Vec<Located>, StoreError> {
        self.read(|connection| {
            let mut statement = connection.prepare_cached(
                "SELECT id, folder_id, uid, sealed FROM bridge_emails
                 WHERE account_key = ?1 AND id = ?2 AND uid > 0",
            )?;
            let mut located = Vec::new();
            for id in ids {
                let found = statement
                    .query_row(params![key.as_str(), id], |row| {
                        Ok(Located {
                            id: row.get(0)?,
                            folder: row.get(1)?,
                            uid: row.get(2)?,
                            sealed: row.get(3)?,
                        })
                    })
                    .optional()?;
                located.extend(found);
            }
            Ok(located)
        })
    }

    /// Keeps the previews in the blobs of their rows as one state, each
    /// email logged as updated. A row that left in between or whose blob
    /// does not open is skipped. Answers the state afterwards.
    ///
    /// # Errors
    ///
    /// Returns the database error or the host's failure to seal.
    pub fn save_previews(
        &self,
        key: &AccountKey,
        sealer: &dyn Sealer,
        previews: &[(EmailId, String)],
    ) -> Result<u64, StoreError> {
        self.write(|transaction| {
            let mut ledger = Ledger::default();
            for (id, preview) in previews {
                let blob: Option<Vec<u8>> = transaction
                    .query_row(
                        "SELECT sealed FROM bridge_emails WHERE account_key = ?1 AND id = ?2",
                        params![key.as_str(), id],
                        |row| row.get(0),
                    )
                    .optional()?;
                let Some(plain) = blob.and_then(|blob| sealer.open(key, id, &blob)) else {
                    continue;
                };
                let mut personal: Personal = serde_json::from_slice(&plain)?;
                personal.preview = Some(preview.clone());
                let blob = sealer.seal(key, id, &serde_json::to_vec(&personal)?)?;
                transaction.execute(
                    "UPDATE bridge_emails SET sealed = ?3 WHERE account_key = ?1 AND id = ?2",
                    params![key.as_str(), id, blob],
                )?;
                ledger.note(ObjectType::Email, id.as_str(), ChangeKind::Updated);
            }
            if ledger.is_empty() {
                return state_of(transaction, key);
            }
            let sequence = next_sequence(transaction, key)?;
            ledger.write(transaction, key, sequence)?;
            prune(transaction, key)?;
            Ok(sequence)
        })
    }
}
