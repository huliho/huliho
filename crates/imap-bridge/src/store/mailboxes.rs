// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The mailbox rows: what a listing pass wrote and what `Mailbox/get`
//! reads, with the thread counts over the memberships and the rights
//! MYRIGHTS named.

mod counts;
mod rights;
mod set;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};

use rusqlite::{Connection, Row, Transaction, params};

use super::changes::{ChangeKind, ObjectType, log, next_sequence, prune, state_of};
use super::folders::{self, FolderWrite};
use super::{AccountKey, MailboxId, Store, StoreError};
use crate::session::Moving;

pub use set::{MailboxChange, MailboxesWritten, PassInputs, RoleOverride};

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

#[cfg(test)]
impl MailboxFacts {
    /// A selectable, subscribed, empty folder of that wire name with
    /// that role, its leaf as its name.
    pub(crate) fn folder(imap_name: &str, role: Option<&str>) -> Self {
        Self {
            name: imap_name.rsplit('/').next().unwrap_or(imap_name).to_owned(),
            imap_name: imap_name.to_owned(),
            parent_imap_name: None,
            role: role.map(str::to_owned),
            sort_order: 6,
            subscribed: true,
            selectable: true,
            store: true,
            gmail_label: None,
            uid_validity: Some(1),
            uid_next: Some(1),
            highest_modseq: None,
            total_emails: 0,
            unread_emails: 0,
        }
    }
}

/// A mailbox as the table holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailboxRow {
    pub id: MailboxId,
    pub parent_id: Option<MailboxId>,
    pub facts: MailboxFacts,
}

/// What the memberships say about a mailbox (RFC 8621 section 2): the
/// threads with an email in it, the threads with an unread one, the
/// emails the bridge holds for it and the unread ones among them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub total_threads: u32,
    pub unread_threads: u32,
    pub synced_emails: u32,
    pub unread_emails: u32,
}

/// Every mailbox of an account with its counts, read under one lock so
/// the state belongs to the rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailboxSnapshot {
    pub state: u64,
    pub rows: Vec<MailboxRow>,
    pub counts: HashMap<MailboxId, Counts>,
    /// The mailboxes whose email counts come from the memberships: a
    /// folder whose first sync is done and, on Gmail, a label mailbox
    /// once the All Mail store is.
    pub done: HashSet<MailboxId>,
    /// Whether the account is a Gmail account: some row carries a label,
    /// which is how a write tells it too.
    pub gmail: bool,
    /// The commands the server offers for moving mail, as the last
    /// mailbox pass of this process read them; none before the first.
    pub moving: Moving,
    /// The rights MYRIGHTS named per mailbox, as letters; a mailbox not
    /// named holds every right the server's commands allow.
    pub rights: HashMap<MailboxId, String>,
}

/// One mailbox pass as the store writes it.
#[derive(Debug, Clone, Copy)]
pub struct Pass<'a> {
    pub found: &'a [MailboxFacts],
    /// The commands the server offers for moving mail.
    pub moving: Moving,
    /// The letters MYRIGHTS named, by wire name, where the server
    /// advertises ACL; a name left out loses the letters on record.
    /// `None` where it does not, which clears them.
    pub rights: Option<&'a HashMap<String, String>>,
}

const COLUMNS: &str = "id, parent_id, name, imap_name, role, sort_order, subscribed, selectable, \
                       store, gmail_label, uid_validity, uid_next, highest_modseq, total_emails, \
                       unread_emails";

impl Store {
    /// Replaces the account's mailbox rows with what a listing found: a
    /// new name is created, a changed row updated, a missing one
    /// destroyed with its emails, every one written to the log under one
    /// new state. The emails of a folder whose UIDVALIDITY changed wait
    /// for the fresh sync to match them. Answers the account's state
    /// afterwards. `found` names each mailbox once. A parent it does not
    /// hold leaves the mailbox at the top. Every row loses its letters.
    ///
    /// # Errors
    ///
    /// Returns the database error.
    pub fn apply_mailboxes(
        &self,
        key: &AccountKey,
        found: &[MailboxFacts],
    ) -> Result<u64, StoreError> {
        let kept = HashMap::new();
        self.write(key, |transaction| {
            apply(transaction, key, (found, Some(&kept)), false)
        })
    }

