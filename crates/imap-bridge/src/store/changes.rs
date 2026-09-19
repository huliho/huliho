// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The per-account state counter and the change log every `/changes`
//! method reads (RFC 8620 section 5.2).

use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ValueRef};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use super::{AccountKey, Store, StoreError};

/// Rows the log keeps per account at least; a state older than the
/// oldest kept sequence cannot be calculated from.
pub const CHANGES_HORIZON: u64 = 10_000;

/// The object types the log distinguishes; each has a `/changes`
/// method of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectType {
    Mailbox,
    Email,
    Thread,
}

impl ObjectType {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Mailbox => "Mailbox",
            Self::Email => "Email",
            Self::Thread => "Thread",
        }
    }
}

/// What happened to one object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Created,
    Updated,
    Destroyed,
}

impl ChangeKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Updated => "updated",
            Self::Destroyed => "destroyed",
        }
    }
}

impl FromSql for ChangeKind {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        match value.as_str()? {
            "created" => Ok(Self::Created),
            "updated" => Ok(Self::Updated),
            "destroyed" => Ok(Self::Destroyed),
            other => Err(FromSqlError::Other(
                format!("unknown change kind {other}").into(),
            )),
        }
    }
}

/// One row of the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub sequence: u64,
    pub id: String,
    pub kind: ChangeKind,
}

/// The log read from a state: the current state and the rows after the
/// given one, oldest first; `None` when the log has forgotten rows the
/// answer would need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangesSince {
    pub state: u64,
    pub changes: Option<Vec<Change>>,
}

impl Store {
    /// The account's state counter; zero before the first change.
    ///
    /// # Errors
    ///
    /// Returns the database error.
    pub fn state(&self, key: &AccountKey) -> Result<u64, StoreError> {
        self.read(|connection| state_of(connection, key))
    }

    /// Every change of `object` after `since`, with the current state.
    ///
    /// # Errors
    ///
    /// Returns the database error.
    pub fn changes_since(
        &self,
        key: &AccountKey,
        object: ObjectType,
        since: u64,
    ) -> Result<ChangesSince, StoreError> {
        self.read(|connection| {
            let state = state_of(connection, key)?;
            let changes = match oldest_kept(connection, key)? {
                _ if since > state => None,
                None if since == state => Some(Vec::new()),
                None => None,
                Some(oldest) if since + 1 < oldest => None,
                Some(_) => Some(read_changes(connection, key, object, since)?),
            };
            Ok(ChangesSince { state, changes })
        })
    }
}

