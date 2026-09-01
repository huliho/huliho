// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Local login: argon2id hashing and non-enumerating verification.

use std::sync::LazyLock;

use argon2::{Algorithm, Argon2, Params, PasswordHasher, PasswordVerifier, Version};
use rusqlite::OptionalExtension;
use thiserror::Error;

use crate::events::{Actor, DomainEvent, append};
use crate::ids::{OrganizationId, UserId};
use crate::store::{Store, StoreError};

/// OWASP password storage baseline: 19 MiB memory, two passes, one lane.
const MEMORY_KIB: u32 = 19 * 1024;
const PASSES: u32 = 2;
const LANES: u32 = 1;

/// ASVS floor for a user-chosen password.
pub const MIN_PASSWORD_CHARS: usize = 12;

/// Generous passphrase room while bounding the hashing input.
pub const MAX_PASSWORD_CHARS: usize = 128;

/// Verified for names without a stored hash, so timing says nothing.
static DUMMY_HASH: LazyLock<String> =
    LazyLock::new(|| hash("correct horse battery staple").expect("the fixed input hashes"));

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("passwords take {MIN_PASSWORD_CHARS} to {MAX_PASSWORD_CHARS} characters")]
    PasswordLength,
    #[error("password hashing failed: {0}")]
    Hash(argon2::password_hash::Error),
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// What a login attempt came to; a rejection keeps the matched user for
/// the event log without telling the caller more than "no".
#[derive(Debug)]
pub enum LoginOutcome {
    Verified(UserId),
    Rejected(Option<UserId>),
}

/// Hashes and stores a new password for the user.
///
/// # Errors
///
/// Returns an error when the password falls outside the length window,
/// hashing fails, the user does not exist or the database fails.
pub fn set_password(store: &Store, user_id: &UserId, password: &str) -> Result<(), AuthError> {
    let length = password.chars().count();
    if !(MIN_PASSWORD_CHARS..=MAX_PASSWORD_CHARS).contains(&length) {
        return Err(AuthError::PasswordLength);
    }
    let encoded = hash(password).map_err(AuthError::Hash)?;
    store.write(|transaction| {
        let updated = transaction.execute(
            "UPDATE users SET password_hash = ?1 WHERE id = ?2",
            [encoded.as_str(), user_id.as_str()],
        )?;
        if updated == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    })?;
    Ok(())
}

/// Verifies a login attempt without revealing whether the name exists:
/// unknown names and passwordless users burn the same hashing cost.
///
/// # Errors
///
/// Returns an error when the database fails; a wrong name or password is
/// a [`LoginOutcome::Rejected`], not an error.
pub fn verify_login(
    store: &Store,
    login: &str,
    password: &str,
) -> Result<LoginOutcome, StoreError> {
    let row: Option<(UserId, Option<String>)> = store.read(|connection| {
        let row = connection
            .query_row(
                "SELECT id, password_hash FROM users WHERE login = ?1",
                [login],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        Ok(row)
    })?;
    Ok(match row {
        Some((user_id, Some(stored))) => {
            if hasher()
                .verify_password(password.as_bytes(), stored.as_str())
                .is_ok()
            {
                LoginOutcome::Verified(user_id)
            } else {
                LoginOutcome::Rejected(Some(user_id))
            }
        }
        Some((user_id, None)) => {
            burn_hashing_cost(password);
            LoginOutcome::Rejected(Some(user_id))
        }
        None => {
            burn_hashing_cost(password);
            LoginOutcome::Rejected(None)
        }
    })
}

/// Records a failed attempt on an existing user in the event log.
///
/// # Errors
///
/// Returns an error when the user is gone or the database fails.
pub fn record_login_failure(store: &Store, user_id: &UserId) -> Result<(), StoreError> {
    store.write(|transaction| {
        let organization_id: OrganizationId = transaction
            .query_row(
                "SELECT organization_id FROM users WHERE id = ?1",
                [user_id.as_str()],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(StoreError::NotFound)?;
        let event = DomainEvent::LoginFailed {
            user_id: user_id.clone(),
        };
        append(transaction, &organization_id, &Actor::System, &event)
    })
}

fn burn_hashing_cost(password: &str) {
    let _ = hasher().verify_password(password.as_bytes(), DUMMY_HASH.as_str());
}

fn hash(password: &str) -> Result<String, argon2::password_hash::Error> {
    Ok(hasher().hash_password(password.as_bytes())?.to_string())
}

fn hasher() -> Argon2<'static> {
    let params = Params::new(MEMORY_KIB, PASSES, LANES, None).expect("the fixed parameters hold");
    Argon2::new(Algorithm::Argon2id, Version::default(), params)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity;

    const PASSWORD: &str = "example passphrase";

    fn store_with_user() -> (Store, UserId) {
        let store = Store::in_memory().unwrap();
        let (_, user) = identity::create_personal_user(&store, "mira@example.com").unwrap();
        (store, user.id)
    }

    #[test]
    fn a_set_password_verifies_and_a_wrong_one_rejects() {
        let (store, user_id) = store_with_user();
        set_password(&store, &user_id, PASSWORD).unwrap();
        let verified = verify_login(&store, "mira@example.com", PASSWORD).unwrap();
        assert!(matches!(verified, LoginOutcome::Verified(id) if id == user_id));
        let rejected = verify_login(&store, "mira@example.com", "wrong password!").unwrap();
        assert!(matches!(rejected, LoginOutcome::Rejected(Some(id)) if id == user_id));
    }

    #[test]
    fn an_unknown_name_rejects_without_a_user() {
        let (store, _) = store_with_user();
        let rejected = verify_login(&store, "ghost@example.com", PASSWORD).unwrap();
        assert!(matches!(rejected, LoginOutcome::Rejected(None)));
    }

    #[test]
    fn a_passwordless_user_cannot_sign_in() {
        let (store, user_id) = store_with_user();
        let rejected = verify_login(&store, "mira@example.com", PASSWORD).unwrap();
        assert!(matches!(rejected, LoginOutcome::Rejected(Some(id)) if id == user_id));
    }

    #[test]
    fn password_length_is_bounded() {
        let (store, user_id) = store_with_user();
        let short = set_password(&store, &user_id, "short");
        assert!(matches!(short, Err(AuthError::PasswordLength)));
        let long = "x".repeat(MAX_PASSWORD_CHARS + 1);
        let too_long = set_password(&store, &user_id, &long);
        assert!(matches!(too_long, Err(AuthError::PasswordLength)));
    }

    #[test]
    fn setting_a_password_for_an_unknown_user_is_not_found() {
        let store = Store::in_memory().unwrap();
        let unknown = UserId::from("unknown".to_owned());
        let result = set_password(&store, &unknown, PASSWORD);
        assert!(matches!(
            result,
            Err(AuthError::Store(StoreError::NotFound))
        ));
    }
}
