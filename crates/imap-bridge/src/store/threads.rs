// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Threads across mailboxes as a union-find over the keyed hashes of
//! message ids: every id a header names points at one thread and a
//! message that names two threads joins them (the REFERENCES algorithm
//! of RFC 5256 section 3 without its subject step).

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use super::changes::{ChangeKind, ObjectType, state_of};
use super::ledger::Ledger;
use super::{AccountKey, EmailId, Store, StoreError, ThreadId};

/// The threads a `/get` named with their emails, read under one lock so
/// the state belongs to them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadSnapshot {
    pub state: u64,
    /// Each thread that holds an email, oldest email first.
    pub threads: Vec<(ThreadId, Vec<EmailId>)>,
}

impl Store {
    /// The threads among `ids` the account holds, in the order asked,
    /// their emails by `receivedAt` then id (RFC 8621 section 3).
    ///
    /// # Errors
    ///
    /// Returns the database error.
    pub fn threads(&self, key: &AccountKey, ids: &[&str]) -> Result<ThreadSnapshot, StoreError> {
        self.read(|connection| {
            let mut threads = Vec::new();
            for id in ids {
                let emails = emails_of(connection, key, id)?;
                if !emails.is_empty() {
                    threads.push((ThreadId::from((*id).to_owned()), emails));
                }
            }
            Ok(ThreadSnapshot {
                state: state_of(connection, key)?,
                threads,
            })
        })
    }
}

fn emails_of(
    connection: &Connection,
    key: &AccountKey,
    thread: &str,
) -> Result<Vec<EmailId>, StoreError> {
    let mut statement = connection.prepare_cached(
        "SELECT id FROM bridge_emails WHERE account_key = ?1 AND thread_id = ?2
         ORDER BY received_at, id",
    )?;
    let emails = statement
        .query_map(params![key.as_str(), thread], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(emails)
}

/// The thread of an email whose header names `hashes`, before its row is
/// written. Threads the hashes tell apart merge into the one that holds
/// the most emails, so a merge moves the fewer rows; with no thread
/// named the email starts one. Every hash points at the thread
/// afterwards.
pub(super) fn join(
    transaction: &Transaction<'_>,
    key: &AccountKey,
    hashes: &[Vec<u8>],
    ledger: &mut Ledger,
) -> Result<ThreadId, StoreError> {
    let mut named: Vec<(u64, ThreadId)> = Vec::new();
    for hash in hashes {
        if let Some(thread) = thread_of(transaction, key, hash)?
            && !named.iter().any(|(_, known)| *known == thread)
        {
            named.push((size(transaction, key, &thread)?, thread));
        }
    }
    // The most emails first, the smaller id on a tie.
    named.sort_by(|(a_size, a), (b_size, b)| b_size.cmp(a_size).then_with(|| a.cmp(b)));
    let mut named = named.into_iter();
    let (held, thread) = named.next().unwrap_or_else(|| (0, ThreadId::generate()));
    let mut merged = false;
    for (_, loser) in named {
        merge(transaction, key, (&loser, &thread), ledger)?;
        merged = true;
    }
    let kind = if held == 0 && !merged {
        ChangeKind::Created
    } else {
        ChangeKind::Updated
    };
    ledger.note(ObjectType::Thread, thread.as_str(), kind);
    for hash in hashes {
        transaction.execute(
            "INSERT INTO bridge_message_ids (account_key, message_id_hash, thread_id)
             VALUES (?1, ?2, ?3)
             ON CONFLICT (account_key, message_id_hash) DO UPDATE SET thread_id = excluded.thread_id",
            params![key.as_str(), hash, thread],
        )?;
    }
    Ok(thread)
}

fn thread_of(
    connection: &Connection,
    key: &AccountKey,
    hash: &[u8],
) -> Result<Option<ThreadId>, StoreError> {
    Ok(connection
        .query_row(
            "SELECT thread_id FROM bridge_message_ids
             WHERE account_key = ?1 AND message_id_hash = ?2",
            params![key.as_str(), hash],
            |row| row.get(0),
        )
        .optional()?)
}

/// The emails a thread holds.
fn size(connection: &Connection, key: &AccountKey, thread: &ThreadId) -> Result<u64, StoreError> {
    Ok(connection.query_row(
        "SELECT COUNT(*) FROM bridge_emails WHERE account_key = ?1 AND thread_id = ?2",
        params![key.as_str(), thread],
        |row| row.get(0),
    )?)
}

/// Moves the emails and the hashes of `loser` to `survivor`: the thread
/// that loses is destroyed and each of its emails updated.
fn merge(
    transaction: &Transaction<'_>,
    key: &AccountKey,
    (loser, survivor): (&ThreadId, &ThreadId),
    ledger: &mut Ledger,
) -> Result<(), StoreError> {
    for email in emails_of(transaction, key, loser.as_str())? {
        ledger.note(ObjectType::Email, email.as_str(), ChangeKind::Updated);
    }
    for table in ["bridge_emails", "bridge_message_ids"] {
        transaction.execute(
            &format!("UPDATE {table} SET thread_id = ?3 WHERE account_key = ?1 AND thread_id = ?2"),
            params![key.as_str(), loser, survivor],
        )?;
    }
    ledger.note(ObjectType::Thread, loser.as_str(), ChangeKind::Destroyed);
    Ok(())
}

/// An email left `thread`: the thread is destroyed with its hashes when
/// it holds none anymore and updated otherwise.
pub(super) fn left(
    transaction: &Transaction<'_>,
    key: &AccountKey,
    thread: &ThreadId,
    ledger: &mut Ledger,
) -> Result<(), StoreError> {
    if size(transaction, key, thread)? > 0 {
        ledger.note(ObjectType::Thread, thread.as_str(), ChangeKind::Updated);
        return Ok(());
    }
    ledger.note(ObjectType::Thread, thread.as_str(), ChangeKind::Destroyed);
    transaction.execute(
        "DELETE FROM bridge_message_ids WHERE account_key = ?1 AND thread_id = ?2",
        params![key.as_str(), thread],
    )?;
    Ok(())
}
