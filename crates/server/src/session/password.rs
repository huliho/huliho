// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A password change ends every other session and rotates the current
//! one, so the browser that changed it stays signed in on a fresh token.

use std::net::IpAddr;

use rusqlite::{Connection, params};

use super::activity::record_activity;
use super::list::end_others;
use super::open::fresh_token;
use super::{SealedSession, Session, SessionError, TokenHash, seal};
use crate::events::{Actor, DomainEvent, append};
use crate::ids::SessionId;
use crate::scope::Scope;
use crate::secrets::SessionKeys;
use crate::store::{Store, StoreError, now_ms};

/// The current session, the hash of the chosen password and where the
/// change came from.
pub struct PasswordChange<'a> {
    pub current: &'a Session,
    pub password_hash: String,
    pub address: Option<IpAddr>,
}

/// The row that takes the current session's place.
struct Replacement<'a> {
    id: SessionId,
    token_hash: &'a TokenHash,
    sealed: &'a [u8],
    address: Option<String>,
    created_at: i64,
}

/// Stores the new hash, moves the current session onto a fresh token
/// and ends every other session of the scope user. Returns the fresh
/// token.
///
/// # Errors
///
/// Returns [`SessionError::Unauthenticated`] when the current session is
/// gone; randomness, sealing and database failures pass through.
pub fn apply_password_change(
    store: &Store,
    keys: &SessionKeys,
    scope: &Scope,
    change: &PasswordChange<'_>,
) -> Result<String, SessionError> {
    let token = fresh_token()?;
    let token_hash = TokenHash::of(&token);
    let created_at = now_ms();
    let sealed = seal(
        keys,
        &token_hash,
        &SealedSession {
            user_id: scope.user_id().clone(),
            created_at,
            password_change_required: false,
        },
    )?;
    let replacement = Replacement {
        id: SessionId::generate(),
        token_hash: &token_hash,
        sealed: &sealed,
        address: change.address.map(|address| address.to_string()),
        created_at,
    };
    let rotated = store.write(|transaction| {
        if !rotate_row(transaction, scope, change.current, &replacement)? {
            return Ok(false);
        }
        transaction.execute(
            "UPDATE users SET password_hash = ?1, password_reset_expires_at = NULL WHERE id = ?2",
            params![change.password_hash, scope.user_id().as_str()],
        )?;
        end_others(transaction, scope, &replacement.id)?;
        record_activity(
            transaction,
            scope.organization_id(),
            scope.user_id(),
            created_at,
        )?;
        let event = DomainEvent::UserPasswordChanged {
            user_id: scope.user_id().clone(),
        };
        let actor = Actor::User(scope.user_id().clone());
        append(transaction, scope.organization_id(), &actor, &event)?;
        Ok(true)
    })?;
    if !rotated {
        return Err(SessionError::Unauthenticated);
    }
    Ok(token)
}

