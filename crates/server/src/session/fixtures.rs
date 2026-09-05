// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Test fixtures shared by the session modules.

use rusqlite::params;

use super::SessionTimeouts;
use crate::identity;
use crate::ids::UserId;
use crate::secrets::{InstanceSecret, Keys};
use crate::store::{Store, StoreError};

pub(crate) const GENEROUS_TIMEOUTS: SessionTimeouts = SessionTimeouts {
    idle_ms: 3_600_000,
    absolute_ms: 3_600_000,
};

pub(crate) fn keys() -> Keys {
    Keys::derive(&InstanceSecret::from_bytes(b"0123456789abcdef0123456789abcdef".to_vec()).unwrap())
}

pub(crate) fn store_with_user() -> (Store, UserId) {
    let store = Store::in_memory().unwrap();
    let (_, user) = identity::create_personal_user(&store, "mira@example.com").unwrap();
    (store, user.id)
}

/// Ages every session's last activity by `by_ms`.
pub(crate) fn age_idle(store: &Store, by_ms: i64) {
    let sql = format!("UPDATE sessions SET last_seen_at = last_seen_at - {by_ms}");
    store
        .write(|transaction| {
            transaction.execute(&sql, []).map_err(StoreError::from)?;
            Ok(())
        })
        .unwrap();
}

pub(crate) fn session_rows(store: &Store) -> i64 {
    store
        .read(|connection| {
            connection
                .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
                .map_err(StoreError::from)
        })
        .unwrap()
}

/// Puts a one-time password on the user's row; the hash itself never
/// matters to the session, only that one is live until `expires_at`.
pub(crate) fn hand_out_one_time_password(store: &Store, user_id: &UserId, expires_at: i64) {
    store
        .write(|transaction| {
            transaction
                .execute(
                    "UPDATE users SET password_hash = 'one-time', password_reset_expires_at = ?1
                     WHERE id = ?2",
                    params![expires_at, user_id.as_str()],
                )
                .map_err(StoreError::from)?;
            Ok(())
        })
        .unwrap();
}

pub(crate) fn password_hash_of(store: &Store, user_id: &UserId) -> Option<String> {
    store
        .read(|connection| {
            connection
                .query_row(
                    "SELECT password_hash FROM users WHERE id = ?1",
                    [user_id.as_str()],
                    |row| row.get(0),
                )
                .map_err(StoreError::from)
        })
        .unwrap()
}
