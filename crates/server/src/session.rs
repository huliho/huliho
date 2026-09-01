// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Server-side sessions: opaque tokens over AEAD-sealed rows.

use chacha20poly1305::XNonce;
use chacha20poly1305::aead::{Aead, Generate, Payload};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::config::AuthConfig;
use crate::events::{Actor, DomainEvent, append};
use crate::ids::{OrganizationId, UserId};
use crate::secrets::SessionKeys;
use crate::store::{Store, StoreError, now_ms};

/// Cookie carrying the opaque session token.
pub const SESSION_COOKIE: &str = "huliho_session";

/// 128-bit tokens per the security doctrine.
const TOKEN_BYTES: usize = 16;

/// XChaCha20-Poly1305 prefixes its nonce to the sealed blob.
const NONCE_BYTES: usize = 24;

/// Expiry is minute-grained at its finest, so a daily sweep suffices.
const PRUNE_INTERVAL: std::time::Duration = std::time::Duration::from_hours(24);

const MS_PER_MINUTE: i64 = 60_000;

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

/// Creates a session for the user and returns the cookie token.
///
/// # Errors
///
/// Returns an error when randomness fails, the user is gone or the
/// database fails.
pub fn create(store: &Store, keys: &SessionKeys, user_id: &UserId) -> Result<String, SessionError> {
    let mut bytes = [0u8; TOKEN_BYTES];
    getrandom::fill(&mut bytes).map_err(|_| SessionError::Random)?;
    let token = base16ct::lower::encode_string(&bytes);
    let token_hash = Sha256::digest(token.as_bytes());
    let created_at = now_ms();
    let sealed = seal(
        keys,
        &token_hash,
        &SealedSession {
            user_id: user_id.clone(),
            created_at,
        },
    )?;
    store.write(|transaction| {
        let organization_id = organization_of(transaction, user_id)?;
        transaction.execute(
            "INSERT INTO sessions (token_hash, user_id, sealed, created_at, last_seen_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                token_hash.as_slice(),
                user_id.as_str(),
                sealed,
                created_at,
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
        Ok(())
    })?;
    Ok(token)
}

/// Resolves a cookie token to its user, enforcing both timeouts. The
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
) -> Result<UserId, SessionError> {
    let token_hash = Sha256::digest(token.as_bytes());
    let user_id = store.read(|connection| {
        let row: Option<(Vec<u8>, i64)> = connection
            .query_row(
                "SELECT sealed, last_seen_at FROM sessions WHERE token_hash = ?1",
                [token_hash.as_slice()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((sealed, last_seen_at)) = row else {
            return Ok(None);
        };
        let Some(session) = open(keys, &token_hash, &sealed) else {
            return Ok(None);
        };
        let now = now_ms();
        let idled_out = now.saturating_sub(last_seen_at) >= timeouts.idle_ms;
        let aged_out = now.saturating_sub(session.created_at) >= timeouts.absolute_ms;
        if idled_out || aged_out {
            return Ok(None);
        }
        Ok(Some(session.user_id))
    })?;
    user_id.ok_or(SessionError::Unauthenticated)
}

/// Removes every row past either timeout.
///
/// # Errors
///
/// Returns an error when the database fails; the transaction then rolls
/// back and nothing is removed.
pub fn prune_expired(store: &Store, timeouts: SessionTimeouts) -> Result<u64, StoreError> {
    store.write(|transaction| {
        let now = now_ms();
        let removed = transaction.execute(
            "DELETE FROM sessions WHERE last_seen_at <= ?1 OR created_at <= ?2",
            params![
                now.saturating_sub(timeouts.idle_ms),
                now.saturating_sub(timeouts.absolute_ms)
            ],
        )?;
        Ok(u64::try_from(removed).unwrap_or_default())
    })
}

/// Sweeps expired sessions at startup and then daily; failures are
/// logged and retried at the next tick.
pub async fn prune_periodically(store: std::sync::Arc<Store>, timeouts: SessionTimeouts) {
    let mut interval = tokio::time::interval(PRUNE_INTERVAL);
    loop {
        interval.tick().await;
        let store = std::sync::Arc::clone(&store);
        match tokio::task::spawn_blocking(move || prune_expired(&store, timeouts)).await {
            Ok(Ok(0)) => {}
            Ok(Ok(removed)) => tracing::info!(removed, "pruned expired sessions"),
            Ok(Err(error)) => tracing::warn!(%error, "session pruning failed"),
            Err(error) => tracing::warn!(%error, "session pruning task failed"),
        }
    }
}

/// Revokes the session behind a cookie token; unknown tokens are a no-op
/// so logout stays idempotent.
///
/// # Errors
///
/// Returns an error when the database fails.
pub fn revoke(store: &Store, token: &str) -> Result<(), StoreError> {
    let token_hash = Sha256::digest(token.as_bytes());
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
        let actor = Actor::User(user_id);
        append(
            transaction,
            &organization_id,
            &actor,
            &DomainEvent::SessionRevoked {},
        )?;
        Ok(())
    })
}

