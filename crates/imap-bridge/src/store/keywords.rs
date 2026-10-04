// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The keywords an `Email/set` writes: where the message of an email
//! lies with the keywords its row holds, and the write that keeps what
//! a STORE changed on the server as one state.

use std::collections::BTreeMap;

use rusqlite::{OptionalExtension, Transaction, params};

use super::changes::{ChangeKind, ObjectType, next_sequence, prune, state_of};
use super::ledger::Ledger;
use super::{AccountKey, EmailId, MailboxId, Store, StoreError};

/// An email as a keyword write finds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeywordRow {
    pub id: EmailId,
    /// The store folder its message lies in.
    pub folder: MailboxId,
    /// `None` while the row waits for a match after a renumbering, when
    /// no UID names its message.
    pub uid: Option<u32>,
    pub keywords: BTreeMap<String, bool>,
}

/// The keywords a STORE gave to and took from the message of one email.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeywordChange {
    pub id: EmailId,
    /// Where the STORE found the message; a row that moved since is
    /// left as it is.
    pub folder: MailboxId,
    pub uid: u32,
    pub add: Vec<String>,
    pub remove: Vec<String>,
}

/// What a keyword write left behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeywordsWritten {
    /// The state the account stood at before the write.
    pub before: u64,
    /// The state it stands at after it; the same when no row differed.
    pub after: u64,
    /// The emails whose row was gone or lay elsewhere by then.
    pub gone: Vec<EmailId>,
}

impl Store {
    /// The rows among `ids` the account holds, with their keywords.
    ///
    /// # Errors
    ///
    /// Returns the database error or `Encoding` for a keywords column
    /// that is not a JSON object.
    pub fn keyword_rows(
        &self,
        key: &AccountKey,
        ids: &[&str],
    ) -> Result<(u64, Vec<KeywordRow>), StoreError> {
        self.read(|connection| {
            let mut statement = connection.prepare_cached(
                "SELECT id, folder_id, uid, keywords FROM bridge_emails
                 WHERE account_key = ?1 AND id = ?2",
            )?;
            let mut rows = Vec::new();
            for id in ids {
                let found: Option<(EmailId, MailboxId, i64, String)> = statement
                    .query_row(params![key.as_str(), id], |row| {
                        Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                    })
                    .optional()?;
                if let Some((id, folder, uid, keywords)) = found {
                    rows.push(KeywordRow {
                        id,
                        folder,
                        uid: u32::try_from(uid).ok().filter(|uid| *uid > 0),
                        keywords: serde_json::from_str(&keywords)?,
                    });
                }
            }
            Ok((state_of(connection, key)?, rows))
        })
    }

    /// Writes the keywords as one state: each row that differs is
    /// updated, its email logged as updated and every mailbox it is a
    /// member of as well, since its counts moved.
    ///
    /// # Errors
    ///
    /// Returns the database error.
    pub fn write_keywords(
        &self,
        key: &AccountKey,
        changes: &[KeywordChange],
    ) -> Result<KeywordsWritten, StoreError> {
        self.write(key, |transaction| {
            let before = state_of(transaction, key)?;
            let mut ledger = Ledger::default();
            let mut gone = Vec::new();
            for change in changes {
                if !apply(transaction, key, change, &mut ledger)? {
                    gone.push(change.id.clone());
                }
            }
            let after = if ledger.is_empty() {
                before
            } else {
                let sequence = next_sequence(transaction, key)?;
                ledger.write(transaction, key, sequence)?;
                prune(transaction, key)?;
                sequence
            };
            Ok(KeywordsWritten {
                before,
                after,
                gone,
            })
        })
    }
}

/// One change on its row; whether the row was there.
fn apply(
    transaction: &Transaction<'_>,
    key: &AccountKey,
    change: &KeywordChange,
    ledger: &mut Ledger,
) -> Result<bool, StoreError> {
    let place = params![key.as_str(), change.id, change.folder, change.uid];
    let stored: Option<String> = transaction
        .query_row(
            "SELECT keywords FROM bridge_emails
             WHERE account_key = ?1 AND id = ?2 AND folder_id = ?3 AND uid = ?4",
            place,
            |row| row.get(0),
        )
        .optional()?;
    let Some(stored) = stored else {
        return Ok(false);
    };
    let mut keywords: BTreeMap<String, bool> = serde_json::from_str(&stored)?;
    for keyword in &change.remove {
        keywords.remove(keyword);
    }
    for keyword in &change.add {
        keywords.insert(keyword.clone(), true);
    }
    let written = serde_json::to_string(&keywords)?;
    if written == stored {
        return Ok(true);
    }
    transaction.execute(
        "UPDATE bridge_emails SET keywords = ?3 WHERE account_key = ?1 AND id = ?2",
        params![key.as_str(), change.id, written],
    )?;
    ledger.note(ObjectType::Email, change.id.as_str(), ChangeKind::Updated);
    let mut statement = transaction.prepare_cached(
        "SELECT mailbox_id FROM bridge_memberships WHERE account_key = ?1 AND email_id = ?2",
    )?;
    let mailboxes: Vec<MailboxId> = statement
        .query_map(params![key.as_str(), change.id], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    for mailbox in mailboxes {
        ledger.note(ObjectType::Mailbox, mailbox.as_str(), ChangeKind::Updated);
    }
    Ok(true)
}
