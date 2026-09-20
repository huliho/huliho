// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The email rows: what a sync batch writes as one state and what
//! `Email/get` reads, the personal fields sealed by the host.

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

use super::changes::{ChangeKind, ObjectType, log, next_sequence, prune, state_of};
use super::folders::{self, FolderWrite, Leaving};
use super::{AccountKey, EmailId, MailboxId, Store, StoreError, ThreadId};
use crate::seal::Sealer;

/// One address as `Email/get` renders it (RFC 8621 section 4.1.2.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Address {
    pub name: Option<String>,
    pub email: String,
}

/// The personal fields of an email, the JSON inside the sealed blob.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Personal {
    pub from: Option<Vec<Address>>,
    pub to: Option<Vec<Address>>,
    pub cc: Option<Vec<Address>>,
    pub bcc: Option<Vec<Address>>,
    pub reply_to: Option<Vec<Address>>,
    pub sender: Option<Vec<Address>>,
    pub subject: Option<String>,
    pub message_id: Option<Vec<String>>,
    pub in_reply_to: Option<Vec<String>>,
    pub references: Option<Vec<String>>,
}

/// What the sync knows about one message before the store names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailFacts {
    pub uid: u32,
    pub keywords: BTreeMap<String, bool>,
    pub size: u32,
    pub received_at: i64,
    pub sent_at: Option<i64>,
    pub has_attachment: bool,
    pub personal: Personal,
}

/// One step of a folder's sync: the messages of one fetch and how far
/// the folder stands after them.
#[derive(Debug, Clone, Copy)]
pub struct Batch<'a> {
    pub folder: &'a MailboxId,
    /// The UIDVALIDITY the messages were fetched under.
    pub uid_validity: u32,
    pub emails: &'a [EmailFacts],
    /// The lowest UID the sync has passed; `None` for an empty folder.
    pub lowest_uid: Option<u32>,
    /// Whether the folder holds nothing below `lowest_uid`.
    pub done: bool,
}

/// How far a folder's first sync stands.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Progress {
    pub lowest_synced_uid: Option<u32>,
    pub done: bool,
}

/// An email as the tables hold it, the blob still sealed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailRow {
    pub id: EmailId,
    pub thread_id: ThreadId,
    pub mailbox_ids: Vec<MailboxId>,
    pub keywords: BTreeMap<String, bool>,
    pub size: u32,
    pub received_at: i64,
    pub sent_at: Option<i64>,
    pub has_attachment: bool,
    pub sealed: Vec<u8>,
}

/// The rows a `/get` named, read under one lock so the state belongs to
/// them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailSnapshot {
    pub state: u64,
    pub rows: Vec<EmailRow>,
}

impl Store {
    /// Writes one batch as one state: the rows with their memberships
    /// and threads, the progress and the log. `None` when the folder is
    /// gone or renumbered since the fetch, in which case nothing is
    /// written. A message the folder already holds is skipped. After a
    /// renumbering a message keeps the id of the waiting row that
    /// matches it; what still waits once the folder is done is
    /// destroyed.
    ///
    /// # Errors
    ///
    /// Returns the database error or the host's failure to seal.
    pub fn apply_batch(
        &self,
        key: &AccountKey,
        batch: &Batch<'_>,
        sealer: &dyn Sealer,
    ) -> Result<Option<u64>, StoreError> {
        self.write(|transaction| write_batch(transaction, key, batch, sealer))
    }

    /// How far the first sync of a folder stands.
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
                    "SELECT lowest_synced_uid, done FROM bridge_sync
                     WHERE account_key = ?1 AND folder_id = ?2",
                    params![key.as_str(), folder],
                    |row| {
                        Ok(Progress {
                            lowest_synced_uid: row.get(0)?,
                            done: row.get(1)?,
                        })
                    },
                )
                .optional()?;
            Ok(progress.unwrap_or_default())
        })
    }

    /// The emails among `ids` the account holds, in the order asked.
    ///
    /// # Errors
    ///
    /// Returns the database error or `Encoding` for a keywords column
    /// that is not a JSON object.
    pub fn emails(&self, key: &AccountKey, ids: &[&str]) -> Result<EmailSnapshot, StoreError> {
        self.read(|connection| {
            let mut rows = Vec::new();
            for id in ids {
                rows.extend(read_email(connection, key, id)?);
            }
            Ok(EmailSnapshot {
                state: state_of(connection, key)?,
                rows,
            })
        })
    }
}

