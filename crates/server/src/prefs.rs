// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Server-side per-user preferences and per-sender policies.

use rusqlite::{OptionalExtension, params};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::ids::text_enum;
use crate::scope::Scope;
use crate::store::{Store, StoreError, now_ms};

/// Policy rows one user may hold; a second device reads them as one
/// list, so the list stays one answer.
pub const MAX_SENDER_POLICIES: usize = 5000;

/// The longest address a policy is keyed on: the 320 octets of a
/// local part, the at sign and a domain.
pub const MAX_SENDER_BYTES: usize = 320;

/// An authserv-id is a domain name at most (RFC 8601 section 2.5).
pub const MAX_AUTHSERV_BYTES: usize = 255;

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
        FontSize => "fontSize",
        LineHeight => "lineHeight",
        DarkMail => "darkMail",
    }
);

impl PreferenceKey {
    /// Every key the API lists.
    pub const ALL: [Self; 7] = [
        Self::ReadingPane,
        Self::Theme,
        Self::Density,
        Self::Locale,
        Self::FontSize,
        Self::LineHeight,
        Self::DarkMail,
    ];

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
            Self::FontSize => &["default", "large", "larger"],
            Self::LineHeight => &["default", "relaxed", "loose"],
            Self::DarkMail => &["adapt", "original"],
        }
    }
}

text_enum!(
    /// The policies the sender-policy API reads and writes.
    PolicyName {
        RemoteContent => "remoteContent",
    }
);

impl PolicyName {
    /// Every policy the API lists.
    pub const ALL: [Self; 1] = [Self::RemoteContent];

    /// The policy behind its word; `None` off the list.
    #[must_use]
    pub fn from_word(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|name| name.as_str() == word)
    }
}

/// The remote-content grant for one sender: the authserv-id of the
/// message's topmost Authentication-Results header at the grant, the
/// empty string when that header named no server and none when the
/// message carried no such header.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteContentPolicy {
    pub allow: bool,
    pub authserv: Option<String>,
}

/// One listed policy row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SenderPolicy {
    pub sender: String,
    pub key: PolicyName,
    pub value: RemoteContentPolicy,
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

/// Writes one per-sender policy value for the scope's user; a new row
/// past [`MAX_SENDER_POLICIES`] is refused, an update of a row that
/// stands is not.
///
/// # Errors
///
/// Returns [`StoreError::PolicyLimit`] past the bound and an error
/// when the value does not encode or the database fails.
pub fn set_sender_policy<T: Serialize>(
    store: &Store,
    scope: &Scope,
    key: PolicyKey<'_>,
    value: &T,
) -> Result<(), StoreError> {
    let encoded = serde_json::to_string(value)?;
    store.write(|transaction| {
        let held: usize = transaction.query_row(
            "SELECT COUNT(*) FROM sender_policies WHERE user_id = ?1",
            [scope.user_id().as_str()],
            |row| row.get(0),
        )?;
        let standing: bool = transaction.query_row(
            "SELECT EXISTS (SELECT 1 FROM sender_policies
                            WHERE user_id = ?1 AND sender = ?2 AND key = ?3)",
            [scope.user_id().as_str(), key.sender, key.name],
            |row| row.get(0),
        )?;
        if held >= MAX_SENDER_POLICIES && !standing {
            return Err(StoreError::PolicyLimit);
        }
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

/// Every listed policy of the scope's user, by sender and key; a row
/// under a key off the list stays out.
///
/// # Errors
///
/// Returns an error when a stored value does not decode or the
/// database fails.
pub fn sender_policies(store: &Store, scope: &Scope) -> Result<Vec<SenderPolicy>, StoreError> {
    let rows: Vec<(String, String, String)> = store.read(|connection| {
        let mut statement = connection.prepare(
            "SELECT sender, key, value FROM sender_policies
             WHERE user_id = ?1 ORDER BY sender, key LIMIT ?2",
        )?;
        let rows = statement
            .query_map(
                params![scope.user_id().as_str(), MAX_SENDER_POLICIES],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })?;
    rows.into_iter()
        .filter_map(|(sender, key, encoded)| {
            PolicyName::from_word(&key).map(|key| (sender, key, encoded))
        })
        .map(|(sender, key, encoded)| {
            Ok(SenderPolicy {
                sender,
                key,
                value: serde_json::from_str(&encoded)?,
            })
        })
        .collect()
}

/// Removes one per-sender policy row of the scope's user; a row that
/// is not there leaves nothing to do.
///
/// # Errors
///
/// Returns an error when the database fails.
pub fn remove_sender_policy(
    store: &Store,
    scope: &Scope,
    key: PolicyKey<'_>,
) -> Result<(), StoreError> {
    store.write(|transaction| {
        transaction.execute(
            "DELETE FROM sender_policies WHERE user_id = ?1 AND sender = ?2 AND key = ?3",
            [scope.user_id().as_str(), key.sender, key.name],
        )?;
        Ok(())
    })
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
        assert!(!PreferenceKey::FontSize.accepts("small"));
        assert!(!PreferenceKey::DarkMail.accepts("Adapt"));
    }

    #[test]
    fn a_policy_name_is_found_by_its_word_and_its_value_takes_the_shape_alone() {
        assert_eq!(
            PolicyName::from_word("remoteContent"),
            Some(PolicyName::RemoteContent)
        );
        assert_eq!(PolicyName::from_word("remote_content"), None);
        let grant: RemoteContentPolicy =
            serde_json::from_str(r#"{"allow":true,"authserv":"mx.example"}"#).unwrap();
        assert_eq!(grant.authserv.as_deref(), Some("mx.example"));
        let unpinned: RemoteContentPolicy = serde_json::from_str(r#"{"allow":true}"#).unwrap();
        assert_eq!(unpinned.authserv, None);
        assert!(
            serde_json::from_str::<RemoteContentPolicy>(r#"{"allow":true,"authserv":null,"x":1}"#)
                .is_err()
        );
    }
}
