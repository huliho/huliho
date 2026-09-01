// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Test fixtures shared by the session modules.

use super::SessionTimeouts;
use crate::identity;
use crate::ids::UserId;
use crate::secrets::{InstanceSecret, SessionKeys};
use crate::store::{Store, StoreError};

pub(crate) const GENEROUS_TIMEOUTS: SessionTimeouts = SessionTimeouts {
    idle_ms: 3_600_000,
    absolute_ms: 3_600_000,
};

pub(crate) fn keys() -> SessionKeys {
    SessionKeys::derive(&InstanceSecret::for_tests(
        b"0123456789abcdef0123456789abcdef",
    ))
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