fn write_batch(
    transaction: &Transaction<'_>,
    key: &AccountKey,
    batch: &Batch<'_>,
    sealer: &dyn Sealer,
) -> Result<Option<u64>, StoreError> {
    if !folder_stands(transaction, key, batch)? {
        return Ok(None);
    }
    let mut waiting = folders::unmatched(transaction, key, batch.folder)?;
    let mut entries = Vec::new();
    for facts in batch.emails {
        if holds(transaction, key, batch.folder, facts.uid)? {
            continue;
        }
        let hash = facts
            .personal
            .message_id
            .as_deref()
            .and_then(<[String]>::first)
            .map(|id| sealer.keyed_hash(id.as_bytes()));
        let identity = (hash, facts.received_at, facts.size);
        let matched = waiting.get_mut(&identity).and_then(Vec::pop);
        let (id, kind) = match matched {
            Some(id) => (id, ChangeKind::Updated),
            None => (EmailId::generate(), ChangeKind::Created),
        };
        let plain = serde_json::to_vec(&facts.personal)?;
        let row = Written {
            id: &id,
            folder: batch.folder,
            hash: identity.0.as_deref(),
            sealed: &sealer.seal(key, &id, &plain)?,
        };
        if kind == ChangeKind::Created {
            let thread = insert(transaction, key, facts, &row)?;
            entries.push((ObjectType::Thread, thread.to_string(), kind));
        } else {
            rewrite(transaction, key, facts, &row)?;
        }
        entries.push((ObjectType::Email, id.to_string(), kind));
    }
    let finished = save_progress(transaction, key, batch)?;
    if entries.is_empty() && !finished {
        return state_of(transaction, key).map(Some);
    }
    let sequence = next_sequence(transaction, key)?;
    if finished {
        let write = FolderWrite {
            key,
            folder: batch.folder,
            sequence,
        };
        folders::leave(transaction, &write, Leaving::Unmatched)?;
    }
    for (object, id, kind) in &entries {
        log(transaction, key, (sequence, *object), (id, *kind))?;
    }
    let mailbox = (batch.folder.as_str(), ChangeKind::Updated);
    log(transaction, key, (sequence, ObjectType::Mailbox), mailbox)?;
    prune(transaction, key)?;
    Ok(Some(sequence))
}

/// Whether the folder row exists under the UIDVALIDITY of the batch,
/// which covers a removed account, a vanished mailbox and a
/// renumbering since the fetch.
fn folder_stands(
    transaction: &Transaction<'_>,
    key: &AccountKey,
    batch: &Batch<'_>,
) -> Result<bool, StoreError> {
    let uid_validity: Option<Option<u32>> = transaction
        .query_row(
            "SELECT uid_validity FROM bridge_mailboxes WHERE account_key = ?1 AND id = ?2",
            params![key.as_str(), batch.folder],
            |row| row.get(0),
        )
        .optional()?;
    Ok(uid_validity == Some(Some(batch.uid_validity)))
}

fn holds(
    connection: &Connection,
    key: &AccountKey,
    folder: &MailboxId,
    uid: u32,
) -> Result<bool, StoreError> {
    Ok(connection.query_row(
        "SELECT EXISTS (SELECT 1 FROM bridge_emails
                        WHERE account_key = ?1 AND folder_id = ?2 AND uid = ?3)",
        params![key.as_str(), folder, uid],
        |row| row.get(0),
    )?)
}

/// What the store adds to the facts of one row.
struct Written<'a> {
    id: &'a EmailId,
    folder: &'a MailboxId,
    hash: Option<&'a [u8]>,
    sealed: &'a [u8],
}

