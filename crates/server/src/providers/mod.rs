// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The sign-in providers an instance can hold a client for; the row in
//! `auth_providers` is that client, its secret sealed under the issuer so
//! it opens for no other provider.

mod endpoints;

use std::fmt;

use rusqlite::{OptionalExtension, params};

pub use endpoints::OauthProvider;

use crate::events::{Actor, DomainEvent, append};
use crate::ids::ProviderId;
use crate::scope::Scope;
use crate::sealed;
use crate::secrets::Keys;
use crate::store::{Store, StoreError, now_ms};

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

/// Registers or replaces the provider's client and appends the fact to
/// the acting user's organization. Requires the instance-admin flag.
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
    let actor = Actor::User(scope.user_id().clone());
    let event = DomainEvent::ProviderClientUpdated {
        provider: client.provider,
    };
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
        append(transaction, scope.organization_id(), &actor, &event)
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

/// The providers the instance holds a client for, Google first.
/// Provider rows belong to the instance, so no scope is asked.
///
/// # Errors
///
/// Returns an error when the database fails.
pub fn registered(store: &Store) -> Result<Vec<OauthProvider>, StoreError> {
    let issuers: Vec<String> = store.read(|connection| {
        let mut statement = connection.prepare("SELECT issuer FROM auth_providers")?;
        let rows = statement
            .query_map([], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })?;
    Ok(endpoints::ALL
        .into_iter()
        .filter(|provider| issuers.iter().any(|issuer| issuer == provider.issuer()))
        .collect())
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
    use super::*;
    use crate::events;
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

    /// The `provider.*` rows of the owner's organization, as actor and
    /// payload pairs.
    fn provider_events(store: &Store, scope: &Scope) -> Vec<(String, String)> {
        events::for_organization(store, scope)
            .unwrap()
            .into_iter()
            .filter(|record| record.event_type.starts_with("provider."))
            .map(|record| (record.actor, record.payload))
            .collect()
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
        assert!(registered(&store).unwrap().is_empty());
        assert!(provider_events(&store, &scope).is_empty());
    }

    #[test]
    fn every_write_appends_the_fact_with_the_provider_word_only() {
        let (store, scope) = store_with_owner(true);
        let keys = keys();
        set_client(&store, &keys, &scope, &google()).unwrap();
        let replaced = OauthClient {
            id: "other-id".to_owned(),
            ..google()
        };
        set_client(&store, &keys, &scope, &replaced).unwrap();
        let actor = scope.user_id().as_str().to_owned();
        let payload = r#"{"provider":"google"}"#.to_owned();
        assert_eq!(
            provider_events(&store, &scope),
            [(actor.clone(), payload.clone()), (actor, payload)]
        );
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
        assert_eq!(registered(&store).unwrap(), [OauthProvider::Google]);
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
}
