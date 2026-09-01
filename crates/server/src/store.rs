// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! One embedded database in the data volume, migrated at startup.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, Transaction};
use rusqlite_migration::{M, Migrations};
use thiserror::Error;

/// The database file inside the data volume.
const DATABASE_FILE: &str = "huliho.db";

/// Forward-only numbered migrations, embedded so the binary carries its schema.
const MIGRATION_SOURCES: &[&str] = &[include_str!("migrations/0001_identity.sql")];

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("cannot prepare the data directory {path}: {source}")]
    DataDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("migration error: {0}")]
    Migration(#[from] rusqlite_migration::Error),
    #[error("cannot encode a stored value: {0}")]
    Encoding(#[from] serde_json::Error),
    #[error("the store lock was poisoned by an earlier panic")]
    Poisoned,
    #[error("not found within the current scope")]
    NotFound,
    #[error("the current role does not permit this action")]
    Forbidden,
    #[error("an organization keeps at least one owner")]
    LastOwner,
    #[error("the scope carries no account")]
    MissingAccount,
}

/// Handle to the embedded database; all access goes through it.
pub struct Store {
    connection: Mutex<Connection>,
}

impl Store {
    /// Opens the database inside `data_dir`, creating both when missing,
    /// and applies pending migrations.
    ///
    /// # Errors
    ///
    /// Returns an error when the directory cannot be created, the
    /// database cannot be opened or a migration fails.
    pub fn open(data_dir: &Path) -> Result<Self, StoreError> {
        std::fs::create_dir_all(data_dir).map_err(|source| StoreError::DataDirectory {
            path: data_dir.to_owned(),
            source,
        })?;
        Self::initialize(Connection::open(data_dir.join(DATABASE_FILE))?)
    }

    /// Opens a fresh in-memory database, migrated to the latest schema.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be opened or a
    /// migration fails.
    pub fn in_memory() -> Result<Self, StoreError> {
        Self::initialize(Connection::open_in_memory()?)
    }

    fn initialize(mut connection: Connection) -> Result<Self, StoreError> {
        connection.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
        connection.pragma_update(None, "foreign_keys", true)?;
        migrations().to_latest(&mut connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
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
        let transaction = connection.transaction()?;
        let value = operation(&transaction)?;
        transaction.commit()?;
        Ok(value)
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, StoreError> {
        self.connection.lock().map_err(|_| StoreError::Poisoned)
    }
}

fn migrations() -> Migrations<'static> {
    Migrations::new(MIGRATION_SOURCES.iter().map(|sql| M::up(sql)).collect())
}

/// Current time as unix milliseconds, the timestamp unit of every table.
pub(crate) fn now_ms() -> i64 {
    let since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    i64::try_from(since_epoch.as_millis()).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_valid() {
        migrations().validate().unwrap();
    }

    #[test]
    fn now_is_after_the_epoch() {
        assert!(now_ms() > 0);
    }
}
