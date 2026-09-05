// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Typed identifiers and the fixed role set.

use rusqlite::types::{FromSql, FromSqlResult, ValueRef};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! id_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub struct $name(String);

        impl $name {
            pub(crate) fn generate() -> Self {
                Self(Uuid::new_v4().to_string())
            }

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

        impl FromSql for $name {
            fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
                String::column_result(value).map(Self)
            }
        }
    };
}

/// A closed set of words stored as TEXT: `as_str` for the column and
/// `FromSql` back, refusing any word outside the set.
macro_rules! text_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
        pub enum $name {
            $(
                #[serde(rename = $text)]
                $variant
            ),+
        }

        impl $name {
            #[must_use]
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text),+
                }
            }
        }

        impl ::rusqlite::types::FromSql for $name {
            fn column_result(
                value: ::rusqlite::types::ValueRef<'_>,
            ) -> ::rusqlite::types::FromSqlResult<Self> {
                let word = <String as ::rusqlite::types::FromSql>::column_result(value)?;
                match word.as_str() {
                    $($text => Ok(Self::$variant),)+
                    other => Err(::rusqlite::types::FromSqlError::Other(
                        format!("unknown {} {other}", stringify!($name)).into(),
                    )),
                }
            }
        }
    };
}

pub(crate) use text_enum;

id_type!(
    /// Identifies an organization.
    OrganizationId
);

id_type!(
    /// Identifies a Huliho user.
    UserId
);

id_type!(
    /// Identifies a connected mail account.
    AccountId
);

id_type!(
    /// Identifies a session toward the API; the cookie token stays secret.
    SessionId
);

text_enum!(
    /// Fixed roles within an organization, ordered lowest authority first.
    #[derive(PartialOrd, Ord)]
    Role {
        Member => "member",
        Admin => "admin",
        Owner => "owner",
    }
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_order_by_authority() {
        assert!(Role::Member < Role::Admin);
        assert!(Role::Admin < Role::Owner);
    }

    #[test]
    fn generated_ids_are_unique() {
        assert_ne!(UserId::generate(), UserId::generate());
    }

    #[test]
    fn a_role_serializes_to_its_stored_word() {
        for role in [Role::Member, Role::Admin, Role::Owner] {
            let json = serde_json::to_string(&role).unwrap();
            assert_eq!(json, format!("\"{}\"", role.as_str()));
            assert_eq!(serde_json::from_str::<Role>(&json).unwrap(), role);
        }
    }
}
