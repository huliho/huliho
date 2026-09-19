// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The opaque ids the bridge hands out: a type letter in front of
//! `UUIDv4` text, so an id starts inside the alphabet of RFC 8620
//! section 1.2 and never looks like a number.

use std::fmt;

use rusqlite::types::{FromSql, FromSqlResult, ToSql, ToSqlOutput, ValueRef};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The host's name for an account; every bridge row carries it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AccountKey(String);

impl AccountKey {
    /// Wraps the host's id.
    #[must_use]
    pub fn new(key: impl Into<String>) -> Self {
        Self(key.into())
    }

    /// The key as the columns hold it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

macro_rules! id_type {
    ($(#[$meta:meta])* $name:ident, $letter:literal) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// A fresh id: the type letter, then `UUIDv4` text.
            #[must_use]
            pub fn generate() -> Self {
                Self(format!("{}{}", $letter, Uuid::new_v4()))
            }

            /// The id as the columns and the JSON hold it.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl FromSql for $name {
            fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
                String::column_result(value).map(Self)
            }
        }

        impl ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
                self.0.to_sql()
            }
        }
    };
}

id_type!(
    /// Identifies a mailbox.
    MailboxId,
    'm'
);

id_type!(
    /// Identifies an email.
    EmailId,
    'e'
);

id_type!(
    /// Identifies a thread.
    ThreadId,
    't'
);

#[cfg(test)]
mod tests {
    use super::*;

    /// The letter, then the 36 characters of hyphenated UUID text.
    const ID_LENGTH: usize = 37;

    fn inside_the_alphabet(c: char) -> bool {
        c.is_ascii_alphanumeric() || c == '-' || c == '_'
    }

    #[test]
    fn an_id_starts_with_its_letter_and_stays_inside_the_alphabet_rfc8620_1_2() {
        for (id, letter) in [
            (MailboxId::generate().to_string(), 'm'),
            (EmailId::generate().to_string(), 'e'),
            (ThreadId::generate().to_string(), 't'),
        ] {
            assert_eq!(id.chars().next(), Some(letter), "{id}");
            assert_eq!(id.len(), ID_LENGTH, "{id}");
            assert!(id.chars().all(inside_the_alphabet), "{id}");
        }
    }

    #[test]
    fn generated_ids_are_unique_and_serialize_as_plain_strings() {
        let first = MailboxId::generate();
        assert_ne!(first, MailboxId::generate());
        let json = serde_json::to_string(&first).unwrap();
        assert_eq!(json, format!("\"{first}\""));
        assert_eq!(serde_json::from_str::<MailboxId>(&json).unwrap(), first);
    }
}
