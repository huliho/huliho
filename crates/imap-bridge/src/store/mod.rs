// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The bridge's tables inside the host's database: the schema as
//! constants the host applies, one connection the bridge locks per call.

mod changes;
mod emails;
mod folders;
mod gmail;
mod ids;
mod ledger;
mod mailboxes;
mod personal;
mod previews;
mod progress;
mod query;
mod refresh;
mod threads;

use std::collections::HashSet;
use std::sync::{Mutex, MutexGuard, PoisonError};

use rusqlite::{Connection, Transaction, TransactionBehavior};
use thiserror::Error;

use crate::seal::SealError;

pub use changes::{CHANGES_HORIZON, Change, ChangeKind, ChangesSince, ObjectType};
pub use emails::{Batch, EmailFacts, EmailRow, EmailSnapshot};
pub use folders::{REMATCH_LIMIT, Standing};
pub use gmail::GmailFacts;
pub use ids::{AccountKey, EmailId, MailboxId, ThreadId};
pub use mailboxes::{Counts, MailboxFacts, MailboxRow, MailboxSnapshot};
pub use personal::{Address, Personal, PreviewPart};
pub use previews::Located;
pub use progress::{Advance, Progress};
pub use query::{Queried, Query, Start, Window};
pub use refresh::{FlagChange, Synced};
pub use threads::ThreadSnapshot;

/// The schema, one entry per migration; the host's migration list
/// applies them in order under its own version counter. The second
/// entry indexes the message ids by thread, which a merge of two
/// threads and the end of one search by, and gives the progress row of
/// a folder the values the refresh stands at.
pub const MIGRATIONS: &[&str] = &[
    include_str!("schema.sql"),
    include_str!("refresh_progress.sql"),
];

/// Every table of the bridge, the ones an account's rows leave from.
const TABLES: [&str; 7] = [
    "bridge_changes",
    "bridge_emails",
    "bridge_mailboxes",
    "bridge_memberships",
    "bridge_message_ids",
    "bridge_state",
    "bridge_sync",
];

/// Why the store could not answer.
#[derive(Debug, Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("cannot encode a stored value: {0}")]
    Encoding(#[from] serde_json::Error),
    #[error(transparent)]
    Seal(#[from] SealError),
    #[error("the store lock was poisoned by an earlier panic")]
    Poisoned,
    /// The host forgot the account; nothing is written for it anymore.
    #[error("the account was forgotten")]
    Forgotten,
}

/// The bridge's handle on the database; every read and write goes
/// through it.
pub struct Store {
    connection: Mutex<Connection>,
    /// The accounts the host forgot: a write for one is refused, so no
    /// row lands after the host deleted the account's rows.
    forgotten: Mutex<HashSet<AccountKey>>,
}

impl Store {
    /// Takes a connection the host opened with the schema present.
    #[must_use]
    pub fn new(connection: Connection) -> Self {
        Self {
            connection: Mutex::new(connection),
            forgotten: Mutex::new(HashSet::new()),
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

    /// Refuses every later write for the account; the host deletes its
    /// rows in the transaction that removes the account.
    pub fn forget(&self, key: &AccountKey) {
        self.forgotten
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key.clone());
    }

    pub(crate) fn read<T>(
        &self,
        operation: impl FnOnce(&Connection) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let connection = self.lock()?;
        operation(&connection)
    }

    /// One write for the account as one transaction; refused once the
    /// host forgot the account.
    pub(crate) fn write<T>(
        &self,
        key: &AccountKey,
        operation: impl FnOnce(&Transaction<'_>) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        if self
            .forgotten
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(key)
        {
            return Err(StoreError::Forgotten);
        }
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

/// Deletes every row of the account on a connection of the host's, so
/// the rows leave in the transaction that removes the account.
///
/// # Errors
///
/// Returns the database error.
pub fn remove_rows(connection: &Connection, key: &AccountKey) -> rusqlite::Result<()> {
    for table in TABLES {
        connection.execute(
            &format!("DELETE FROM {table} WHERE account_key = ?1"),
            [key.as_str()],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn key() -> AccountKey {
        AccountKey::new("a1")
    }

    /// The tables, in the order the schema names them.
    fn tables(store: &Store) -> Vec<String> {
        store
            .read(|connection| {
                let mut statement = connection
                    .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")?;
                let names = statement
                    .query_map([], |row| row.get(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(names)
            })
            .unwrap()
    }

    #[test]
    fn the_schema_creates_the_seven_tables() {
        let store = Store::in_memory().unwrap();
        assert_eq!(tables(&store), TABLES);
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
            .write(&key(), |_transaction| {
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

    #[test]
    fn a_forgotten_account_takes_no_write_and_every_other_account_does() {
        let store = Store::in_memory().unwrap();
        store.forget(&key());
        let refused = store.write(&key(), |_transaction| Ok(()));
        assert!(matches!(refused, Err(StoreError::Forgotten)), "{refused:?}");
        store
            .write(&AccountKey::new("a2"), |_transaction| Ok(()))
            .unwrap();
    }

    /// The rows the tables hold for the key.
    fn rows_of(connection: &Connection, key: &str) -> Vec<(String, i64)> {
        TABLES
            .iter()
            .map(|table| {
                let count = connection
                    .query_row(
                        &format!("SELECT COUNT(*) FROM {table} WHERE account_key = ?1"),
                        [key],
                        |row| row.get(0),
                    )
                    .unwrap();
                ((*table).to_owned(), count)
            })
            .collect()
    }

    #[test]
    fn removing_an_account_empties_every_table_for_its_key_alone() {
        let connection = Connection::open_in_memory().unwrap();
        Store::apply_schema(&connection).unwrap();
        for key in ["a1", "a2"] {
            connection
                .execute_batch(&format!(
                    "INSERT INTO bridge_state (account_key, sequence) VALUES ('{key}', 1);
                     INSERT INTO bridge_sync (account_key, folder_id, done) VALUES ('{key}', 'm', 0);
                     INSERT INTO bridge_changes (account_key, sequence, type, id, kind)
                         VALUES ('{key}', 1, 'Mailbox', 'm', 'created');
                     INSERT INTO bridge_mailboxes (account_key, id, name, imap_name, sort_order,
                         subscribed, selectable, store)
                         VALUES ('{key}', 'm', 'INBOX', 'INBOX', 0, 1, 1, 1);
                     INSERT INTO bridge_emails (account_key, id, folder_id, uid, thread_id, keywords,
                         size, received_at, has_attachment, sealed)
                         VALUES ('{key}', 'e', 'm', 1, 't', '{{}}', 0, 0, 0, X'00');
                     INSERT INTO bridge_memberships (account_key, email_id, mailbox_id, received_at)
                         VALUES ('{key}', 'e', 'm', 0);
                     INSERT INTO bridge_message_ids (account_key, message_id_hash, thread_id)
                         VALUES ('{key}', X'01', 't');"
                ))
                .unwrap();
        }
        remove_rows(&connection, &key()).unwrap();
        assert!(
            rows_of(&connection, "a1")
                .iter()
                .all(|(_, count)| *count == 0)
        );
        assert!(
            rows_of(&connection, "a2")
                .iter()
                .all(|(_, count)| *count == 1)
        );
    }
}
