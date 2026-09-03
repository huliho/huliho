// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Opening a session: the ordinary sign-in and the one a one-time
//! password buys.

use rusqlite::{Connection, params};

use super::activity::record_activity;
use super::{Client, SealedSession, SessionError, TokenHash, organization_of, seal};
use crate::events::{Actor, DomainEvent, append};
use crate::ids::{SessionId, UserId};
use crate::secrets::SessionKeys;
use crate::store::{Store, StoreError, now_ms};

/// 128 random bits per token, beyond guessing.
const TOKEN_BYTES: usize = 16;

/// What a new row records.
struct NewRow<'a> {
    user_id: &'a UserId,
    client: &'a Client,
    password_change_required: bool,
}

/// The token and the sealed row derived from it, ready to insert.
struct Prepared {
    token: String,
    token_hash: TokenHash,
    sealed: Vec<u8>,
    device: String,
    created_at: i64,
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
    let row = NewRow {
        user_id,
        client,
        password_change_required: false,
    };
    let prepared = prepare(keys, &row)?;
    store.write(|transaction| insert(transaction, &row, &prepared))?;
    Ok(prepared.token)
}

/// Creates the session a one-time password opens: it reaches only the
/// password change and spends the one-time password in the same
/// transaction. Returns `None` when no live one-time password remains,
/// so a secret works exactly once even under concurrent attempts.
///
/// # Errors
///
/// As [`create`].
pub fn create_for_password_change(
    store: &Store,
    keys: &SessionKeys,
    user_id: &UserId,
    client: &Client,
) -> Result<Option<String>, SessionError> {
    let row = NewRow {
        user_id,
        client,
        password_change_required: true,
    };
    let prepared = prepare(keys, &row)?;
    let spent = store.write(|transaction| {
        if !spend_one_time_password(transaction, user_id, prepared.created_at)? {
            return Ok(false);
        }
        insert(transaction, &row, &prepared)?;
        Ok(true)
    })?;
    Ok(spent.then_some(prepared.token))
}

pub(super) fn fresh_token() -> Result<String, SessionError> {
    let mut bytes = [0u8; TOKEN_BYTES];
    getrandom::fill(&mut bytes).map_err(|_| SessionError::Random)?;
    Ok(base16ct::lower::encode_string(&bytes))
}

fn prepare(keys: &SessionKeys, row: &NewRow<'_>) -> Result<Prepared, SessionError> {
    let token = fresh_token()?;
    let token_hash = TokenHash::of(&token);
    let created_at = now_ms();
    let sealed = seal(
        keys,
        &token_hash,
        &SealedSession {
            user_id: row.user_id.clone(),
            created_at,
            password_change_required: row.password_change_required,
        },
    )?;
    let device = serde_json::to_string(&row.client.device).map_err(StoreError::from)?;
    Ok(Prepared {
        token,
        token_hash,
        sealed,
        device,
        created_at,
    })
}

/// Clears a live one-time password; `false` when none is live, so a
/// spent or expired secret opens nothing.
fn spend_one_time_password(
    connection: &Connection,
    user_id: &UserId,
    now: i64,
) -> Result<bool, StoreError> {
    let spent = connection.execute(
        "UPDATE users SET password_hash = NULL, password_reset_expires_at = NULL
         WHERE id = ?1 AND password_reset_expires_at > ?2",
        params![user_id.as_str(), now],
    )?;
    Ok(spent == 1)
}

fn insert(
    connection: &Connection,
    row: &NewRow<'_>,
    prepared: &Prepared,
) -> Result<(), StoreError> {
    let organization_id = organization_of(connection, row.user_id)?;
    connection.execute(
        "INSERT INTO sessions
         (token_hash, id, user_id, sealed, device, address, created_at, last_seen_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
        params![
            prepared.token_hash.as_slice(),
            SessionId::generate().as_str(),
            row.user_id.as_str(),
            prepared.sealed,
            prepared.device,
            row.client.address.map(|address| address.to_string()),
            prepared.created_at
        ],
    )?;
    let actor = Actor::User(row.user_id.clone());
    append(
        connection,
        &organization_id,
        &actor,
        &DomainEvent::SessionCreated {},
    )?;
    record_activity(
        connection,
        &organization_id,
        row.user_id,
        prepared.created_at,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::fixtures::{
        GENEROUS_TIMEOUTS, hand_out_one_time_password, keys, password_hash_of, session_rows,
        store_with_user,
    };
    use crate::session::{MS_PER_MINUTE, authenticate};

    #[test]
    fn a_forced_session_carries_its_flag_and_spends_the_secret() {
        let (store, user_id) = store_with_user();
        let keys = keys();
        hand_out_one_time_password(&store, &user_id, now_ms() + MS_PER_MINUTE);
        let token = create_for_password_change(&store, &keys, &user_id, &Client::default())
            .unwrap()
            .unwrap();
        let session = authenticate(&store, &keys, GENEROUS_TIMEOUTS, &token).unwrap();
        assert!(session.password_change_required);
        assert_eq!(password_hash_of(&store, &user_id), None);
        let plain = create(&store, &keys, &user_id, &Client::default()).unwrap();
        let plain = authenticate(&store, &keys, GENEROUS_TIMEOUTS, &plain).unwrap();
        assert!(!plain.password_change_required);
    }

    #[test]
    fn a_missing_expired_or_spent_secret_opens_no_session() {
        let (store, user_id) = store_with_user();
        let keys = keys();
        let attempt = || create_for_password_change(&store, &keys, &user_id, &Client::default());
        assert!(attempt().unwrap().is_none());
        hand_out_one_time_password(&store, &user_id, now_ms() - 1);
        assert!(attempt().unwrap().is_none());
        hand_out_one_time_password(&store, &user_id, now_ms() + MS_PER_MINUTE);
        assert!(attempt().unwrap().is_some());
        assert!(attempt().unwrap().is_none());
        assert_eq!(session_rows(&store), 1);
    }
}