/// Moves the current row onto the replacement; `false` when the row is
/// already gone or belongs to someone else.
fn rotate_row(
    connection: &Connection,
    scope: &Scope,
    current: &Session,
    replacement: &Replacement<'_>,
) -> Result<bool, StoreError> {
    let updated = connection.execute(
        "UPDATE sessions
         SET token_hash = ?1, id = ?2, sealed = ?3, address = ?4, created_at = ?5, last_seen_at = ?5
         WHERE token_hash = ?6 AND user_id = ?7",
        params![
            replacement.token_hash.as_slice(),
            replacement.id.as_str(),
            replacement.sealed,
            replacement.address,
            replacement.created_at,
            current.token_hash.as_slice(),
            scope.user_id().as_str()
        ],
    )?;
    Ok(updated == 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{LoginOutcome, hash_password, verify_login};
    use crate::scope;
    use crate::session::fixtures::{
        GENEROUS_TIMEOUTS, hand_out_one_time_password, keys, session_rows, store_with_user,
    };
    use crate::session::{
        Client, MS_PER_MINUTE, authenticate, create, create_for_password_change, device, revoke,
    };

    const NEW_PASSWORD: &str = "a brand new passphrase";
    const ADDRESS: &str = "203.0.113.7";

    fn change_for(current: &Session) -> PasswordChange<'_> {
        PasswordChange {
            current,
            password_hash: hash_password(NEW_PASSWORD).unwrap(),
            address: Some(ADDRESS.parse().unwrap()),
        }
    }

    #[test]
    fn a_change_rotates_the_token_and_ends_the_others() {
        let (store, user_id) = store_with_user();
        let keys = keys();
        let other = create(&store, &keys, &user_id, &Client::default()).unwrap();
        let client = Client {
            device: device::from_user_agent("Mozilla/5.0 (X11; Linux x86_64) Firefox/128.0", true),
            address: None,
        };
        let token = create(&store, &keys, &user_id, &client).unwrap();
        let current = authenticate(&store, &keys, GENEROUS_TIMEOUTS, &token).unwrap();
        let scope = scope::resolve(&store, &user_id, None).unwrap();
        let fresh = apply_password_change(&store, &keys, &scope, &change_for(&current)).unwrap();
        assert!(authenticate(&store, &keys, GENEROUS_TIMEOUTS, &token).is_err());
        assert!(authenticate(&store, &keys, GENEROUS_TIMEOUTS, &other).is_err());
        let rotated = authenticate(&store, &keys, GENEROUS_TIMEOUTS, &fresh).unwrap();
        assert!(!rotated.password_change_required);
        assert_ne!(rotated.id, current.id);
        assert_eq!(session_rows(&store), 1);
        let rows = crate::session::list(&store, &scope, &rotated, GENEROUS_TIMEOUTS).unwrap();
        assert_eq!(rows[0].device, client.device);
        assert_eq!(rows[0].address.as_deref(), Some(ADDRESS));
        let outcome = verify_login(&store, "mira@example.com", NEW_PASSWORD).unwrap();
        assert!(matches!(outcome, LoginOutcome::Verified(_)));
        let events = crate::events::for_organization(&store, &scope).unwrap();
        let types: Vec<&str> = events.iter().map(|e| e.event_type.as_str()).collect();
        assert_eq!(types.iter().filter(|t| **t == "session.revoked").count(), 1);
        assert!(types.contains(&"user.password_changed"));
    }

    #[test]
    fn a_forced_session_becomes_an_ordinary_one() {
        let (store, user_id) = store_with_user();
        let keys = keys();
        hand_out_one_time_password(&store, &user_id, now_ms() + MS_PER_MINUTE);
        let token = create_for_password_change(&store, &keys, &user_id, &Client::default())
            .unwrap()
            .unwrap();
        let current = authenticate(&store, &keys, GENEROUS_TIMEOUTS, &token).unwrap();
        let scope = scope::resolve(&store, &user_id, None).unwrap();
        let fresh = apply_password_change(&store, &keys, &scope, &change_for(&current)).unwrap();
        let rotated = authenticate(&store, &keys, GENEROUS_TIMEOUTS, &fresh).unwrap();
        assert!(!rotated.password_change_required);
        let outcome = verify_login(&store, "mira@example.com", NEW_PASSWORD).unwrap();
        assert!(matches!(outcome, LoginOutcome::Verified(_)));
    }

    #[test]
    fn a_change_from_a_vanished_session_is_refused() {
        let (store, user_id) = store_with_user();
        let keys = keys();
        let token = create(&store, &keys, &user_id, &Client::default()).unwrap();
        let current = authenticate(&store, &keys, GENEROUS_TIMEOUTS, &token).unwrap();
        revoke(&store, &token).unwrap();
        let scope = scope::resolve(&store, &user_id, None).unwrap();
        let result = apply_password_change(&store, &keys, &scope, &change_for(&current));
        assert!(matches!(result, Err(SessionError::Unauthenticated)));
        let outcome = verify_login(&store, "mira@example.com", NEW_PASSWORD).unwrap();
        assert!(matches!(outcome, LoginOutcome::Rejected(Some(_))));
        assert_eq!(session_rows(&store), 0);
    }

    #[test]
    fn a_change_leaves_the_accounts_alone() {
        use crate::accounts::{self, AccountKind, AuthMethod};
        let (store, user_id) = store_with_user();
        let keys = keys();
        let scope = scope::resolve(&store, &user_id, None).unwrap();
        let account =
            accounts::link(&store, &scope, AccountKind::Jmap, AuthMethod::Bearer).unwrap();
        let token = create(&store, &keys, &user_id, &Client::default()).unwrap();
        let current = authenticate(&store, &keys, GENEROUS_TIMEOUTS, &token).unwrap();
        apply_password_change(&store, &keys, &scope, &change_for(&current)).unwrap();
        let listed = accounts::list(&store, &scope).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, account.id);
    }
}
