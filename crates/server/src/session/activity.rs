// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Activity on a session: the last-seen stamp, the monthly active fact
//! and the sweep that ends expired sessions.

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use rusqlite::{Connection, params};
use time::OffsetDateTime;

use super::{MS_PER_MINUTE, Session, SessionTimeouts};
use crate::events::{Actor, DomainEvent, append};
use crate::ids::{OrganizationId, UserId};
use crate::scope::Scope;
use crate::store::{MS_PER_SECOND, Store, StoreError, now_ms};

/// A stamp per mutation would write on every keystroke; five minutes is
/// finer than any display of "last seen".
pub const TOUCH_INTERVAL_MS: i64 = 5 * MS_PER_MINUTE;

/// Expiry is minute-grained at its finest, so a daily sweep suffices.
const PRUNE_INTERVAL: Duration = Duration::from_hours(24);

/// Records that the session is in use from `address` and keeps the
/// user's activity facts current, at most once per [`TOUCH_INTERVAL_MS`].
///
/// # Errors
///
/// Returns an error when the database fails.
pub fn touch(
    store: &Store,
    scope: &Scope,
    session: &Session,
    address: Option<IpAddr>,
) -> Result<(), StoreError> {
    let now = now_ms();
    if now.saturating_sub(session.last_seen_at) < TOUCH_INTERVAL_MS {
        return Ok(());
    }
    store.write(|transaction| {
        transaction.execute(
            "UPDATE sessions SET last_seen_at = ?1, address = ?2 WHERE token_hash = ?3",
            params![
                now,
                address.map(|address| address.to_string()),
                session.token_hash.as_slice()
            ],
        )?;
        record_activity(transaction, scope.organization_id(), scope.user_id(), now)
    })
}

/// Stamps the user's last activity and appends the `user.active` fact
/// once per UTC calendar month, the unit metering counts in.
pub(crate) fn record_activity(
    connection: &Connection,
    organization_id: &OrganizationId,
    user_id: &UserId,
    now: i64,
) -> Result<(), StoreError> {
    connection.execute(
        "UPDATE users SET last_active_at = ?1 WHERE id = ?2",
        params![now, user_id.as_str()],
    )?;
    let (period, period_start) = period_of(now);
    let recorded: bool = connection.query_row(
        "SELECT EXISTS (
             SELECT 1 FROM domain_events
             WHERE organization_id = ?1 AND actor = ?2
               AND event_type = 'user.active' AND created_at >= ?3
         )",
        params![organization_id.as_str(), user_id.as_str(), period_start],
        |row| row.get(0),
    )?;
    if recorded {
        return Ok(());
    }
    let event = DomainEvent::UserActive {
        user_id: user_id.clone(),
        period,
    };
    append(
        connection,
        organization_id,
        &Actor::User(user_id.clone()),
        &event,
    )
}

/// The UTC calendar month holding `now_ms`, as its label and first
/// millisecond.
fn period_of(now_ms: i64) -> (String, i64) {
    let date = OffsetDateTime::from_unix_timestamp(now_ms.div_euclid(MS_PER_SECOND))
        .unwrap_or(OffsetDateTime::UNIX_EPOCH)
        .date();
    let first = date.replace_day(1).unwrap_or(date);
    let label = format!("{:04}-{:02}", first.year(), u8::from(first.month()));
    let start = first
        .midnight()
        .assume_utc()
        .unix_timestamp()
        .saturating_mul(MS_PER_SECOND);
    (label, start)
}

/// Removes every row past either timeout, recording each as expired.
///
/// # Errors
///
/// Returns an error when the database fails; the transaction then rolls
/// back and nothing is removed.
pub fn prune_expired(store: &Store, timeouts: SessionTimeouts) -> Result<u64, StoreError> {
    store.write(|transaction| {
        let expired = expired_rows(transaction, now_ms(), timeouts)?;
        for (token_hash, user_id, organization_id) in &expired {
            transaction.execute(
                "DELETE FROM sessions WHERE token_hash = ?1",
                [token_hash.as_slice()],
            )?;
            let event = DomainEvent::SessionExpired {
                user_id: user_id.clone(),
            };
            append(transaction, organization_id, &Actor::System, &event)?;
        }
        Ok(u64::try_from(expired.len()).unwrap_or(u64::MAX))
    })
}

type ExpiredRow = (Vec<u8>, UserId, OrganizationId);

