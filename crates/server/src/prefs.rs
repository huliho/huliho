// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Server-side per-user preferences and per-sender policies.

use rusqlite::{OptionalExtension, params};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::scope::Scope;
use crate::store::{Store, StoreError, now_ms};

/// Addresses one per-sender policy value.
#[derive(Debug, Clone, Copy)]
pub struct PolicyKey<'a> {
    pub sender: &'a str,
    pub name: &'a str,
}

/// Writes one preference value for the scope's user.
///
/// # Errors
///
/// Returns an error when the value does not encode or the database
/// fails.
pub fn set_preference<T: Serialize>(
    store: &Store,
    scope: &Scope,
    key: &str,
    value: &T,
) -> Result<(), StoreError> {
    let encoded = serde_json::to_string(value)?;
    store.write(|transaction| {
        transaction.execute(
            "INSERT INTO user_preferences (user_id, key, value, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (user_id, key)
             DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            params![scope.user_id().as_str(), key, encoded, now_ms()],
        )?;
        Ok(())
    })
}

/// Reads one preference value for the scope's user.
///
/// # Errors
///
/// Returns an error when the stored value does not decode or the
/// database fails.
pub fn preference<T: DeserializeOwned>(
    store: &Store,
    scope: &Scope,
    key: &str,
) -> Result<Option<T>, StoreError> {
    let stored: Option<String> = store.read(|connection| {
        let value = connection
            .query_row(
                "SELECT value FROM user_preferences WHERE user_id = ?1 AND key = ?2",
                [scope.user_id().as_str(), key],
                |row| row.get(0),
            )
            .optional()?;
        Ok(value)
    })?;
    decode(stored)
}

/// Writes one per-sender policy value for the scope's user.
///
/// # Errors
///
/// Returns an error when the value does not encode or the database
/// fails.
pub fn set_sender_policy<T: Serialize>(
    store: &Store,
    scope: &Scope,
    key: PolicyKey<'_>,
    value: &T,
) -> Result<(), StoreError> {
    let encoded = serde_json::to_string(value)?;
    store.write(|transaction| {
        transaction.execute(
            "INSERT INTO sender_policies (user_id, sender, key, value, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (user_id, sender, key)
             DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            params![
                scope.user_id().as_str(),
                key.sender,
                key.name,
                encoded,
                now_ms()
            ],
        )?;
        Ok(())
    })
}

/// Reads one per-sender policy value for the scope's user.
///
/// # Errors
///
/// Returns an error when the stored value does not decode or the
/// database fails.
pub fn sender_policy<T: DeserializeOwned>(
    store: &Store,
    scope: &Scope,
    key: PolicyKey<'_>,
) -> Result<Option<T>, StoreError> {
    let stored: Option<String> = store.read(|connection| {
        let value = connection
            .query_row(
                "SELECT value FROM sender_policies
                 WHERE user_id = ?1 AND sender = ?2 AND key = ?3",
                [scope.user_id().as_str(), key.sender, key.name],
                |row| row.get(0),
            )
            .optional()?;
        Ok(value)
    })?;
    decode(stored)
}

fn decode<T: DeserializeOwned>(stored: Option<String>) -> Result<Option<T>, StoreError> {
    stored
        .map(|value| serde_json::from_str(&value))
        .transpose()
        .map_err(StoreError::from)
}
