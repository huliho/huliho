// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Server-side sessions: opaque tokens over AEAD-sealed rows.

mod activity;
pub mod device;
#[cfg(test)]
pub(crate) mod fixtures;
mod list;

use std::net::IpAddr;

use chacha20poly1305::XNonce;
use chacha20poly1305::aead::{Aead, Generate, Payload};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub use activity::{TOUCH_INTERVAL_MS, prune_expired, prune_periodically, touch};
pub use device::Device;
pub use list::{SessionRow, list, revoke_other, revoke_others};

use crate::config::AuthConfig;
use crate::events::{Actor, DomainEvent, append};
use crate::ids::{OrganizationId, SessionId, UserId};
use crate::secrets::SessionKeys;
use crate::store::{Store, StoreError, now_ms};

use activity::record_activity;

/// Cookie carrying the opaque session token.
pub const SESSION_COOKIE: &str = "huliho_session";

/// 128-bit tokens per the security doctrine.
const TOKEN_BYTES: usize = 16;

/// XChaCha20-Poly1305 prefixes its nonce to the sealed blob.
const NONCE_BYTES: usize = 24;

pub(crate) const MS_PER_MINUTE: i64 = 60_000;

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("the session is missing, expired or revoked")]
    Unauthenticated,
    #[error("the system randomness source failed")]
    Random,
    #[error("sealing the session row failed")]
    Sealing,
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// Idle and absolute limits in milliseconds, from the instance config.
#[derive(Debug, Clone, Copy)]
pub struct SessionTimeouts {
    pub idle_ms: i64,
    pub absolute_ms: i64,
}

impl From<&AuthConfig> for SessionTimeouts {
    fn from(auth: &AuthConfig) -> Self {
        Self {
            idle_ms: i64::from(auth.idle_timeout_minutes) * MS_PER_MINUTE,
            absolute_ms: i64::from(auth.absolute_timeout_minutes) * MS_PER_MINUTE,
        }
    }
}

/// What a row seals: the binding the plain columns cannot prove.
#[derive(Serialize, Deserialize)]
struct SealedSession {
    user_id: UserId,
    created_at: i64,
}

/// Digest of the cookie token; the row key and the AEAD associated data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TokenHash(Vec<u8>);

impl TokenHash {
    fn of(token: &str) -> Self {
        Self(Sha256::digest(token.as_bytes()).to_vec())
    }

