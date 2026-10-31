// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What one `Email/set` changed on the server, written to the rows as
//! one state: the keywords a STORE gave and took, the labels a Gmail
//! STORE gave and took, the rows a move took to another folder under
//! their new UIDs and the rows whose message left. A row is written only
//! where the server found its message. A row a move took where the
//! write cannot follow it waits for the pass on a Gmail account and
//! leaves elsewhere (`gone`). On a Gmail account the Starred label
//! mailbox and `$flagged` move together (`starred`).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use rusqlite::{OptionalExtension, Transaction, params};

use super::changes::{ChangeKind, ObjectType, next_sequence, prune, state_of};
use super::emails::holds;
use super::folders::{self, Standing};
use super::gmail::{self, Labeled, Labels};
use super::keywords::{self, KeywordChange};
use super::ledger::Ledger;
use super::{AccountKey, EmailId, MailboxId, Store, StoreError};

mod gone;
mod starred;

pub use gone::GoneRow;

/// The label mailboxes a Gmail STORE gave to and took from the message
/// of one email.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelChange {
    pub id: EmailId,
    /// Where the STORE found the message; a row that moved since is
    /// left as it is.
    pub folder: MailboxId,
    pub uid: u32,
    pub add: Vec<MailboxId>,
    pub remove: Vec<MailboxId>,
}

/// One message a move took to another folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveChange {
    pub id: EmailId,
    /// Where the move found the message.
    pub from: MailboxId,
    pub from_uid: u32,
    /// Where it lies now: the folder, its UIDVALIDITY as `COPYUID` named
    /// it and the new UID.
    pub to: MailboxId,
    pub uid_validity: u32,
    pub uid: u32,
    /// The labels the server gives it there, where they were read.
    pub labels: Option<Vec<String>>,
}

/// Everything one `Email/set` changed on the server.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SetWrite {
    /// Whether the account is a Gmail account, where a row the write
    /// cannot follow waits for the pass instead of leaving.
    pub gmail: bool,
    pub keywords: Vec<KeywordChange>,
    pub labels: Vec<LabelChange>,
    pub moves: Vec<MoveChange>,
    /// The rows whose message an expunge took.
    pub gone: Vec<GoneRow>,
    /// The rows whose message a move took where the write cannot follow
    /// it.
    pub lost: Vec<GoneRow>,
}

impl SetWrite {
    /// Whether the server took nothing, so there is nothing to write.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.keywords.is_empty()
            && self.labels.is_empty()
            && self.moves.is_empty()
            && self.gone.is_empty()
            && self.lost.is_empty()
    }
}

/// An email as a write left it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    pub mailboxes: BTreeSet<MailboxId>,
    pub keywords: BTreeMap<String, bool>,
}

/// What the write left behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetWritten {
    /// The state the account stood at before the write.
    pub before: u64,
    /// The state it stands at after it; the same when no row differed.
    pub after: u64,
    /// The emails a keyword, label or move change named whose row was
    /// gone or lay elsewhere by then.
    pub stale: Vec<EmailId>,
    /// Every email a keyword, label or move change wrote, as it stands
    /// afterwards.
    pub emails: HashMap<EmailId, Written>,
}

