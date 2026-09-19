// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The mailbox rows: what a listing pass wrote and what `Mailbox/get`
//! reads, with the thread counts over the memberships.

#[cfg(test)]
mod tests;

use std::collections::HashMap;

use rusqlite::{Connection, Row, Transaction, params};

use super::changes::{ChangeKind, ObjectType, log, next_sequence, prune, state_of};
use super::{AccountKey, MailboxId, Store, StoreError};

/// What one listing pass knows about a mailbox before the store names
/// its row: the parent by wire name, since the parent's id may not
/// exist yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailboxFacts {
    pub name: String,
    pub imap_name: String,
    pub parent_imap_name: Option<String>,
    pub role: Option<String>,
    pub sort_order: u32,
    pub subscribed: bool,
    pub selectable: bool,
    pub store: bool,
    pub gmail_label: Option<String>,
    pub uid_validity: Option<u32>,
    pub uid_next: Option<u32>,
    pub highest_modseq: Option<u64>,
    pub total_emails: u32,
    pub unread_emails: u32,
}

/// A mailbox as the table holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailboxRow {
    pub id: MailboxId,
    pub parent_id: Option<MailboxId>,
    pub facts: MailboxFacts,
}

/// What the memberships say about a mailbox (RFC 8621 section 2): the
/// threads with an email in it, the threads with an unread one and the
/// emails the bridge holds for it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub total_threads: u32,
    pub unread_threads: u32,
    pub synced_emails: u32,
}

/// Every mailbox of an account with its counts, read under one lock so
/// the state belongs to the rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailboxSnapshot {
    pub state: u64,
    pub rows: Vec<MailboxRow>,
    pub counts: HashMap<MailboxId, Counts>,
}

const COLUMNS: &str = "id, parent_id, name, imap_name, role, sort_order, subscribed, selectable, \
                       store, gmail_label, uid_validity, uid_next, highest_modseq, total_emails, \
                       unread_emails";

impl Store {
    /// Replaces the account's mailbox rows with what a listing found: a
    /// new name is created, a changed row updated, a missing one
    /// destroyed, every one written to the log under one new state.
    /// Answers the account's state afterwards. `found` names each
    /// mailbox once. A parent it does not hold leaves the mailbox at the
    /// top.
    ///
    /// # Errors
    ///
    /// Returns the database error.
    pub fn apply_mailboxes(
        &self,
        key: &AccountKey,
        found: &[MailboxFacts],
    ) -> Result<u64, StoreError> {
        self.write(|transaction| apply(transaction, key, found))
    }

    /// Every mailbox of the account by sort order then name, with the
    /// counts and the state they belong to.
    ///
    /// # Errors
    ///
    /// Returns the database error.
    pub fn mailbox_snapshot(&self, key: &AccountKey) -> Result<MailboxSnapshot, StoreError> {
        self.read(|connection| {
            Ok(MailboxSnapshot {
                state: state_of(connection, key)?,
                rows: read_rows(connection, key)?,
                counts: read_counts(connection, key)?,
            })
        })
    }
}

fn apply(
    transaction: &Transaction<'_>,
    key: &AccountKey,
    found: &[MailboxFacts],
) -> Result<u64, StoreError> {
    let existing: HashMap<String, MailboxRow> = read_rows(transaction, key)?
        .into_iter()
        .map(|row| (row.facts.imap_name.clone(), row))
        .collect();
    // Ids first, so a parent is named before its own row is written.
    let ids: HashMap<&str, MailboxId> = found
        .iter()
        .map(|facts| {
            let id = existing
                .get(&facts.imap_name)
                .map_or_else(MailboxId::generate, |row| row.id.clone());
            (facts.imap_name.as_str(), id)
        })
        .collect();
    let mut changes = Vec::new();
    for facts in found {
        // The parent's wire name is not a column, so the row compares on
        // the fourteen columns alone.
        let row = MailboxRow {
            id: ids[facts.imap_name.as_str()].clone(),
            parent_id: facts
                .parent_imap_name
                .as_deref()
                .and_then(|parent| ids.get(parent).cloned()),
            facts: MailboxFacts {
                parent_imap_name: None,
                ..facts.clone()
            },
        };
        match existing.get(&facts.imap_name) {
            Some(current) if *current == row => {}
            Some(_) => {
                upsert(transaction, key, &row)?;
                changes.push((row.id, ChangeKind::Updated));
            }
            None => {
                upsert(transaction, key, &row)?;
                changes.push((row.id, ChangeKind::Created));
            }
        }
    }
    for (imap_name, row) in &existing {
        if !ids.contains_key(imap_name.as_str()) {
            transaction.execute(
                "DELETE FROM bridge_mailboxes WHERE account_key = ?1 AND id = ?2",
                params![key.as_str(), row.id],
            )?;
            changes.push((row.id.clone(), ChangeKind::Destroyed));
        }
    }
    if changes.is_empty() {
        return state_of(transaction, key);
    }
    let sequence = next_sequence(transaction, key)?;
    for (id, kind) in &changes {
        log(
            transaction,
            key,
            (sequence, ObjectType::Mailbox),
            (id.as_str(), *kind),
        )?;
    }
    prune(transaction, key)?;
    Ok(sequence)
}

