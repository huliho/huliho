// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What the Gmail label model adds to the rows: one row per X-GM-MSGID
//! across the three stores, memberships from the labels of a row in
//! All Mail and rows that wait for the pass once their UID left a store.

use std::collections::{BTreeSet, HashMap};

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use super::changes::{ChangeKind, ObjectType, log, next_sequence, prune, state_of};
use super::emails::EmailFacts;
use super::folders::{self, FolderWrite, Leaving, Standing};
use super::ledger::Ledger;
use super::threads;
use super::{AccountKey, EmailId, MailboxId, Store, StoreError, ThreadId};
use crate::gmail::ALL_MAIL_ROLE;
use crate::seal::Sealer;

/// The three items of one message on a Gmail account, in the bridge's
/// spelling, each label once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GmailFacts {
    pub labels: Vec<String>,
    pub msgid: u64,
    pub thrid: u64,
}

impl GmailFacts {
    /// The thread the server names: `t` and the decimal id.
    fn thread(&self) -> ThreadId {
        ThreadId::from(format!("t{}", self.thrid))
    }

    /// The id as the column holds it: the signed value of the same bits,
    /// since the column is INTEGER and the id unsigned.
    fn stored_msgid(&self) -> i64 {
        i64::from_ne_bytes(self.msgid.to_ne_bytes())
    }
}

/// The label mailboxes of an account by their label, and whether the
/// folder written to shows the labels of its rows: All Mail does, Spam
/// and Trash do not.
pub(super) struct Labels {
    key: AccountKey,
    by_label: HashMap<String, MailboxId>,
    shown: bool,
}

/// One row of a store folder with the labels the server gives it now.
pub(super) struct Labeled<'a> {
    pub id: &'a EmailId,
    pub folder: &'a MailboxId,
    pub labels: &'a [String],
    /// Orders the memberships inside their windows.
    pub received_at: i64,
}

impl Labels {
    pub(super) fn read(
        connection: &Connection,
        key: &AccountKey,
        folder: &MailboxId,
    ) -> Result<Self, StoreError> {
        let mut statement = connection.prepare_cached(
            "SELECT gmail_label, id FROM bridge_mailboxes
             WHERE account_key = ?1 AND gmail_label IS NOT NULL",
        )?;
        let by_label = statement
            .query_map([key.as_str()], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<HashMap<_, _>, _>>()?;
        let role: Option<Option<String>> = connection
            .query_row(
                "SELECT role FROM bridge_mailboxes WHERE account_key = ?1 AND id = ?2 AND store = 1",
                params![key.as_str(), folder],
                |row| row.get(0),
            )
            .optional()?;
        Ok(Self {
            key: key.clone(),
            by_label,
            shown: role.flatten().as_deref() == Some(ALL_MAIL_ROLE),
        })
    }

    /// The mailboxes a row of `folder` with these labels is a member of.
    fn memberships(&self, folder: &MailboxId, labels: &[String]) -> BTreeSet<MailboxId> {
        let mut found = BTreeSet::from([folder.clone()]);
        if self.shown {
            found.extend(
                labels
                    .iter()
                    .filter_map(|label| self.by_label.get(label).cloned()),
            );
        }
        found
    }

    /// Brings the memberships of a row to what its labels say. When any
    /// moved the email reads as updated and so does every mailbox whose
    /// counts moved, the folder excepted: its line is the caller's.
    pub(super) fn relabel(
        &self,
        transaction: &Transaction<'_>,
        row: &Labeled<'_>,
        ledger: &mut Ledger,
    ) -> Result<(), StoreError> {
        let current = current_memberships(transaction, &self.key, row.id)?;
        let wanted = self.memberships(row.folder, row.labels);
        if current == wanted {
            return Ok(());
        }
        replace_memberships(transaction, &self.key, row, &wanted)?;
        note_mailboxes(ledger, current.symmetric_difference(&wanted), row.folder);
        ledger.note(ObjectType::Email, row.id.as_str(), ChangeKind::Updated);
        Ok(())
    }
}

/// One message of a Gmail account on its way into the rows.
pub(super) struct Arrival<'a> {
    pub key: &'a AccountKey,
    pub folder: &'a MailboxId,
    pub facts: &'a EmailFacts,
    pub gmail: &'a GmailFacts,
    /// The keyed hash of the Message-ID, kept as every row keeps it.
    pub hash: Option<&'a [u8]>,
    pub labels: &'a Labels,
}

