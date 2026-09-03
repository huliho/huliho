// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A user's own sessions: listing them and ending the others.

use rusqlite::{Connection, Row, params};
use serde::Serialize;

use super::{Device, Session, SessionTimeouts};
use crate::events::{Actor, DomainEvent, append};
use crate::ids::{OrganizationId, SessionId, UserId};
use crate::scope::Scope;
use crate::store::{Store, StoreError, now_ms};

/// One session as the list shows it; the wire shape of `GET /sessions`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRow {
    pub id: SessionId,
    pub current: bool,
    pub device: Device,
    pub address: Option<String>,
    pub created_at: i64,
    pub last_seen_at: i64,
}

/// Lists the scope user's live sessions, the current one first and the
/// rest by last activity. Rows past either timeout are left out even
/// before the sweep removes them.
///
/// # Errors
///
/// Returns an error when the database fails or a device record does
/// not decode.
pub fn list(
    store: &Store,
    scope: &Scope,
    current: &Session,
    timeouts: SessionTimeouts,
) -> Result<Vec<SessionRow>, StoreError> {
    let now = now_ms();
    let mut sessions = store.read(|connection| {
        let mut statement = connection.prepare(
            "SELECT id, device, address, created_at, last_seen_at FROM sessions
             WHERE user_id = ?1 AND last_seen_at > ?2 AND created_at > ?3
             ORDER BY last_seen_at DESC, id",
        )?;
        let rows = statement
            .query_map(
                params![
                    scope.user_id().as_str(),
                    now.saturating_sub(timeouts.idle_ms),
                    now.saturating_sub(timeouts.absolute_ms)
                ],
                |row| session_row(row, &current.id),
            )?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })?;
    sessions.sort_by_key(|row| !row.current);
    Ok(sessions)
}

/// Ends one of the scope user's other sessions.
///
/// # Errors
///
/// Returns [`StoreError::CurrentSession`] for the session making the
/// call and [`StoreError::NotFound`] for an id outside the user's rows.
pub fn revoke_other(
    store: &Store,
    scope: &Scope,
    current: &Session,
    id: &SessionId,
) -> Result<(), StoreError> {
    if *id == current.id {
        return Err(StoreError::CurrentSession);
    }
    store.write(|transaction| {
        let removed = transaction.execute(
            "DELETE FROM sessions WHERE id = ?1 AND user_id = ?2",
            [id.as_str(), scope.user_id().as_str()],
        )?;
        if removed == 0 {
            return Err(StoreError::NotFound);
        }
        record_revocation(transaction, scope)
    })
}

/// Ends every session of the scope user except the current one.
///
/// # Errors
///
/// Returns an error when the database fails.
pub fn revoke_others(store: &Store, scope: &Scope, current: &Session) -> Result<(), StoreError> {
    store.write(|transaction| end_others(transaction, scope, &current.id))
}

/// Ends every session of the scope user except `kept`, one revocation
/// event per row.
pub(super) fn end_others(
    connection: &Connection,
    scope: &Scope,
    kept: &SessionId,
) -> Result<(), StoreError> {
    let removed = connection.execute(
        "DELETE FROM sessions WHERE user_id = ?1 AND id <> ?2",
        [scope.user_id().as_str(), kept.as_str()],
    )?;
    for _ in 0..removed {
        record_revocation(connection, scope)?;
    }
    Ok(())
}

/// Ends every session of `target`, one revocation event per row under
/// `actor`; an admin reset acts on another user's rows.
pub(crate) fn revoke_all(
    connection: &Connection,
    organization_id: &OrganizationId,
    actor: &Actor,
    target: &UserId,
) -> Result<(), StoreError> {
    let removed =
        connection.execute("DELETE FROM sessions WHERE user_id = ?1", [target.as_str()])?;
    for _ in 0..removed {
        let event = DomainEvent::SessionRevoked {
            user_id: target.clone(),
        };
        append(connection, organization_id, actor, &event)?;
    }
    Ok(())
}

fn record_revocation(connection: &Connection, scope: &Scope) -> Result<(), StoreError> {
    let event = DomainEvent::SessionRevoked {
        user_id: scope.user_id().clone(),
    };
    let actor = Actor::User(scope.user_id().clone());
    append(connection, scope.organization_id(), &actor, &event)
}