    /// One mailbox pass: [`Store::apply_mailboxes`] with the commands the
    /// server offers for moving mail and the rights as the pass read
    /// them. When the commands are new to this process or changed, every
    /// mailbox reads as updated under the pass's state, since its rights
    /// follow them; a mailbox whose letters changed reads so too. The
    /// commands are checked and recorded under one lock around the
    /// write, and a write that fails records nothing.
    ///
    /// # Errors
    ///
    /// Returns the database error.
    pub fn apply_pass(&self, key: &AccountKey, pass: Pass<'_>) -> Result<u64, StoreError> {
        let mut recorded = self.moving_lock();
        let flipped = recorded.get(key) != Some(&pass.moving);
        let state = self.write(key, |transaction| {
            apply(transaction, key, (pass.found, pass.rights), flipped)
        })?;
        recorded.insert(key.clone(), pass.moving);
        Ok(state)
    }

    /// Every mailbox of the account by sort order then name, with the
    /// counts and the state they belong to. The commands the server
    /// offers for moving mail are read under the lock a pass records
    /// them under, so the words belong to the rows.
    ///
    /// # Errors
    ///
    /// Returns the database error.
    pub fn mailbox_snapshot(&self, key: &AccountKey) -> Result<MailboxSnapshot, StoreError> {
        // Held through the read, so no pass can land between the two.
        let recorded = self.moving_lock();
        let moving = recorded.get(key).copied().unwrap_or_default();
        self.read(|connection| {
            let rows = read_rows(connection, key)?;
            let gmail = rows.iter().any(|row| row.facts.gmail_label.is_some());
            Ok(MailboxSnapshot {
                state: state_of(connection, key)?,
                rows,
                counts: counts::read_counts(connection, key)?,
                done: counts::read_done(connection, key)?,
                gmail,
                moving,
                rights: rights::read(connection, key)?,
            })
        })
    }
}

/// The pass's rows and letters written; with `rights_moved` every
/// mailbox the pass found reads as updated.
fn apply(
    transaction: &Transaction<'_>,
    key: &AccountKey,
    (found, letters): (&[MailboxFacts], Option<&HashMap<String, String>>),
    rights_moved: bool,
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
    let mut renumbered = Vec::new();
    for facts in found {
        let recorded = existing.get(&facts.imap_name);
        // The parent's wire name is not a column, so the row compares on
        // the fourteen columns alone. A pass that learned no UIDVALIDITY
        // keeps the one on record, so a later change still shows.
        let row = MailboxRow {
            id: ids[facts.imap_name.as_str()].clone(),
            parent_id: facts
                .parent_imap_name
                .as_deref()
                .and_then(|parent| ids.get(parent).cloned()),
            facts: MailboxFacts {
                parent_imap_name: None,
                uid_validity: facts
                    .uid_validity
                    .or_else(|| recorded.and_then(|row| row.facts.uid_validity)),
                ..facts.clone()
            },
        };
        match recorded {
            Some(current) if *current == row => {}
            Some(current) => {
                if was_renumbered(current, &row) {
                    renumbered.push(row.id.clone());
                }
                upsert(transaction, key, &row)?;
                changes.push((row.id, ChangeKind::Updated));
            }
            None => {
                upsert(transaction, key, &row)?;
                changes.push((row.id, ChangeKind::Created));
            }
        }
    }
    for (id, kind) in rights::apply(transaction, key, &ids, letters)? {
        if !changes.iter().any(|(changed, _)| *changed == id) {
            changes.push((id, kind));
        }
    }
    let vanished: Vec<&MailboxId> = existing
        .iter()
        .filter(|(imap_name, _)| !ids.contains_key(imap_name.as_str()))
        .map(|(_, row)| &row.id)
        .collect();
    for id in &vanished {
        transaction.execute(
            "DELETE FROM bridge_mailboxes WHERE account_key = ?1 AND id = ?2",
            params![key.as_str(), id],
        )?;
        changes.push(((*id).clone(), ChangeKind::Destroyed));
    }
    if rights_moved {
        for id in ids.values() {
            if !changes.iter().any(|(changed, _)| changed == id) {
                changes.push((id.clone(), ChangeKind::Updated));
            }
        }
    }
    if changes.is_empty() {
        return state_of(transaction, key);
    }
    let sequence = next_sequence(transaction, key)?;
    let write = |folder| FolderWrite {
        key,
        folder,
        sequence,
    };
    for id in vanished {
        folders::vanish(transaction, &write(id))?;
    }
    for id in &renumbered {
        folders::renumber(transaction, &write(id))?;
    }
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

/// A UIDVALIDITY that differs from the one on record: every UID the
/// folder's rows hold names nothing anymore (RFC 3501 section 2.3.1.1).
fn was_renumbered(current: &MailboxRow, found: &MailboxRow) -> bool {
    matches!(
        (current.facts.uid_validity, found.facts.uid_validity),
        (Some(before), Some(now)) if before != now
    )
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
