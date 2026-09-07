// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The sign-in providers an instance can hold a client for; the row in
//! `auth_providers` is that client, its secret sealed under the issuer so
//! it opens for no other provider.

use std::fmt;

use rusqlite::{OptionalExtension, params};

use crate::ids::{ProviderId, text_enum};
use crate::scope::Scope;
use crate::sealed;
use crate::secrets::Keys;
use crate::store::{Store, StoreError, now_ms};

text_enum!(
    /// The providers a mail account can sign in through.
    OauthProvider {
        Google => "google",
        Microsoft => "microsoft",
    }
);

/// A refresh token comes only when the consent asks for it: Google wants
/// these two parameters, Microsoft a scope.
const GOOGLE_EXTRA_PARAMS: &[(&str, &str)] = &[("access_type", "offline"), ("prompt", "consent")];

/// Full mailbox access, the one scope Gmail's IMAP and SMTP accept.
const GOOGLE_SCOPES: &[&str] = &["https://mail.google.com/"];

/// IMAP and SMTP access plus the refresh token.
const MICROSOFT_SCOPES: &[&str] = &[
    "https://outlook.office.com/IMAP.AccessAsUser.All",
    "https://outlook.office.com/SMTP.Send",
    "offline_access",
];

const ALL: [OauthProvider; 2] = [OauthProvider::Google, OauthProvider::Microsoft];

impl OauthProvider {
    /// The provider behind a word from a URL path.
    #[must_use]
    pub fn from_word(word: &str) -> Option<Self> {
        ALL.into_iter().find(|provider| provider.as_str() == word)
    }

    /// The issuer, which keys the provider row.
    #[must_use]
    pub fn issuer(self) -> &'static str {
        match self {
            Self::Google => "https://accounts.google.com",
            Self::Microsoft => "https://login.microsoftonline.com/common/v2.0",
        }
    }

    /// The `OpenID` configuration document, for the sign-in that comes
    /// later.
    #[must_use]
    pub fn discovery_url(self) -> &'static str {
        match self {
            Self::Google => "https://accounts.google.com/.well-known/openid-configuration",
            Self::Microsoft => {
                "https://login.microsoftonline.com/common/v2.0/.well-known/openid-configuration"
            }
        }
    }

    /// The authorization endpoint the discovery document names.
    #[must_use]
    pub fn authorization_url(self) -> &'static str {
        match self {
            Self::Google => "https://accounts.google.com/o/oauth2/v2/auth",
            Self::Microsoft => "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
        }
    }

    /// The token endpoint the discovery document names.
    #[must_use]
    pub fn token_url(self) -> &'static str {
        match self {
            Self::Google => "https://oauth2.googleapis.com/token",
            Self::Microsoft => "https://login.microsoftonline.com/common/oauth2/v2.0/token",
        }
    }

    /// The least a mail client asks for.
    #[must_use]
    pub fn scopes(self) -> &'static [&'static str] {
        match self {
            Self::Google => GOOGLE_SCOPES,
            Self::Microsoft => MICROSOFT_SCOPES,
        }
    }

    /// Parameters beyond the standard ones the consent needs.
    #[must_use]
    pub fn extra_params(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Google => GOOGLE_EXTRA_PARAMS,
            Self::Microsoft => &[],
        }
    }

    fn from_issuer(issuer: &str) -> Option<Self> {
        ALL.into_iter().find(|provider| provider.issuer() == issuer)
    }
}

/// A registered client: what the admin got from the provider.
#[derive(Clone, PartialEq, Eq)]
pub struct OauthClient {
    pub provider: OauthProvider,
    pub id: String,
    pub secret: String,
}

/// Prints the provider and the client id; the secret stays out.
impl fmt::Debug for OauthClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "OauthClient({}, {})", self.provider.as_str(), self.id)
    }
}

/// A row as the list shows it: never the secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredClient {
    pub provider: OauthProvider,
    pub id: String,
}