fn seal(
    keys: &SessionKeys,
    token_hash: &[u8],
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
                aad: token_hash,
            },
        )
        .map_err(|_| SessionError::Sealing)?;
    let mut sealed = nonce.to_vec();
    sealed.extend_from_slice(&ciphertext);
    Ok(sealed)
}

fn open(keys: &SessionKeys, token_hash: &[u8], sealed: &[u8]) -> Option<SealedSession> {
    let (nonce, ciphertext) = sealed.split_at_checked(NONCE_BYTES)?;
    let nonce = XNonce::try_from(nonce).ok()?;
    let plaintext = keys
        .cipher()
        .decrypt(
            &nonce,
            Payload {
                msg: ciphertext,
                aad: token_hash,
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
    use super::*;
    use crate::identity;
    use crate::secrets::InstanceSecret;

    const GENEROUS_TIMEOUTS: SessionTimeouts = SessionTimeouts {
        idle_ms: 3_600_000,
        absolute_ms: 3_600_000,
    };

    fn keys() -> SessionKeys {
        SessionKeys::derive(&InstanceSecret::for_tests(
            b"0123456789abcdef0123456789abcdef",
        ))
    }

    fn store_with_user() -> (Store, UserId) {
        let store = Store::in_memory().unwrap();
        let (_, user) = identity::create_personal_user(&store, "mira@example.com").unwrap();
        (store, user.id)
    }

    fn age_idle(store: &Store, by_ms: i64) {
        let sql = format!("UPDATE sessions SET last_seen_at = last_seen_at - {by_ms}");
        store
            .write(|transaction| {
                transaction.execute(&sql, []).map_err(StoreError::from)?;
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn a_created_session_authenticates_to_its_user() {
        let (store, user_id) = store_with_user();
        let keys = keys();
        let token = create(&store, &keys, &user_id).unwrap();
        let resolved = authenticate(&store, &keys, GENEROUS_TIMEOUTS, &token).unwrap();
        assert_eq!(resolved, user_id);
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
        let token = create(&store, &keys, &user_id).unwrap();
        revoke(&store, &token).unwrap();
        let result = authenticate(&store, &keys, GENEROUS_TIMEOUTS, &token);
        assert!(matches!(result, Err(SessionError::Unauthenticated)));
        revoke(&store, &token).unwrap();
    }

    fn session_rows(store: &Store) -> i64 {
        store
            .read(|connection| {
                connection
                    .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
                    .map_err(StoreError::from)
            })
            .unwrap()
    }

    #[test]
    fn an_idle_session_expires_and_the_sweep_removes_it() {
        let (store, user_id) = store_with_user();
        let keys = keys();
        let token = create(&store, &keys, &user_id).unwrap();
        age_idle(&store, GENEROUS_TIMEOUTS.idle_ms);
        let result = authenticate(&store, &keys, GENEROUS_TIMEOUTS, &token);
        assert!(matches!(result, Err(SessionError::Unauthenticated)));
        assert_eq!(session_rows(&store), 1);
        assert_eq!(prune_expired(&store, GENEROUS_TIMEOUTS).unwrap(), 1);
        assert_eq!(session_rows(&store), 0);
    }

    #[test]
    fn the_sweep_keeps_live_sessions() {
        let (store, user_id) = store_with_user();
        let keys = keys();
        let token = create(&store, &keys, &user_id).unwrap();
        assert_eq!(prune_expired(&store, GENEROUS_TIMEOUTS).unwrap(), 0);
        assert!(authenticate(&store, &keys, GENEROUS_TIMEOUTS, &token).is_ok());
    }

    #[test]
    fn an_aged_out_session_expires_even_while_active() {
        let (store, user_id) = store_with_user();
        let keys = keys();
        let token = create(&store, &keys, &user_id).unwrap();
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
        let token = create(&store, &keys, &user_id).unwrap();
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
        let token = create(&store, &keys(), &user_id).unwrap();
        let other = SessionKeys::derive(&InstanceSecret::for_tests(
            b"fedcba9876543210fedcba9876543210",
        ));
        let result = authenticate(&store, &other, GENEROUS_TIMEOUTS, &token);
        assert!(matches!(result, Err(SessionError::Unauthenticated)));
    }

    #[test]
    fn session_lifecycle_lands_in_the_event_log() {
        let (store, user_id) = store_with_user();
        let keys = keys();
        let token = create(&store, &keys, &user_id).unwrap();
        revoke(&store, &token).unwrap();
        let scope = crate::scope::resolve(&store, &user_id, None).unwrap();
        let events = crate::events::for_organization(&store, &scope).unwrap();
        let types: Vec<&str> = events
            .iter()
            .map(|event| event.event_type.as_str())
            .collect();
        assert!(types.contains(&"session.created"));
        assert!(types.contains(&"session.revoked"));
    }
}