pub(crate) fn state_of(connection: &Connection, key: &AccountKey) -> Result<u64, StoreError> {
    let sequence: Option<u64> = connection
        .query_row(
            "SELECT sequence FROM bridge_state WHERE account_key = ?1",
            [key.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    Ok(sequence.unwrap_or(0))
}

fn oldest_kept(connection: &Connection, key: &AccountKey) -> Result<Option<u64>, StoreError> {
    Ok(connection.query_row(
        "SELECT MIN(sequence) FROM bridge_changes WHERE account_key = ?1",
        [key.as_str()],
        |row| row.get(0),
    )?)
}

fn read_changes(
    connection: &Connection,
    key: &AccountKey,
    object: ObjectType,
    since: u64,
) -> Result<Vec<Change>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT sequence, id, kind FROM bridge_changes
         WHERE account_key = ?1 AND type = ?2 AND sequence > ?3
         ORDER BY sequence, id",
    )?;
    let rows = statement
        .query_map(params![key.as_str(), object.as_str(), since], |row| {
            Ok(Change {
                sequence: row.get(0)?,
                id: row.get(1)?,
                kind: row.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Opens the next state for a batch of changes: the counter moves by
/// one and the new sequence comes back for the rows.
pub(crate) fn next_sequence(
    transaction: &Transaction<'_>,
    key: &AccountKey,
) -> Result<u64, StoreError> {
    let sequence = transaction.query_row(
        "INSERT INTO bridge_state (account_key, sequence) VALUES (?1, 1)
         ON CONFLICT (account_key) DO UPDATE SET sequence = sequence + 1
         RETURNING sequence",
        [key.as_str()],
        |row| row.get(0),
    )?;
    Ok(sequence)
}

/// Writes one change under `sequence`.
pub(crate) fn log(
    transaction: &Transaction<'_>,
    key: &AccountKey,
    entry: (u64, ObjectType),
    change: (&str, ChangeKind),
) -> Result<(), StoreError> {
    let (sequence, object) = entry;
    let (id, kind) = change;
    transaction.execute(
        "INSERT INTO bridge_changes (account_key, sequence, type, id, kind)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![key.as_str(), sequence, object.as_str(), id, kind.as_str()],
    )?;
    Ok(())
}

/// Forgets the sequences below the newest `CHANGES_HORIZON` rows; a
/// sequence leaves whole, so every kept state is complete.
pub(crate) fn prune(transaction: &Transaction<'_>, key: &AccountKey) -> Result<(), StoreError> {
    transaction.execute(
        "DELETE FROM bridge_changes WHERE account_key = ?1 AND sequence < (
             SELECT MIN(sequence) FROM (
                 SELECT sequence FROM bridge_changes WHERE account_key = ?1
                 ORDER BY sequence DESC LIMIT ?2))",
        params![key.as_str(), CHANGES_HORIZON],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> AccountKey {
        AccountKey::new("a1")
    }

    /// One state with `count` mailbox rows created under it.
    fn batch(store: &Store, count: u64) -> u64 {
        store
            .write(|transaction| {
                let sequence = next_sequence(transaction, &key())?;
                for index in 0..count {
                    let id = format!("m{sequence}-{index}");
                    log(
                        transaction,
                        &key(),
                        (sequence, ObjectType::Mailbox),
                        (&id, ChangeKind::Created),
                    )?;
                }
                prune(transaction, &key())?;
                Ok(sequence)
            })
            .unwrap()
    }

    #[test]
    fn a_fresh_account_is_at_state_zero_and_calculates_from_it() {
        let store = Store::in_memory().unwrap();
        assert_eq!(store.state(&key()).unwrap(), 0);
        let since = store.changes_since(&key(), ObjectType::Mailbox, 0).unwrap();
        assert_eq!(since.state, 0);
        assert_eq!(since.changes, Some(Vec::new()));
        let ahead = store.changes_since(&key(), ObjectType::Mailbox, 1).unwrap();
        assert_eq!(ahead.changes, None);
    }

    #[test]
    fn every_batch_is_one_state_and_the_log_reads_from_any_kept_one() {
        let store = Store::in_memory().unwrap();
        assert_eq!(batch(&store, 2), 1);
        assert_eq!(batch(&store, 1), 2);
        assert_eq!(store.state(&key()).unwrap(), 2);
        let since = store.changes_since(&key(), ObjectType::Mailbox, 0).unwrap();
        let changes = since.changes.unwrap();
        assert_eq!(changes.len(), 3);
        assert_eq!(changes[0].sequence, 1);
        assert_eq!(changes[2].sequence, 2);
        assert_eq!(changes[2].id, "m2-0");
        assert_eq!(changes[2].kind, ChangeKind::Created);
        let later = store.changes_since(&key(), ObjectType::Mailbox, 1).unwrap();
        assert_eq!(later.changes.unwrap().len(), 1);
        let emails = store.changes_since(&key(), ObjectType::Email, 0).unwrap();
        assert_eq!(emails.changes, Some(Vec::new()));
    }

    /// The rows the log holds.
    fn kept(store: &Store) -> u64 {
        store
            .read(|connection| {
                Ok(connection
                    .query_row("SELECT COUNT(*) FROM bridge_changes", [], |row| row.get(0))?)
            })
            .unwrap()
    }

    #[test]
    fn a_sequence_that_straddles_the_horizon_stays_whole() {
        let store = Store::in_memory().unwrap();
        batch(&store, 5);
        batch(&store, CHANGES_HORIZON - 2);
        assert_eq!(kept(&store), CHANGES_HORIZON + 3);
        let since = store.changes_since(&key(), ObjectType::Mailbox, 0).unwrap();
        assert_eq!(since.changes.map(|changes| changes.len()), Some(10_003));
    }

    #[test]
    fn the_horizon_forgets_whole_sequences_and_a_state_below_it_cannot_be_calculated() {
        let store = Store::in_memory().unwrap();
        batch(&store, 2);
        batch(&store, CHANGES_HORIZON);
        assert_eq!(kept(&store), CHANGES_HORIZON);
        let below = store.changes_since(&key(), ObjectType::Mailbox, 0).unwrap();
        assert_eq!(below.changes, None);
        let at = store.changes_since(&key(), ObjectType::Mailbox, 1).unwrap();
        assert_eq!(at.changes.map(|changes| changes.len()), Some(10_000));
        let current = store.changes_since(&key(), ObjectType::Mailbox, 2).unwrap();
        assert_eq!(current.changes, Some(Vec::new()));
    }

    #[test]
    fn an_unknown_kind_in_the_column_is_refused() {
        let store = Store::in_memory().unwrap();
        let result = store.read(|connection| {
            Ok(connection.query_row("SELECT 'archived'", [], |row| row.get::<_, ChangeKind>(0))?)
        });
        assert!(result.is_err());
    }
}