fn expired_rows(
    connection: &Connection,
    now: i64,
    timeouts: SessionTimeouts,
) -> Result<Vec<ExpiredRow>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT s.token_hash, s.user_id, u.organization_id
         FROM sessions s JOIN users u ON u.id = s.user_id
         WHERE s.last_seen_at <= ?1 OR s.created_at <= ?2",
    )?;
    let rows = statement
        .query_map(
            params![
                now.saturating_sub(timeouts.idle_ms),
                now.saturating_sub(timeouts.absolute_ms)
            ],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Sweeps expired sessions at startup and then daily; failures are
/// logged and retried at the next tick.
pub async fn prune_periodically(store: Arc<Store>, timeouts: SessionTimeouts) {
    let mut interval = tokio::time::interval(PRUNE_INTERVAL);
    loop {
        interval.tick().await;
        let store = Arc::clone(&store);
        match tokio::task::spawn_blocking(move || prune_expired(&store, timeouts)).await {
            Ok(Ok(0)) => {}
            Ok(Ok(removed)) => tracing::info!(removed, "pruned expired sessions"),
            Ok(Err(error)) => tracing::warn!(%error, "session pruning failed"),
            Err(error) => tracing::warn!(%error, "session pruning task failed"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::fixtures::{
        GENEROUS_TIMEOUTS, age_idle, keys, session_rows, store_with_user,
    };
    use crate::session::{Client, authenticate, create};
    use crate::{events, scope};

    /// 2026-09-15T12:00:00Z in milliseconds.
    const MID_SEPTEMBER_2026_MS: i64 = 1_789_473_600_000;

    /// 2026-09-01T00:00:00Z in milliseconds.
    const SEPTEMBER_2026_START_MS: i64 = 1_788_220_800_000;

    const ADDRESS: &str = "203.0.113.7";

    fn last_seen_and_address(store: &Store) -> (i64, Option<String>) {
        store
            .read(|connection| {
                connection
                    .query_row("SELECT last_seen_at, address FROM sessions", [], |row| {
                        Ok((row.get(0)?, row.get(1)?))
                    })
                    .map_err(StoreError::from)
            })
            .unwrap()
    }

    fn event_types(store: &Store, user_id: &UserId) -> Vec<String> {
        let scope = scope::resolve(store, user_id, None).unwrap();
        events::for_organization(store, &scope)
            .unwrap()
            .into_iter()
            .map(|record| record.event_type)
            .collect()
    }

    #[test]
    fn period_of_names_the_utc_month_and_its_start() {
        assert_eq!(
            period_of(MID_SEPTEMBER_2026_MS),
            ("2026-09".to_owned(), SEPTEMBER_2026_START_MS)
        );
        assert_eq!(
            period_of(SEPTEMBER_2026_START_MS).1,
            SEPTEMBER_2026_START_MS
        );
    }

    #[test]
    fn a_touch_inside_the_interval_writes_nothing() {
        let (store, user_id) = store_with_user();
        let keys = keys();
        let token = create(&store, &keys, &user_id, &Client::default()).unwrap();
        let before = last_seen_and_address(&store);
        let session = authenticate(&store, &keys, GENEROUS_TIMEOUTS, &token).unwrap();
        let scope = scope::resolve(&store, &user_id, None).unwrap();
        touch(&store, &scope, &session, Some(ADDRESS.parse().unwrap())).unwrap();
        assert_eq!(last_seen_and_address(&store), before);
    }

    #[test]
    fn a_touch_past_the_interval_stamps_session_and_user() {
        let (store, user_id) = store_with_user();
        let keys = keys();
        let token = create(&store, &keys, &user_id, &Client::default()).unwrap();
        age_idle(&store, TOUCH_INTERVAL_MS);
        let session = authenticate(&store, &keys, GENEROUS_TIMEOUTS, &token).unwrap();
        let scope = scope::resolve(&store, &user_id, None).unwrap();
        touch(&store, &scope, &session, Some(ADDRESS.parse().unwrap())).unwrap();
        let (last_seen_at, address) = last_seen_and_address(&store);
        assert!(last_seen_at > session.last_seen_at);
        assert_eq!(address.as_deref(), Some(ADDRESS));
        let user = crate::identity::user(&store, &scope).unwrap();
        assert_eq!(user.last_active_at, Some(last_seen_at));
    }

    #[test]
    fn the_period_fact_lands_once_per_month() {
        let (store, user_id) = store_with_user();
        let keys = keys();
        let token = create(&store, &keys, &user_id, &Client::default()).unwrap();
        create(&store, &keys, &user_id, &Client::default()).unwrap();
        age_idle(&store, TOUCH_INTERVAL_MS);
        let session = authenticate(&store, &keys, GENEROUS_TIMEOUTS, &token).unwrap();
        let scope = scope::resolve(&store, &user_id, None).unwrap();
        touch(&store, &scope, &session, None).unwrap();
        let active = event_types(&store, &user_id)
            .iter()
            .filter(|kind| *kind == "user.active")
            .count();
        assert_eq!(active, 1);
    }

    #[test]
    fn the_sweep_removes_expired_rows_and_records_each() {
        let (store, user_id) = store_with_user();
        let keys = keys();
        create(&store, &keys, &user_id, &Client::default()).unwrap();
        create(&store, &keys, &user_id, &Client::default()).unwrap();
        assert_eq!(prune_expired(&store, GENEROUS_TIMEOUTS).unwrap(), 0);
        age_idle(&store, GENEROUS_TIMEOUTS.idle_ms);
        assert_eq!(prune_expired(&store, GENEROUS_TIMEOUTS).unwrap(), 2);
        assert_eq!(session_rows(&store), 0);
        let expired = event_types(&store, &user_id)
            .iter()
            .filter(|kind| *kind == "session.expired")
            .count();
        assert_eq!(expired, 2);
    }
}