/// Where a message with an id already lives.
struct Held {
    id: EmailId,
    folder: MailboxId,
    uid: i64,
    thread: ThreadId,
}

impl Arrival<'_> {
    /// Writes the message: a new row in the thread the server names, or
    /// the row that holds its id moved to where the message lies now,
    /// the memberships from its labels either way. `None` when the same
    /// folder holds another live UID under the id, so two rows never
    /// chase one another. Every mailbox whose counts moved is noted, the
    /// folder excepted.
    pub(super) fn write(
        &self,
        transaction: &Transaction<'_>,
        sealer: &dyn Sealer,
        ledger: &mut Ledger,
    ) -> Result<Option<(EmailId, ChangeKind)>, StoreError> {
        let key = self.key;
        let found = held(transaction, key, self.gmail.stored_msgid())?;
        let (id, kind) = match &found {
            Some(row)
                if row.folder == *self.folder
                    && row.uid > 0
                    && row.uid != i64::from(self.facts.uid) =>
            {
                return Ok(None);
            }
            Some(row) => (row.id.clone(), ChangeKind::Updated),
            None => (EmailId::generate(), ChangeKind::Created),
        };
        let thread = self.gmail.thread();
        // A row that keeps its thread moves nothing inside it.
        let previous = found
            .map(|row| row.thread)
            .filter(|previous| *previous != thread);
        if kind == ChangeKind::Created || previous.is_some() {
            threads::claim(transaction, key, &thread, ledger)?;
        }
        let plain = serde_json::to_vec(&self.facts.personal)?;
        let blob = sealer.seal(key, &id, &plain)?;
        self.upsert(transaction, (&id, &thread), &blob)?;
        if let Some(previous) = previous {
            threads::left(transaction, key, &previous, ledger)?;
        }
        let row = Labeled {
            id: &id,
            folder: self.folder,
            labels: &self.gmail.labels,
            received_at: self.facts.received_at,
        };
        let current = current_memberships(transaction, key, &id)?;
        let wanted = self.labels.memberships(self.folder, &self.gmail.labels);
        replace_memberships(transaction, key, &row, &wanted)?;
        note_mailboxes(ledger, current.symmetric_difference(&wanted), self.folder);
        Ok(Some((id, kind)))
    }

    fn upsert(
        &self,
        transaction: &Transaction<'_>,
        (id, thread): (&EmailId, &ThreadId),
        sealed: &[u8],
    ) -> Result<(), StoreError> {
        let facts = self.facts;
        let key = self.key;
        transaction.execute(
            "INSERT INTO bridge_emails
             (account_key, id, folder_id, uid, thread_id, keywords, size, received_at, sent_at,
              has_attachment, message_id_hash, gmail_msgid, sealed)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT (account_key, id) DO UPDATE SET
               folder_id = excluded.folder_id, uid = excluded.uid,
               thread_id = excluded.thread_id, keywords = excluded.keywords,
               size = excluded.size, received_at = excluded.received_at,
               sent_at = excluded.sent_at, has_attachment = excluded.has_attachment,
               message_id_hash = excluded.message_id_hash, sealed = excluded.sealed",
            params![
                key.as_str(),
                id,
                self.folder,
                facts.uid,
                thread,
                serde_json::to_string(&facts.keywords)?,
                facts.size,
                facts.received_at,
                facts.sent_at,
                facts.has_attachment,
                self.hash,
                self.gmail.stored_msgid(),
                sealed
            ],
        )?;
        Ok(())
    }
}

fn held(connection: &Connection, key: &AccountKey, msgid: i64) -> Result<Option<Held>, StoreError> {
    Ok(connection
        .query_row(
            "SELECT id, folder_id, uid, thread_id FROM bridge_emails
             WHERE account_key = ?1 AND gmail_msgid = ?2",
            params![key.as_str(), msgid],
            |row| {
                Ok(Held {
                    id: row.get(0)?,
                    folder: row.get(1)?,
                    uid: row.get(2)?,
                    thread: row.get(3)?,
                })
            },
        )
        .optional()?)
}

