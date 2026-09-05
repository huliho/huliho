// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The sign-in providers an instance can hold a client for; the row in
//! `auth_providers` is that client.

use rusqlite::OptionalExtension;

use crate::ids::text_enum;
use crate::store::{Store, StoreError};

text_enum!(
    /// The providers a mail account can sign in through.
    OauthProvider {
        Google => "google",
        Microsoft => "microsoft",
    }
);

impl OauthProvider {
    /// The issuer, which keys the provider row.
    #[must_use]
    pub fn issuer(self) -> &'static str {
        match self {
            Self::Google => "https://accounts.google.com",
            Self::Microsoft => "https://login.microsoftonline.com/common/v2.0",
        }
    }
}

/// Whether the instance holds a client for the provider.
///
/// # Errors
///
/// Returns an error when the database fails.
pub fn is_registered(store: &Store, provider: OauthProvider) -> Result<bool, StoreError> {
    store.read(|connection| {
        let row: Option<i64> = connection
            .query_row(
                "SELECT 1 FROM auth_providers WHERE issuer = ?1",
                [provider.issuer()],
                |row| row.get(0),
            )
            .optional()?;
        Ok(row.is_some())
    })
}

#[cfg(test)]
mod tests {
    use rusqlite::params;

    use super::*;

    fn register(store: &Store, provider: OauthProvider) {
        store
            .write(|transaction| {
                transaction.execute(
                    "INSERT INTO auth_providers
                     (id, issuer, discovery_url, client_id, created_at)
                     VALUES (?1, ?2, ?3, 'client-id', 0)",
                    params![
                        provider.as_str(),
                        provider.issuer(),
                        format!("{}/.well-known/openid-configuration", provider.issuer())
                    ],
                )?;
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn nothing_is_registered_on_a_fresh_store() {
        let store = Store::in_memory().unwrap();
        for provider in [OauthProvider::Google, OauthProvider::Microsoft] {
            assert!(!is_registered(&store, provider).unwrap());
        }
    }

    #[test]
    fn a_row_registers_its_own_provider_only() {
        let store = Store::in_memory().unwrap();
        register(&store, OauthProvider::Google);
        assert!(is_registered(&store, OauthProvider::Google).unwrap());
        assert!(!is_registered(&store, OauthProvider::Microsoft).unwrap());
    }

    #[test]
    fn the_provider_words_are_stable() {
        assert_eq!(OauthProvider::Google.as_str(), "google");
        assert_eq!(OauthProvider::Microsoft.as_str(), "microsoft");
        assert_eq!(
            serde_json::to_string(&OauthProvider::Microsoft).unwrap(),
            "\"microsoft\""
        );
    }
}