fn session_row(row: &Row<'_>, current: &SessionId) -> rusqlite::Result<SessionRow> {
    let id: SessionId = row.get(0)?;
    Ok(SessionRow {
        current: id == *current,
        id,
        device: row.get(1)?,
        address: row.get(2)?,
        created_at: row.get(3)?,
        last_seen_at: row.get(4)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity;
    use crate::scope;
    use crate::session::fixtures::{GENEROUS_TIMEOUTS, keys, session_rows, store_with_user};
    use crate::session::{Client, SessionError, authenticate, create, device};

    const FIREFOX: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0";

    fn firefox() -> Client {
        Client {
            device: device::from_user_agent(FIREFOX, false),
            address: None,
        }
    }

    /// Ages one session's last activity by `by_ms`.
    fn age_session(store: &Store, id: &SessionId, by_ms: i64) {
        store
            .write(|transaction| {
                transaction
                    .execute(
                        "UPDATE sessions SET last_seen_at = last_seen_at - ?1 WHERE id = ?2",
                        params![by_ms, id.as_str()],
                    )
                    .map_err(StoreError::from)?;
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn the_list_puts_the_current_session_first_and_marks_it() {
        let (store, user_id) = store_with_user();
        let keys = keys();
        let older = create(&store, &keys, &user_id, &firefox()).unwrap();
        let newer = create(&store, &keys, &user_id, &Client::default()).unwrap();
        let current = authenticate(&store, &keys, GENEROUS_TIMEOUTS, &older).unwrap();
        let scope = scope::resolve(&store, &user_id, None).unwrap();
        let rows = list(&store, &scope, &current, GENEROUS_TIMEOUTS).unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows[0].current);
        assert_eq!(rows[0].id, current.id);
        assert_eq!(rows[0].device.browser.as_deref(), Some("Firefox"));
        assert!(!rows[1].current);
        let other = authenticate(&store, &keys, GENEROUS_TIMEOUTS, &newer).unwrap();
        assert_eq!(rows[1].id, other.id);
    }

    #[test]
    fn the_list_stays_inside_the_own_user() {
        let (store, user_id) = store_with_user();
        let keys = keys();
        let (_, other) = identity::create_personal_user(&store, "noor@example.com").unwrap();
        create(&store, &keys, &other.id, &Client::default()).unwrap();
        let token = create(&store, &keys, &user_id, &Client::default()).unwrap();
        let current = authenticate(&store, &keys, GENEROUS_TIMEOUTS, &token).unwrap();
        let scope = scope::resolve(&store, &user_id, None).unwrap();
        let rows = list(&store, &scope, &current, GENEROUS_TIMEOUTS).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, current.id);
    }

    #[test]
    fn the_list_leaves_out_expired_rows_before_the_sweep() {
        let (store, user_id) = store_with_user();
        let keys = keys();
        let stale = create(&store, &keys, &user_id, &Client::default()).unwrap();
        let stale = authenticate(&store, &keys, GENEROUS_TIMEOUTS, &stale).unwrap();
        let token = create(&store, &keys, &user_id, &Client::default()).unwrap();
        let current = authenticate(&store, &keys, GENEROUS_TIMEOUTS, &token).unwrap();
        age_session(&store, &stale.id, GENEROUS_TIMEOUTS.idle_ms);
        let scope = scope::resolve(&store, &user_id, None).unwrap();
        let rows = list(&store, &scope, &current, GENEROUS_TIMEOUTS).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, current.id);
        assert_eq!(session_rows(&store), 2);
    }

    #[test]
    fn revoking_another_users_session_is_not_found() {
        let (store, user_id) = store_with_user();
        let keys = keys();
        let (_, other) = identity::create_personal_user(&store, "noor@example.com").unwrap();
        let foreign = create(&store, &keys, &other.id, &Client::default()).unwrap();
        let foreign = authenticate(&store, &keys, GENEROUS_TIMEOUTS, &foreign).unwrap();
        let token = create(&store, &keys, &user_id, &Client::default()).unwrap();
        let current = authenticate(&store, &keys, GENEROUS_TIMEOUTS, &token).unwrap();
        let scope = scope::resolve(&store, &user_id, None).unwrap();
        let result = revoke_other(&store, &scope, &current, &foreign.id);
        assert!(matches!(result, Err(StoreError::NotFound)));
        assert_eq!(session_rows(&store), 2);
    }

    #[test]
    fn the_current_session_is_refused() {
        let (store, user_id) = store_with_user();
        let keys = keys();
        let token = create(&store, &keys, &user_id, &Client::default()).unwrap();
        let current = authenticate(&store, &keys, GENEROUS_TIMEOUTS, &token).unwrap();
        let scope = scope::resolve(&store, &user_id, None).unwrap();
        let result = revoke_other(&store, &scope, &current, &current.id);
        assert!(matches!(result, Err(StoreError::CurrentSession)));
        assert_eq!(session_rows(&store), 1);
    }

    #[test]
    fn a_revoked_session_stops_authenticating() {
        let (store, user_id) = store_with_user();
        let keys = keys();
        let phone = create(&store, &keys, &user_id, &Client::default()).unwrap();
        let phone_session = authenticate(&store, &keys, GENEROUS_TIMEOUTS, &phone).unwrap();
        let token = create(&store, &keys, &user_id, &Client::default()).unwrap();
        let current = authenticate(&store, &keys, GENEROUS_TIMEOUTS, &token).unwrap();
        let scope = scope::resolve(&store, &user_id, None).unwrap();
        revoke_other(&store, &scope, &current, &phone_session.id).unwrap();
        let result = authenticate(&store, &keys, GENEROUS_TIMEOUTS, &phone);
        assert!(matches!(result, Err(SessionError::Unauthenticated)));
        assert!(authenticate(&store, &keys, GENEROUS_TIMEOUTS, &token).is_ok());
    }

    #[test]
    fn revoking_the_others_keeps_the_current_one() {
        let (store, user_id) = store_with_user();
        let keys = keys();
        let first = create(&store, &keys, &user_id, &Client::default()).unwrap();
        let second = create(&store, &keys, &user_id, &Client::default()).unwrap();
        let token = create(&store, &keys, &user_id, &Client::default()).unwrap();
        let current = authenticate(&store, &keys, GENEROUS_TIMEOUTS, &token).unwrap();
        let scope = scope::resolve(&store, &user_id, None).unwrap();
        revoke_others(&store, &scope, &current).unwrap();
        assert!(authenticate(&store, &keys, GENEROUS_TIMEOUTS, &first).is_err());
        assert!(authenticate(&store, &keys, GENEROUS_TIMEOUTS, &second).is_err());
        assert!(authenticate(&store, &keys, GENEROUS_TIMEOUTS, &token).is_ok());
        let events = crate::events::for_organization(&store, &scope).unwrap();
        let revoked = events
            .iter()
            .filter(|record| record.event_type == "session.revoked")
            .count();
        assert_eq!(revoked, 2);
    }
}