fn upsert(
    transaction: &Transaction<'_>,
    key: &AccountKey,
    row: &MailboxRow,
) -> Result<(), StoreError> {
    let facts = &row.facts;
    transaction.execute(
        "INSERT INTO bridge_mailboxes
         (account_key, id, parent_id, name, imap_name, role, sort_order, subscribed,
          selectable, store, gmail_label, uid_validity, uid_next, highest_modseq,
          total_emails, unread_emails)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
         ON CONFLICT (account_key, id) DO UPDATE SET
           parent_id = excluded.parent_id, name = excluded.name,
           imap_name = excluded.imap_name, role = excluded.role,
           sort_order = excluded.sort_order, subscribed = excluded.subscribed,
           selectable = excluded.selectable, store = excluded.store,
           gmail_label = excluded.gmail_label, uid_validity = excluded.uid_validity,
           uid_next = excluded.uid_next, highest_modseq = excluded.highest_modseq,
           total_emails = excluded.total_emails, unread_emails = excluded.unread_emails",
        params![
            key.as_str(),
            row.id,
            row.parent_id,
            facts.name,
            facts.imap_name,
            facts.role,
            facts.sort_order,
            facts.subscribed,
            facts.selectable,
            facts.store,
            facts.gmail_label,
            facts.uid_validity,
            facts.uid_next,
            facts.highest_modseq,
            facts.total_emails,
            facts.unread_emails
        ],
    )?;
    Ok(())
}

fn read_rows(connection: &Connection, key: &AccountKey) -> Result<Vec<MailboxRow>, StoreError> {
    let mut statement = connection.prepare(&format!(
        "SELECT {COLUMNS} FROM bridge_mailboxes WHERE account_key = ?1
         ORDER BY sort_order, name, id"
    ))?;
    let rows = statement
        .query_map([key.as_str()], row_from)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn row_from(row: &Row<'_>) -> rusqlite::Result<MailboxRow> {
    Ok(MailboxRow {
        id: row.get(0)?,
        parent_id: row.get(1)?,
        facts: MailboxFacts {
            name: row.get(2)?,
            imap_name: row.get(3)?,
            parent_imap_name: None,
            role: row.get(4)?,
            sort_order: row.get(5)?,
            subscribed: row.get(6)?,
            selectable: row.get(7)?,
            store: row.get(8)?,
            gmail_label: row.get(9)?,
            uid_validity: row.get(10)?,
            uid_next: row.get(11)?,
            highest_modseq: row.get(12)?,
            total_emails: row.get(13)?,
            unread_emails: row.get(14)?,
        },
    })
}

/// The counts of RFC 8621 section 2: a thread is unread while one of its
/// emails carries neither `$seen` nor `$draft`.
fn read_counts(
    connection: &Connection,
    key: &AccountKey,
) -> Result<HashMap<MailboxId, Counts>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT m.mailbox_id, COUNT(*), COUNT(DISTINCT e.thread_id),
                COUNT(DISTINCT CASE
                    WHEN json_extract(e.keywords, '$.\"$seen\"') IS NULL
                     AND json_extract(e.keywords, '$.\"$draft\"') IS NULL
                    THEN e.thread_id END)
         FROM bridge_memberships m
         JOIN bridge_emails e ON e.account_key = m.account_key AND e.id = m.email_id
         WHERE m.account_key = ?1
         GROUP BY m.mailbox_id",
    )?;
    let counts = statement
        .query_map([key.as_str()], |row| {
            Ok((
                row.get::<_, MailboxId>(0)?,
                Counts {
                    synced_emails: row.get(1)?,
                    total_threads: row.get(2)?,
                    unread_threads: row.get(3)?,
                },
            ))
        })?
        .collect::<Result<HashMap<_, _>, _>>()?;
    Ok(counts)
}