impl Store {
    /// Writes what the server changed as one state. Each email that
    /// differs reads as updated and each that left as destroyed with its
    /// thread updated or destroyed; every mailbox whose counts moved
    /// reads as updated.
    ///
    /// # Errors
    ///
    /// Returns the database error or `Encoding` for a keywords column
    /// that is not a JSON object.
    pub fn write_set(&self, key: &AccountKey, set: &SetWrite) -> Result<SetWritten, StoreError> {
        self.write(key, |transaction| {
            let before = state_of(transaction, key)?;
            let mut writing = Writing {
                transaction,
                key,
                ledger: Ledger::default(),
                parks: set.gmail,
            };
            let (mut landed, mut stale) = (Vec::new(), Vec::new());
            for change in &set.keywords {
                if keywords::apply(transaction, key, change, &mut writing.ledger)? {
                    writing.star(&change.id, &change.folder)?;
                    landed.push(change.id.clone());
                } else {
                    stale.push(change.id.clone());
                }
            }
            for change in &set.labels {
                let list = if writing.relabel(change)? {
                    &mut landed
                } else {
                    &mut stale
                };
                list.push(change.id.clone());
            }
            for change in &set.moves {
                match writing.moved(change)? {
                    Placed::Landed => landed.push(change.id.clone()),
                    Placed::Stale => stale.push(change.id.clone()),
                    Placed::Lost => {}
                }
            }
            for gone in &set.gone {
                writing.leave(gone)?;
            }
            for lost in &set.lost {
                writing.lose(lost)?;
            }
            let ledger = writing.ledger;
            let after = if ledger.is_empty() {
                before
            } else {
                let sequence = next_sequence(transaction, key)?;
                ledger.write(transaction, key, sequence)?;
                prune(transaction, key)?;
                sequence
            };
            let mut emails = HashMap::new();
            for id in landed {
                if let Some(written) = written(transaction, key, &id)? {
                    emails.insert(id, written);
                }
            }
            Ok(SetWritten {
                before,
                after,
                stale,
                emails,
            })
        })
    }
}

/// The keywords of a row with what orders its memberships.
type Kept = (BTreeMap<String, bool>, i64);

/// Where a moved row stands after its write.
enum Placed {
    /// Where the move took it, so its state is part of the answer.
    Landed,
    /// Neither where the move found the message nor where it took it.
    Stale,
    /// The store cannot hold it where the move took it: the row is lost
    /// to the write.
    Lost,
}

/// One write in its transaction: the account and what it logs.
struct Writing<'a, 'c> {
    transaction: &'a Transaction<'c>,
    key: &'a AccountKey,
    ledger: Ledger,
    /// Whether a row the write cannot follow waits for the pass, as on a
    /// Gmail account, instead of leaving.
    parks: bool,
}

impl Writing<'_, '_> {
    /// A label change on its row; the keyword of a starred message
    /// follows the starred label mailbox. Whether the row was there.
    fn relabel(&mut self, change: &LabelChange) -> Result<bool, StoreError> {
        let place = (&change.id, &change.folder, change.uid);
        let Some(received_at) = received_at(self.transaction, self.key, place)? else {
            return Ok(false);
        };
        let current = gmail::current_memberships(self.transaction, self.key, &change.id)?;
        let mut wanted = current.clone();
        for mailbox in &change.remove {
            wanted.remove(mailbox);
        }
        wanted.extend(change.add.iter().cloned());
        let row = (&change.id, &change.folder, received_at);
        self.memberships(row, (&current, &wanted))?;
        let labels = Labels::read(self.transaction, self.key, &change.folder)?;
        if let Some(starred) = labels.starred() {
            let flagged = if change.add.contains(starred) {
                Some(true)
            } else {
                change.remove.contains(starred).then_some(false)
            };
            if let Some(flagged) = flagged {
                self.flagged(&change.id, flagged)?;
            }
        }
        Ok(true)
    }