/// Registers or replaces the provider's client. Requires the
/// instance-admin flag.
///
/// # Errors
///
/// Returns [`StoreError::Forbidden`] without the flag; sealing and
/// database failures pass through.
pub fn set_client(
    store: &Store,
    keys: &Keys,
    scope: &Scope,
    client: &OauthClient,
) -> Result<(), StoreError> {
    scope.require_instance_admin()?;
    let issuer = client.provider.issuer();
    let sealed = sealed::seal(
        keys.providers(),
        issuer.as_bytes(),
        client.secret.as_bytes(),
    )?;
    let id = ProviderId::generate();
    store.write(|transaction| {
        transaction.execute(
            "INSERT INTO auth_providers
             (id, issuer, discovery_url, client_id, client_secret, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (issuer) DO UPDATE
             SET client_id = excluded.client_id, client_secret = excluded.client_secret",
            params![
                id.as_str(),
                issuer,
                client.provider.discovery_url(),
                client.id,
                sealed,
                now_ms()
            ],
        )?;
        Ok(())
    })
}

/// Reads the provider's client, `None` when none is registered. Provider
/// rows belong to the instance, so no scope is asked.
///
/// # Errors
///
/// Returns [`StoreError::Tampered`] when the sealed secret is missing or
/// does not open; database failures pass through.
pub fn client(
    store: &Store,
    keys: &Keys,
    provider: OauthProvider,
) -> Result<Option<OauthClient>, StoreError> {
    let issuer = provider.issuer();
    let stored: Option<(String, Option<Vec<u8>>)> = store.read(|connection| {
        connection
            .query_row(
                "SELECT client_id, client_secret FROM auth_providers WHERE issuer = ?1",
                [issuer],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(StoreError::from)
    })?;
    let Some((id, blob)) = stored else {
        return Ok(None);
    };
    let secret = blob
        .and_then(|blob| sealed::open(keys.providers(), issuer.as_bytes(), &blob))
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .ok_or(StoreError::Tampered)?;
    Ok(Some(OauthClient {
        provider,
        id,
        secret,
    }))
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

/// The registered clients without their secrets, by issuer. Requires the
/// instance-admin flag.
///
/// # Errors
///
/// Returns [`StoreError::Forbidden`] without the flag; database failures
/// pass through.
pub fn list(store: &Store, scope: &Scope) -> Result<Vec<RegisteredClient>, StoreError> {
    scope.require_instance_admin()?;
    store.read(|connection| {
        let mut statement =
            connection.prepare("SELECT issuer, client_id FROM auth_providers ORDER BY issuer")?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows
            .into_iter()
            .filter_map(|(issuer, id)| {
                OauthProvider::from_issuer(&issuer)
                    .map(|provider| RegisteredClient { provider, id })
            })
            .collect())
    })
}

#[cfg(test)]
mod tests {
    use url::Url;

    use super::*;
    use crate::identity;
    use crate::scope;
    use crate::secrets::InstanceSecret;

    const SECRET: &str = "GOCSPX-not-a-real-secret";

    fn keys() -> Keys {
        Keys::derive(
            &InstanceSecret::from_bytes(b"0123456789abcdef0123456789abcdef".to_vec()).unwrap(),
        )
    }

    /// An owner without the flag or the same owner with it.
    fn store_with_owner(instance_admin: bool) -> (Store, Scope) {
        let store = Store::in_memory().unwrap();
        let (_, owner) = identity::create_personal_user(&store, "mira@example.com").unwrap();
        if instance_admin {
            identity::grant_instance_admin(&store, "mira@example.com").unwrap();
        }
        let scope = scope::resolve(&store, &owner.id, None).unwrap();
        (store, scope)
    }

    fn google() -> OauthClient {
        OauthClient {
            provider: OauthProvider::Google,
            id: "client-id".to_owned(),
            secret: SECRET.to_owned(),
        }
    }

    fn secret_bytes(store: &Store) -> Vec<u8> {
        store
            .read(|connection| {
                connection
                    .query_row("SELECT client_secret FROM auth_providers", [], |row| {
                        row.get(0)
                    })
                    .map_err(StoreError::from)
            })
            .unwrap()
    }

    #[test]
    fn an_owner_without_the_flag_registers_nothing_and_lists_nothing() {
        let (store, scope) = store_with_owner(false);
        let result = set_client(&store, &keys(), &scope, &google());
        assert!(matches!(result, Err(StoreError::Forbidden)));
        assert!(matches!(list(&store, &scope), Err(StoreError::Forbidden)));
        assert_eq!(
            client(&store, &keys(), OauthProvider::Google).unwrap(),
            None
        );
        assert!(!is_registered(&store, OauthProvider::Google).unwrap());
    }

    #[test]
    fn a_secret_sealed_for_google_does_not_open_for_microsoft() {
        let (store, scope) = store_with_owner(true);
        let keys = keys();
        set_client(&store, &keys, &scope, &google()).unwrap();
        store
            .write(|transaction| {
                transaction
                    .execute(
                        "UPDATE auth_providers SET issuer = ?1, discovery_url = ?2",
                        [
                            OauthProvider::Microsoft.issuer(),
                            OauthProvider::Microsoft.discovery_url(),
                        ],
                    )
                    .map_err(StoreError::from)?;
                Ok(())
            })
            .unwrap();
        let result = client(&store, &keys, OauthProvider::Microsoft);
        assert!(matches!(result, Err(StoreError::Tampered)));
        assert_eq!(client(&store, &keys, OauthProvider::Google).unwrap(), None);
    }

    #[test]
    fn another_instance_secret_opens_no_client_secret() {
        let (store, scope) = store_with_owner(true);
        set_client(&store, &keys(), &scope, &google()).unwrap();
        let other = Keys::derive(
            &InstanceSecret::from_bytes(b"fedcba9876543210fedcba9876543210".to_vec()).unwrap(),
        );
        let result = client(&store, &other, OauthProvider::Google);
        assert!(matches!(result, Err(StoreError::Tampered)));
    }

    #[test]
    fn the_row_holds_no_plaintext_secret_and_debug_leaves_it_out() {
        let (store, scope) = store_with_owner(true);
        set_client(&store, &keys(), &scope, &google()).unwrap();
        let stored = secret_bytes(&store);
        assert!(
            !stored
                .windows(SECRET.len())
                .any(|window| window == SECRET.as_bytes())
        );
        assert_eq!(format!("{:?}", google()), "OauthClient(google, client-id)");
    }

    #[test]
    fn a_registered_client_reads_back_lists_and_is_replaced_by_a_second_write() {
        let (store, scope) = store_with_owner(true);
        let keys = keys();
        assert_eq!(client(&store, &keys, OauthProvider::Google).unwrap(), None);
        set_client(&store, &keys, &scope, &google()).unwrap();
        assert_eq!(
            client(&store, &keys, OauthProvider::Google).unwrap(),
            Some(google())
        );
        assert!(is_registered(&store, OauthProvider::Google).unwrap());
        let replaced = OauthClient {
            id: "other-id".to_owned(),
            secret: "GOCSPX-another-one".to_owned(),
            ..google()
        };
        set_client(&store, &keys, &scope, &replaced).unwrap();
        assert_eq!(
            client(&store, &keys, OauthProvider::Google).unwrap(),
            Some(replaced)
        );
        assert_eq!(
            client(&store, &keys, OauthProvider::Microsoft).unwrap(),
            None
        );
        assert_eq!(
            list(&store, &scope).unwrap(),
            [RegisteredClient {
                provider: OauthProvider::Google,
                id: "other-id".to_owned(),
            }]
        );
    }

    #[test]
    fn the_endpoints_are_https_and_the_discovery_url_hangs_off_the_issuer() {
        for provider in ALL {
            assert_eq!(
                provider.discovery_url(),
                format!("{}/.well-known/openid-configuration", provider.issuer())
            );
            for text in [provider.authorization_url(), provider.token_url()] {
                let url = Url::parse(text).unwrap();
                assert_eq!(url.scheme(), "https", "{text}");
            }
            assert!(!provider.scopes().is_empty());
            assert_eq!(OauthProvider::from_word(provider.as_str()), Some(provider));
        }
        assert_eq!(OauthProvider::from_word("yahoo"), None);
        assert!(
            OauthProvider::Microsoft
                .scopes()
                .contains(&"offline_access")
        );
        assert!(OauthProvider::Microsoft.extra_params().is_empty());
        assert_eq!(OauthProvider::Google.extra_params().len(), 2);
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