/// The row of a folder under a UID, with what orders its memberships.
pub(super) fn row_at(
    connection: &Connection,
    key: &AccountKey,
    folder: &MailboxId,
    uid: u32,
) -> Result<Option<(EmailId, i64)>, StoreError> {
    Ok(connection
        .query_row(
            "SELECT id, received_at FROM bridge_emails
             WHERE account_key = ?1 AND folder_id = ?2 AND uid = ?3",
            params![key.as_str(), folder, uid],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?)
}

fn current_memberships(
    connection: &Connection,
    key: &AccountKey,
    email: &EmailId,
) -> Result<BTreeSet<MailboxId>, StoreError> {
    let mut statement = connection.prepare_cached(
        "SELECT mailbox_id FROM bridge_memberships WHERE account_key = ?1 AND email_id = ?2",
    )?;
    let found = statement
        .query_map(params![key.as_str(), email], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(found)
}

fn replace_memberships(
    transaction: &Transaction<'_>,
    key: &AccountKey,
    row: &Labeled<'_>,
    wanted: &BTreeSet<MailboxId>,
) -> Result<(), StoreError> {
    transaction.execute(
        "DELETE FROM bridge_memberships WHERE account_key = ?1 AND email_id = ?2",
        params![key.as_str(), row.id],
    )?;
    for mailbox in wanted {
        transaction.execute(
            "INSERT INTO bridge_memberships (account_key, email_id, mailbox_id, received_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![key.as_str(), row.id, mailbox, row.received_at],
        )?;
    }
    Ok(())
}

/// Notes every mailbox whose counts moved, the folder of the write
/// excepted: its line is the caller's, so no mailbox is logged twice.
fn note_mailboxes<'a>(
    ledger: &mut Ledger,
    touched: impl Iterator<Item = &'a MailboxId>,
    folder: &MailboxId,
) {
    for mailbox in touched.filter(|mailbox| *mailbox != folder) {
        ledger.note(ObjectType::Mailbox, mailbox.as_str(), ChangeKind::Updated);
    }
}

impl Store {
    /// Parks the messages of these UIDs: their rows wait under a negative
    /// UID for a store that holds their id, as the rows of a renumbered
    /// folder wait for their match. Nothing is logged, since nothing a
    /// client sees has moved yet. `None` when the folder does not stand
    /// anymore.
    ///
    /// # Errors
    ///
    /// Returns the database error.
    pub fn park_uids(
        &self,
        key: &AccountKey,
        standing: &Standing<'_>,
        uids: &[u32],
    ) -> Result<Option<()>, StoreError> {
        self.write(|transaction| {
            if !folders::stands(transaction, key, standing)? {
                return Ok(None);
            }
            for uid in uids {
                transaction.execute(
                    "UPDATE bridge_emails SET uid = -uid
                     WHERE account_key = ?1 AND folder_id = ?2 AND uid = ?3",
                    params![key.as_str(), standing.folder, uid],
                )?;
            }
            Ok(Some(()))
        })
    }

    /// Destroys the rows that still wait in every store whose first sync
    /// is done, as one state; each such store and every mailbox the
    /// rows were members of read as updated. Answers the state
    /// afterwards.
    ///
    /// # Errors
    ///
    /// Returns the database error.
    pub fn sweep_parked(&self, key: &AccountKey) -> Result<u64, StoreError> {
        self.write(|transaction| {
            let mut statement = transaction.prepare(
                "SELECT s.folder_id FROM bridge_sync s
                 WHERE s.account_key = ?1 AND s.done = 1 AND EXISTS (
                     SELECT 1 FROM bridge_emails e
                     WHERE e.account_key = s.account_key AND e.folder_id = s.folder_id
                       AND e.uid < 0)",
            )?;
            let waiting: Vec<MailboxId> = statement
                .query_map([key.as_str()], |row| row.get(0))?
                .collect::<Result<_, _>>()?;
            drop(statement);
            if waiting.is_empty() {
                return state_of(transaction, key);
            }
            let sequence = next_sequence(transaction, key)?;
            for folder in &waiting {
                let write = FolderWrite {
                    key,
                    folder,
                    sequence,
                };
                folders::leave(transaction, &write, Leaving::Unmatched)?;
                let mailbox = (folder.as_str(), ChangeKind::Updated);
                log(transaction, key, (sequence, ObjectType::Mailbox), mailbox)?;
            }
            prune(transaction, key)?;
            Ok(sequence)
        })
    }
}
