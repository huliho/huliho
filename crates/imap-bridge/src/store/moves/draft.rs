// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The row of a draft the server took: written at once where `APPENDUID`
//! names its UID, in the folder it lies in with the headers the bridge
//! wrote and its thread from its references, as the sync threads a
//! fetched message. Elsewhere a pass lands it and it is found by the keyed
//! hash of the Message-ID the bridge wrote.

use rusqlite::{OptionalExtension, Row, params};

use super::super::changes::{ChangeKind, ObjectType};
use super::super::emails::{self, EmailFacts, holds};
use super::super::folders::{self, Standing};
use super::super::{AccountKey, EmailId, MailboxId, Store, StoreError, ThreadId, threads};
use super::Writing;

/// A draft the server took under the UID `APPENDUID` named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftRow {
    pub id: EmailId,
    pub folder: MailboxId,
    /// The UIDVALIDITY `APPENDUID` named.
    pub uid_validity: u32,
    /// The facts of the row, its UID among them.
    pub facts: EmailFacts,
    /// The keyed hash of its Message-ID.
    pub hash: Vec<u8>,
    /// The keyed hashes of every id its header names, each once.
    pub named: Vec<Vec<u8>>,
    pub sealed: Vec<u8>,
}

/// What the rows hold of a draft: what `created` answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Landed {
    pub id: EmailId,
    pub thread: ThreadId,
    pub size: u32,
}

impl Writing<'_, '_> {
    /// The row of a draft, a new one in its thread with its one
    /// membership. A folder that holds no UIDVALIDITY yet takes the one
    /// `APPENDUID` named. Whether it went in: a folder renumbered since or
    /// one that holds another row under the UID leaves the draft to a
    /// pass.
    pub(super) fn draft(&mut self, draft: &DraftRow) -> Result<bool, StoreError> {
        let (transaction, key) = (self.transaction, self.key);
        let standing = Standing {
            folder: &draft.folder,
            uid_validity: draft.uid_validity,
        };
        if !folders::pinned(transaction, key, &standing)?
            || holds(transaction, key, &draft.folder, draft.facts.uid)?
        {
            return Ok(false);
        }
        let thread = threads::join(transaction, key, &draft.named, &mut self.ledger)?;
        let row = emails::Written {
            id: &draft.id,
            folder: &draft.folder,
            hash: Some(&draft.hash),
            sealed: &draft.sealed,
        };
        emails::insert(transaction, key, &draft.facts, (&row, &thread))?;
        let (email, mailbox) = (draft.id.as_str(), draft.folder.as_str());
        self.ledger
            .note(ObjectType::Email, email, ChangeKind::Created);
        self.ledger
            .note(ObjectType::Mailbox, mailbox, ChangeKind::Updated);
        Ok(true)
    }
}

fn landed_row(row: &Row<'_>) -> rusqlite::Result<Landed> {
    Ok(Landed {
        id: row.get(0)?,
        thread: row.get(1)?,
        size: row.get(2)?,
    })
}

impl Store {
    /// The row a pass landed for a Message-ID, by its keyed hash; `None`
    /// while no row holds it. No index holds the hash, so this reads every
    /// row of the account: a row the write itself put in goes by its id.
    ///
    /// # Errors
    ///
    /// Returns the database error.
    pub fn landed(&self, key: &AccountKey, hash: &[u8]) -> Result<Option<Landed>, StoreError> {
        self.read(|connection| {
            Ok(connection
                .query_row(
                    "SELECT id, thread_id, size FROM bridge_emails
                     WHERE account_key = ?1 AND message_id_hash = ?2 AND uid > 0",
                    params![key.as_str(), hash],
                    landed_row,
                )
                .optional()?)
        })
    }

    /// The row a write put in, by its id.
    ///
    /// # Errors
    ///
    /// Returns the database error.
    pub fn draft_written(
        &self,
        key: &AccountKey,
        id: &EmailId,
    ) -> Result<Option<Landed>, StoreError> {
        self.read(|connection| {
            Ok(connection
                .query_row(
                    "SELECT id, thread_id, size FROM bridge_emails
                     WHERE account_key = ?1 AND id = ?2",
                    params![key.as_str(), id],
                    landed_row,
                )
                .optional()?)
        })
    }
}