    /// A row moved to its new folder and UID, its memberships from the
    /// folder and the labels read there. A row the move did not find
    /// where it lay still landed when it lies where the move took it, as
    /// the header sync may have put it. A folder that does not stand
    /// under the UIDVALIDITY of `COPYUID` or holds another row under the
    /// new UID cannot take it: the row is lost to the write.
    fn moved(&mut self, change: &MoveChange) -> Result<Placed, StoreError> {
        let (transaction, key) = (self.transaction, self.key);
        let place = (&change.id, &change.from, change.from_uid);
        let Some(received_at) = received_at(transaction, key, place)? else {
            let there = (&change.id, &change.to, change.uid);
            return Ok(if received_at(transaction, key, there)?.is_some() {
                Placed::Landed
            } else {
                Placed::Stale
            });
        };
        let standing = Standing {
            folder: &change.to,
            uid_validity: change.uid_validity,
        };
        if !folders::pinned(transaction, key, &standing)?
            || holds(transaction, key, &change.to, change.uid)?
        {
            let lost = GoneRow {
                id: change.id.clone(),
                folder: change.from.clone(),
                uid: change.from_uid,
            };
            self.lose(&lost)?;
            return Ok(Placed::Lost);
        }
        transaction.execute(
            "UPDATE bridge_emails SET folder_id = ?3, uid = ?4 WHERE account_key = ?1 AND id = ?2",
            params![key.as_str(), change.id, change.to, change.uid],
        )?;
        let labels = Labels::read(transaction, key, &change.to)?;
        let read = change.labels.as_deref().unwrap_or_default();
        let wanted = labels.memberships(&change.to, read);
        let current = gmail::current_memberships(transaction, key, &change.id)?;
        self.memberships((&change.id, &change.to, received_at), (&current, &wanted))?;
        for folder in [&change.from, &change.to] {
            let mailbox = folder.as_str();
            self.ledger
                .note(ObjectType::Mailbox, mailbox, ChangeKind::Updated);
        }
        let email = change.id.as_str();
        self.ledger
            .note(ObjectType::Email, email, ChangeKind::Updated);
        Ok(Placed::Landed)
    }

    /// The memberships of a row brought to `wanted`; when any moved the
    /// email reads as updated and so does every mailbox whose counts
    /// moved.
    fn memberships(
        &mut self,
        (id, folder, received_at): (&EmailId, &MailboxId, i64),
        (current, wanted): (&BTreeSet<MailboxId>, &BTreeSet<MailboxId>),
    ) -> Result<(), StoreError> {
        if current == wanted {
            return Ok(());
        }
        let row = Labeled {
            id,
            folder,
            labels: &[],
            received_at,
        };
        gmail::replace_memberships(self.transaction, self.key, &row, wanted)?;
        for mailbox in current.symmetric_difference(wanted) {
            let mailbox = mailbox.as_str();
            self.ledger
                .note(ObjectType::Mailbox, mailbox, ChangeKind::Updated);
        }
        self.ledger
            .note(ObjectType::Email, id.as_str(), ChangeKind::Updated);
        Ok(())
    }
}

/// What orders the memberships of the row at that place; `None` when no
/// row of that id lies there.
fn received_at(
    transaction: &Transaction<'_>,
    key: &AccountKey,
    (id, folder, uid): (&EmailId, &MailboxId, u32),
) -> Result<Option<i64>, StoreError> {
    Ok(transaction
        .query_row(
            "SELECT received_at FROM bridge_emails
             WHERE account_key = ?1 AND id = ?2 AND folder_id = ?3 AND uid = ?4",
            params![key.as_str(), id, folder, uid],
            |row| row.get(0),
        )
        .optional()?)
}

/// The keywords of a row with what orders its memberships.
fn row_of(
    transaction: &Transaction<'_>,
    key: &AccountKey,
    id: &EmailId,
) -> Result<Option<Kept>, StoreError> {
    let found: Option<(String, i64)> = transaction
        .query_row(
            "SELECT keywords, received_at FROM bridge_emails WHERE account_key = ?1 AND id = ?2",
            params![key.as_str(), id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    found
        .map(|(keywords, received_at)| Ok((serde_json::from_str(&keywords)?, received_at)))
        .transpose()
}

/// An email as the write left it; `None` when it left.
fn written(
    transaction: &Transaction<'_>,
    key: &AccountKey,
    id: &EmailId,
) -> Result<Option<Written>, StoreError> {
    let Some((keywords, _)) = row_of(transaction, key, id)? else {
        return Ok(None);
    };
    Ok(Some(Written {
        mailboxes: gmail::current_memberships(transaction, key, id)?,
        keywords,
    }))
}
