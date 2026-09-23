// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The email rows: what a sync batch writes as one state and what
//! `Email/get` reads, the personal fields sealed by the host.

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use super::changes::{ChangeKind, ObjectType, log, next_sequence, prune, state_of};
use super::folders::{self, FolderWrite, Leaving, Standing};
use super::gmail::{self, GmailFacts};
use super::ledger::Ledger;
use super::personal::Personal;
use super::progress::{self, Advance};
use super::threads;
use super::{AccountKey, EmailId, MailboxId, Store, StoreError, ThreadId};
use crate::seal::Sealer;

/// What the sync knows about one message before the store names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailFacts {
    pub uid: u32,
    pub keywords: BTreeMap<String, bool>,
    pub size: u32,
    pub received_at: i64,
    pub sent_at: Option<i64>,
    pub has_attachment: bool,
    /// The Gmail items where the server gave them; a message without
    /// them is stored the way a folder account stores every message.
    pub gmail: Option<GmailFacts>,
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
    pub advance: Advance,
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
    /// destroyed. A message with its Gmail items keeps the row that
    /// holds its id wherever that row lies.
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
        self.write(key, |transaction| {
            write_batch(transaction, key, batch, sealer)
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
    let standing = Standing {
        folder: batch.folder,
        uid_validity: batch.uid_validity,
    };
    if !folders::stands(transaction, key, &standing)? {
        return Ok(None);
    }
    let mut waiting = folders::unmatched(transaction, key, batch.folder)?;
    let labels = gmail::Labels::read(transaction, key, batch.folder)?;
    let mut ledger = Ledger::default();
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
        if let Some(items) = &facts.gmail {
            let arrival = gmail::Arrival {
                key,
                folder: batch.folder,
                facts,
                gmail: items,
                hash: hash.as_deref(),
                labels: &labels,
            };
            if let Some((id, kind)) = arrival.write(transaction, sealer, &mut ledger)? {
                ledger.note(ObjectType::Email, id.as_str(), kind);
            }
            continue;
        }
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
            let mut named: Vec<Vec<u8>> = Vec::new();
            for hash in facts
                .personal
                .named_ids()
                .map(|id| sealer.keyed_hash(id.as_bytes()))
            {
                if !named.contains(&hash) {
                    named.push(hash);
                }
            }
            let thread = threads::join(transaction, key, &named, &mut ledger)?;
            insert(transaction, key, facts, (&row, &thread))?;
        } else {
            rewrite(transaction, key, facts, &row)?;
        }
        ledger.note(ObjectType::Email, id.as_str(), kind);
    }
    let finished = progress::save(transaction, key, batch)?;
    if ledger.is_empty() && !finished {
        return state_of(transaction, key).map(Some);
    }
    let sequence = next_sequence(transaction, key)?;
    // The ledger first: what leaves below folds onto its rows.
    ledger.write(transaction, key, sequence)?;
    if finished {
        let write = FolderWrite {
            key,
            folder: batch.folder,
            sequence,
        };
        folders::leave(transaction, &write, Leaving::Unmatched)?;
    }
    let mailbox = (batch.folder.as_str(), ChangeKind::Updated);
    log(transaction, key, (sequence, ObjectType::Mailbox), mailbox)?;
    prune(transaction, key)?;
    Ok(Some(sequence))
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

/// A new row in the thread its header names, with its one membership.
fn insert(
    transaction: &Transaction<'_>,
    key: &AccountKey,
    facts: &EmailFacts,
    (row, thread): (&Written<'_>, &ThreadId),
) -> Result<(), StoreError> {
    let folder = row.folder;
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
    Ok(())
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
