// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Server-side per-user preferences and per-sender policies.

use rusqlite::{OptionalExtension, params};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::ids::text_enum;
use crate::scope::Scope;
use crate::store::{Store, StoreError, now_ms};

/// Addresses one per-sender policy value.
#[derive(Debug, Clone, Copy)]
pub struct PolicyKey<'a> {
    pub sender: &'a str,
    pub name: &'a str,
}

text_enum!(
    /// The keys the preferences API reads and writes; nothing else is
    /// reachable over it.
    PreferenceKey {
        ReadingPane => "readingPane",
        Theme => "theme",
        Density => "density",
        Locale => "locale",
    }
);

impl PreferenceKey {
    /// Every key the API lists.
    pub const ALL: [Self; 4] = [Self::ReadingPane, Self::Theme, Self::Density, Self::Locale];

    /// The key behind its word; `None` off the list.
    #[must_use]
    pub fn from_word(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|key| key.as_str() == word)
    }

    /// Whether `word` is one the key takes.
    #[must_use]
    pub fn accepts(self, word: &str) -> bool {
        self.words().contains(&word)
    }

    /// The words the key takes.
    #[must_use]
    pub fn words(self) -> &'static [&'static str] {
        match self {
            Self::ReadingPane => &["right", "bottom", "off"],
            Self::Theme => &["system", "light", "dark"],
            Self::Density => &["comfortable", "compact"],
            Self::Locale => &["en", "nl"],
        }
    }
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

/// The listed preferences of the scope's user as `(key, word)`; a key
/// never written is absent and every other row of the user stays out.
///
/// # Errors
///
/// Returns an error when a stored value does not decode as a word or
/// the database fails.
pub fn preferences(
    store: &Store,
    scope: &Scope,
) -> Result<Vec<(PreferenceKey, String)>, StoreError> {
    let rows: Vec<(String, String)> = store.read(|connection| {
        let mut statement =
            connection.prepare("SELECT key, value FROM user_preferences WHERE user_id = ?1")?;
        let rows = statement
            .query_map([scope.user_id().as_str()], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })?;
    rows.into_iter()
        .filter_map(|(key, encoded)| PreferenceKey::from_word(&key).map(|key| (key, encoded)))
        .map(|(key, encoded)| Ok((key, serde_json::from_str(&encoded)?)))
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_is_found_by_its_word_and_takes_its_words_only() {
        for key in PreferenceKey::ALL {
            assert_eq!(PreferenceKey::from_word(key.as_str()), Some(key));
            for word in key.words() {
                assert!(key.accepts(word), "{key:?} {word}");
            }
            assert!(!key.accepts(""));
            assert!(!key.accepts("Right"));
        }
        assert_eq!(PreferenceKey::from_word("compose_size"), None);
        assert!(!PreferenceKey::Locale.accepts("en-XA"));
    }
}