/// A new row in a thread of its own; the thread it made.
fn insert(
    transaction: &Transaction<'_>,
    key: &AccountKey,
    facts: &EmailFacts,
    row: &Written<'_>,
) -> Result<ThreadId, StoreError> {
    let (folder, thread) = (row.folder, ThreadId::generate());
    transaction.execute(
        "INSERT INTO bridge_emails
         (account_key, id, folder_id, uid, thread_id, keywords, size, received_at, sent_at,
          has_attachment, message_id_hash, sealed)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            key.as_str(),
            row.id,
            folder,
            facts.uid,
            thread,
            serde_json::to_string(&facts.keywords)?,
            facts.size,
            facts.received_at,
            facts.sent_at,
            facts.has_attachment,
            row.hash,
            row.sealed
        ],
    )?;
    transaction.execute(
        "INSERT INTO bridge_memberships (account_key, email_id, mailbox_id, received_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![key.as_str(), row.id, folder, facts.received_at],
    )?;
    Ok(thread)
}

/// A waiting row under its new UID; the id, the thread and the
/// membership stay.
fn rewrite(
    transaction: &Transaction<'_>,
    key: &AccountKey,
    facts: &EmailFacts,
    row: &Written<'_>,
) -> Result<(), StoreError> {
    transaction.execute(
        "UPDATE bridge_emails SET uid = ?3, keywords = ?4, sent_at = ?5, has_attachment = ?6,
                sealed = ?7
         WHERE account_key = ?1 AND id = ?2",
        params![
            key.as_str(),
            row.id,
            facts.uid,
            serde_json::to_string(&facts.keywords)?,
            facts.sent_at,
            facts.has_attachment,
            row.sealed
        ],
    )?;
    Ok(())
}

/// Writes how far the folder stands; whether this write is the one
/// that finishes it, which moves its counts to the memberships.
fn save_progress(
    transaction: &Transaction<'_>,
    key: &AccountKey,
    batch: &Batch<'_>,
) -> Result<bool, StoreError> {
    let was_done: Option<bool> = transaction
        .query_row(
            "SELECT done FROM bridge_sync WHERE account_key = ?1 AND folder_id = ?2",
            params![key.as_str(), batch.folder],
            |row| row.get(0),
        )
        .optional()?;
    transaction.execute(
        "INSERT INTO bridge_sync (account_key, folder_id, lowest_synced_uid, done)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (account_key, folder_id) DO UPDATE SET
           lowest_synced_uid = COALESCE(excluded.lowest_synced_uid, lowest_synced_uid),
           done = excluded.done",
        params![key.as_str(), batch.folder, batch.lowest_uid, batch.done],
    )?;
    Ok(batch.done && was_done != Some(true))
}

fn read_email(
    connection: &Connection,
    key: &AccountKey,
    id: &str,
) -> Result<Option<EmailRow>, StoreError> {
    let found = connection
        .query_row(
            "SELECT id, thread_id, keywords, size, received_at, sent_at, has_attachment, sealed
             FROM bridge_emails WHERE account_key = ?1 AND id = ?2",
            params![key.as_str(), id],
            |row| {
                let keywords: String = row.get(2)?;
                let email = EmailRow {
                    id: row.get(0)?,
                    thread_id: row.get(1)?,
                    mailbox_ids: Vec::new(),
                    keywords: BTreeMap::new(),
                    size: row.get(3)?,
                    received_at: row.get(4)?,
                    sent_at: row.get(5)?,
                    has_attachment: row.get(6)?,
                    sealed: row.get(7)?,
                };
                Ok((email, keywords))
            },
        )
        .optional()?;
    let Some((mut email, keywords)) = found else {
        return Ok(None);
    };
    email.keywords = serde_json::from_str(&keywords)?;
    let mut statement = connection.prepare_cached(
        "SELECT mailbox_id FROM bridge_memberships
         WHERE account_key = ?1 AND email_id = ?2 ORDER BY mailbox_id",
    )?;
    email.mailbox_ids = statement
        .query_map(params![key.as_str(), id], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(Some(email))
}
