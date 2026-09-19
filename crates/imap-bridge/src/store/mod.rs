// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The bridge's tables inside the host's database: the schema as
//! constants the host applies, one connection the bridge locks per call.

mod changes;
mod ids;
mod mailboxes;

use std::sync::{Mutex, MutexGuard};

use rusqlite::{Connection, Transaction, TransactionBehavior};
use thiserror::Error;

pub use changes::{CHANGES_HORIZON, Change, ChangeKind, ChangesSince, ObjectType};
pub use ids::{AccountKey, EmailId, MailboxId, ThreadId};
pub use mailboxes::{Counts, MailboxFacts, MailboxRow, MailboxSnapshot};

/// The schema, one entry per migration; the host's migration list
/// applies them in order under its own version counter.
pub const MIGRATIONS: &[&str] = &[include_str!("schema.sql")];

/// Why the store could not answer.
#[derive(Debug, Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("cannot encode a stored value: {0}")]
    Encoding(#[from] serde_json::Error),
    #[error("the store lock was poisoned by an earlier panic")]
    Poisoned,
}

/// The bridge's handle on the database; every read and write goes
/// through it.
pub struct Store {
    connection: Mutex<Connection>,
}

impl Store {
    /// Takes a connection the host opened with the schema present.
    #[must_use]
    pub fn new(connection: Connection) -> Self {
        Self {
            connection: Mutex::new(connection),
        }
    }

    /// Applies every migration to a fresh database.
    fn apply_schema(connection: &Connection) -> Result<(), StoreError> {
        for sql in MIGRATIONS {
            connection.execute_batch(sql)?;
        }
        Ok(())
    }

    /// A fresh in-memory database with the schema applied.
    ///
    /// # Errors
    ///
    /// Returns the database error when the database cannot be opened
    /// or a statement fails.
    pub fn in_memory() -> Result<Self, StoreError> {
        let connection = Connection::open_in_memory()?;
        Self::apply_schema(&connection)?;
        Ok(Self::new(connection))
    }

    pub(crate) fn read<T>(
        &self,
        operation: impl FnOnce(&Connection) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let connection = self.lock()?;
        operation(&connection)
    }

    pub(crate) fn write<T>(
        &self,
        operation: impl FnOnce(&Transaction<'_>) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let mut connection = self.lock()?;
        // A deferred transaction that read cannot write past another connection's commit.
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let value = operation(&transaction)?;
        transaction.commit()?;
        Ok(value)
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, StoreError> {
        self.connection.lock().map_err(|_| StoreError::Poisoned)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn the_schema_creates_the_seven_tables() {
        let store = Store::in_memory().unwrap();
        let tables: Vec<String> = store
            .read(|connection| {
                let mut statement = connection
                    .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")?;
                let names = statement
                    .query_map([], |row| row.get(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(names)
            })
            .unwrap();
        assert_eq!(
            tables,
            [
                "bridge_changes",
                "bridge_emails",
                "bridge_mailboxes",
                "bridge_memberships",
                "bridge_message_ids",
                "bridge_state",
                "bridge_sync"
            ]
        );
    }

    /// A deferred transaction would let the second connection commit
    /// here. A writer that had read by then fails at once as a stale
    /// snapshot, whatever its busy timeout.
    #[test]
    fn a_write_holds_the_write_lock_from_its_start() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("host.sqlite");
        let bridge = Connection::open(&path).unwrap();
        bridge
            .query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))
            .unwrap();
        Store::apply_schema(&bridge).unwrap();
        let host = Connection::open(&path).unwrap();
        host.busy_timeout(Duration::ZERO).unwrap();
        let store = Store::new(bridge);
        store
            .write(|_transaction| {
                let outcome = host.execute(
                    "INSERT INTO bridge_state (account_key, sequence) VALUES ('a1', 1)",
                    [],
                );
                let Err(rusqlite::Error::SqliteFailure(failure, _)) = outcome else {
                    panic!("the second connection wrote inside the first one's write");
                };
                assert_eq!(failure.code, rusqlite::ErrorCode::DatabaseBusy);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn the_schema_applies_once_only() {
        let connection = Connection::open_in_memory().unwrap();
        Store::apply_schema(&connection).unwrap();
        assert!(Store::apply_schema(&connection).is_err());
    }
}