    pub(crate) fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

/// Where a session is used from.
#[derive(Debug, Clone, Default)]
pub struct Client {
    pub device: Device,
    pub address: Option<IpAddr>,
}

/// The session behind a cookie token, as [`authenticate`] resolved it.
#[derive(Debug)]
pub struct Session {
    pub id: SessionId,
    pub user_id: UserId,
    pub last_seen_at: i64,
    token_hash: TokenHash,
}

/// Creates a session for the user and returns the cookie token.
///
/// # Errors
///
/// Returns an error when randomness fails, the user is gone or the
/// database fails.
pub fn create(
    store: &Store,
    keys: &SessionKeys,
    user_id: &UserId,
    client: &Client,
) -> Result<String, SessionError> {
    let mut bytes = [0u8; TOKEN_BYTES];
    getrandom::fill(&mut bytes).map_err(|_| SessionError::Random)?;
    let token = base16ct::lower::encode_string(&bytes);
    let token_hash = TokenHash::of(&token);
    let created_at = now_ms();
    let sealed = seal(
        keys,
        &token_hash,
        &SealedSession {
            user_id: user_id.clone(),
            created_at,
        },
    )?;
    let device = serde_json::to_string(&client.device).map_err(StoreError::from)?;
    store.write(|transaction| {
        let organization_id = organization_of(transaction, user_id)?;
        transaction.execute(
            "INSERT INTO sessions
             (token_hash, id, user_id, sealed, device, address, created_at, last_seen_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
            params![
                token_hash.as_slice(),
                SessionId::generate().as_str(),
                user_id.as_str(),
                sealed,
                device,
                client.address.map(|address| address.to_string()),
                created_at
            ],
        )?;
        let actor = Actor::User(user_id.clone());
        append(
            transaction,
            &organization_id,
            &actor,
            &DomainEvent::SessionCreated {},
        )?;
        record_activity(transaction, &organization_id, user_id, created_at)
    })?;
    Ok(token)
}

/// Resolves a cookie token to its session, enforcing both timeouts. The
/// check is read-only: cookies ride along on GET, so expired rows wait
/// for the sweep and activity is recorded by mutations only.
///
/// # Errors
///
/// Returns [`SessionError::Unauthenticated`] for unknown, tampered or
/// expired tokens; database failures pass through.
pub fn authenticate(
    store: &Store,
    keys: &SessionKeys,
    timeouts: SessionTimeouts,
    token: &str,
) -> Result<Session, SessionError> {
    let token_hash = TokenHash::of(token);
    let session = store.read(|connection| {
        let row: Option<(SessionId, Vec<u8>, i64)> = connection
            .query_row(
                "SELECT id, sealed, last_seen_at FROM sessions WHERE token_hash = ?1",
                [token_hash.as_slice()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((id, sealed, last_seen_at)) = row else {
            return Ok(None);
        };
        let Some(sealed) = open(keys, &token_hash, &sealed) else {
            return Ok(None);
        };
        let now = now_ms();
        let idled_out = now.saturating_sub(last_seen_at) >= timeouts.idle_ms;
        let aged_out = now.saturating_sub(sealed.created_at) >= timeouts.absolute_ms;
        if idled_out || aged_out {
            return Ok(None);
        }
        Ok(Some(Session {
            id,
            user_id: sealed.user_id,
            last_seen_at,
            token_hash: token_hash.clone(),
        }))
    })?;
    session.ok_or(SessionError::Unauthenticated)
}

/// Revokes the session behind a cookie token; unknown tokens are a no-op
/// so logout stays idempotent.
///
/// # Errors
///
/// Returns an error when the database fails.
pub fn revoke(store: &Store, token: &str) -> Result<(), StoreError> {
    let token_hash = TokenHash::of(token);
    store.write(|transaction| {
        let user_id: Option<UserId> = transaction
            .query_row(
                "SELECT user_id FROM sessions WHERE token_hash = ?1",
                [token_hash.as_slice()],
                |row| row.get(0),
            )
            .optional()?;
        let Some(user_id) = user_id else {
            return Ok(());
        };
        transaction.execute(
            "DELETE FROM sessions WHERE token_hash = ?1",
            [token_hash.as_slice()],
        )?;
        let organization_id = organization_of(transaction, &user_id)?;
        let event = DomainEvent::SessionRevoked {
            user_id: user_id.clone(),
        };
        append(transaction, &organization_id, &Actor::User(user_id), &event)
    })
}

fn seal(
    keys: &SessionKeys,
    token_hash: &TokenHash,
    session: &SealedSession,
) -> Result<Vec<u8>, SessionError> {
    let plaintext = serde_json::to_vec(session).map_err(StoreError::from)?;
    let nonce = XNonce::try_generate().map_err(|_| SessionError::Random)?;
    let ciphertext = keys
        .cipher()
        .encrypt(
            &nonce,
            Payload {
                msg: &plaintext,
                aad: token_hash.as_slice(),
            },
        )
        .map_err(|_| SessionError::Sealing)?;
    let mut sealed = nonce.to_vec();
    sealed.extend_from_slice(&ciphertext);
    Ok(sealed)
}

fn open(keys: &SessionKeys, token_hash: &TokenHash, sealed: &[u8]) -> Option<SealedSession> {
    let (nonce, ciphertext) = sealed.split_at_checked(NONCE_BYTES)?;
    let nonce = XNonce::try_from(nonce).ok()?;
    let plaintext = keys
        .cipher()
        .decrypt(
            &nonce,
            Payload {
                msg: ciphertext,
                aad: token_hash.as_slice(),
            },
        )
        .ok()?;
    serde_json::from_slice(&plaintext).ok()
}

fn organization_of(
    connection: &Connection,
    user_id: &UserId,
) -> Result<OrganizationId, StoreError> {
    connection
        .query_row(
            "SELECT organization_id FROM users WHERE id = ?1",
            [user_id.as_str()],
            |row| row.get(0),
        )
        .optional()?
        .ok_or(StoreError::NotFound)
}

#[cfg(test)]
mod tests {
    use super::fixtures::{GENEROUS_TIMEOUTS, age_idle, keys, session_rows, store_with_user};
    use super::*;
    use crate::secrets::InstanceSecret;

    #[test]
    fn a_created_session_authenticates_to_its_user() {
        let (store, user_id) = store_with_user();
        let keys = keys();
        let token = create(&store, &keys, &user_id, &Client::default()).unwrap();
        let resolved = authenticate(&store, &keys, GENEROUS_TIMEOUTS, &token).unwrap();
        assert_eq!(resolved.user_id, user_id);
        assert!(!resolved.id.as_str().is_empty());
    }

    #[test]
    fn an_unknown_token_is_unauthenticated() {
        let (store, _) = store_with_user();
        let result = authenticate(&store, &keys(), GENEROUS_TIMEOUTS, "not a token");
        assert!(matches!(result, Err(SessionError::Unauthenticated)));
    }

    #[test]
    fn a_revoked_session_stops_authenticating() {
        let (store, user_id) = store_with_user();
        let keys = keys();
        let token = create(&store, &keys, &user_id, &Client::default()).unwrap();
        revoke(&store, &token).unwrap();
        let result = authenticate(&store, &keys, GENEROUS_TIMEOUTS, &token);
        assert!(matches!(result, Err(SessionError::Unauthenticated)));
        revoke(&store, &token).unwrap();
    }

    #[test]
    fn an_idle_session_expires() {
        let (store, user_id) = store_with_user();
        let keys = keys();
        let token = create(&store, &keys, &user_id, &Client::default()).unwrap();
        age_idle(&store, GENEROUS_TIMEOUTS.idle_ms);
        let result = authenticate(&store, &keys, GENEROUS_TIMEOUTS, &token);
        assert!(matches!(result, Err(SessionError::Unauthenticated)));
        assert_eq!(session_rows(&store), 1);
    }

    #[test]
    fn an_aged_out_session_expires_even_while_active() {
        let (store, user_id) = store_with_user();
        let keys = keys();
        let token = create(&store, &keys, &user_id, &Client::default()).unwrap();
        let spent = SessionTimeouts {
            idle_ms: GENEROUS_TIMEOUTS.idle_ms,
            absolute_ms: 0,
        };
        let result = authenticate(&store, &keys, spent, &token);
        assert!(matches!(result, Err(SessionError::Unauthenticated)));
    }

    #[test]
    fn a_tampered_row_is_unauthenticated() {
        let (store, user_id) = store_with_user();
        let keys = keys();
        let token = create(&store, &keys, &user_id, &Client::default()).unwrap();
        store
            .write(|transaction| {
                transaction
                    .execute("UPDATE sessions SET sealed = X'00'", [])
                    .map_err(StoreError::from)?;
                Ok(())
            })
            .unwrap();
        let result = authenticate(&store, &keys, GENEROUS_TIMEOUTS, &token);
        assert!(matches!(result, Err(SessionError::Unauthenticated)));
    }

    #[test]
    fn another_instance_secret_opens_nothing() {
        let (store, user_id) = store_with_user();
        let token = create(&store, &keys(), &user_id, &Client::default()).unwrap();
        let other = SessionKeys::derive(&InstanceSecret::for_tests(
            b"fedcba9876543210fedcba9876543210",
        ));
        let result = authenticate(&store, &other, GENEROUS_TIMEOUTS, &token);
        assert!(matches!(result, Err(SessionError::Unauthenticated)));
    }

    #[test]
    fn the_client_record_lands_on_the_row() {
        let (store, user_id) = store_with_user();
        let client = Client {
            device: device::from_user_agent("Mozilla/5.0 (X11; Linux x86_64) Firefox/128.0", true),
            address: Some("203.0.113.7".parse().unwrap()),
        };
        create(&store, &keys(), &user_id, &client).unwrap();
        let (device, address): (Device, Option<String>) = store
            .read(|connection| {
                connection
                    .query_row("SELECT device, address FROM sessions", [], |row| {
                        Ok((row.get(0)?, row.get(1)?))
                    })
                    .map_err(StoreError::from)
            })
            .unwrap();
        assert_eq!(device, client.device);
        assert_eq!(address.as_deref(), Some("203.0.113.7"));
    }

    #[test]
    fn session_lifecycle_lands_in_the_event_log() {
        let (store, user_id) = store_with_user();
        let keys = keys();
        let token = create(&store, &keys, &user_id, &Client::default()).unwrap();
        revoke(&store, &token).unwrap();
        let scope = crate::scope::resolve(&store, &user_id, None).unwrap();
        let events = crate::events::for_organization(&store, &scope).unwrap();
        let types: Vec<&str> = events
            .iter()
            .map(|event| event.event_type.as_str())
            .collect();
        assert!(types.contains(&"session.created"));
        assert!(types.contains(&"session.revoked"));
        assert!(types.contains(&"user.active"));
    }
}
