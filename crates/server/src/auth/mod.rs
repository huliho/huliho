// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Local login: argon2id hashing and non-enumerating verification.

mod one_time;

use std::sync::LazyLock;

use argon2::{Algorithm, Argon2, Params, PasswordHasher, PasswordVerifier, Version};
use rusqlite::OptionalExtension;
use thiserror::Error;

pub use one_time::{OneTimePassword, create_user, reset_password};

use crate::events::{Actor, DomainEvent, append};
use crate::ids::{OrganizationId, UserId};
use crate::scope::Scope;
use crate::store::{Store, StoreError, now_ms};

/// OWASP password storage baseline: 19 MiB memory, two passes, one lane.
const MEMORY_KIB: u32 = 19 * 1024;
const PASSES: u32 = 2;
const LANES: u32 = 1;

/// Floor for a user-chosen password, ASVS 4.0.3 requirement 2.1.1.
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
    #[error("an admin resets other users' passwords, not their own")]
    OwnPassword,
    #[error("the system randomness source failed")]
    Random,
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
    /// A live one-time password matched; the session it opens reaches
    /// only the password change.
    VerifiedOneTime(UserId),
    Rejected(Option<UserId>),
}

/// The credential columns of one user row.
struct StoredPassword {
    user_id: UserId,
    hash: Option<String>,
    one_time_expires_at: Option<i64>,
}

/// Hashes and stores a chosen password for the user.
///
/// # Errors
///
/// Returns an error when the password falls outside the length window,
/// hashing fails, the user does not exist or the database fails.
pub fn set_password(store: &Store, user_id: &UserId, password: &str) -> Result<(), AuthError> {
    let encoded = hash_password(password)?;
    store.write(|transaction| {
        let updated = transaction.execute(
            "UPDATE users SET password_hash = ?1, password_reset_expires_at = NULL WHERE id = ?2",
            [encoded.as_str(), user_id.as_str()],
        )?;
        if updated == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    })?;
    Ok(())
}

/// Refuses a password outside the length window.
///
/// # Errors
///
/// Returns [`AuthError::PasswordLength`] outside the window.
pub fn check_length(password: &str) -> Result<(), AuthError> {
    let length = password.chars().count();
    if (MIN_PASSWORD_CHARS..=MAX_PASSWORD_CHARS).contains(&length) {
        Ok(())
    } else {
        Err(AuthError::PasswordLength)
    }
}

/// Hashes a password after checking its length.
///
/// # Errors
///
/// Returns an error when the password falls outside the length window
/// or hashing fails.
pub fn hash_password(password: &str) -> Result<String, AuthError> {
    check_length(password)?;
    hash(password).map_err(AuthError::Hash)
}

/// Verifies a login attempt without revealing whether the name exists:
/// unknown names, passwordless users and expired one-time passwords burn
/// the same hashing cost.
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
    let stored = store.read(|connection| {
        let row = connection
            .query_row(
                "SELECT id, password_hash, password_reset_expires_at FROM users WHERE login = ?1",
                [login],
                |row| {
                    Ok(StoredPassword {
                        user_id: row.get(0)?,
                        hash: row.get(1)?,
                        one_time_expires_at: row.get(2)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    })?;
    let Some(stored) = stored else {
        burn_hashing_cost(password);
        return Ok(LoginOutcome::Rejected(None));
    };
    Ok(outcome_of(&stored, password))
}

fn outcome_of(stored: &StoredPassword, password: &str) -> LoginOutcome {
    let user_id = stored.user_id.clone();
    let Some(hash) = stored.hash.as_deref() else {
        burn_hashing_cost(password);
        return LoginOutcome::Rejected(Some(user_id));
    };
    let live_one_time = stored
        .one_time_expires_at
        .map(|expires_at| now_ms() < expires_at);
    let verified = match live_one_time {
        None => matches(password, hash),
        Some(true) => matches(&one_time::typed(password), hash),
        Some(false) => {
            burn_hashing_cost(password);
            false
        }
    };
    match (verified, live_one_time) {
        (true, None) => LoginOutcome::Verified(user_id),
        (true, Some(_)) => LoginOutcome::VerifiedOneTime(user_id),
        (false, _) => LoginOutcome::Rejected(Some(user_id)),
    }
}

/// Checks the scope user's current password for the self-service
/// change. A user without a chosen password burns the hashing cost and
/// gets `false`; a one-time password is no current password.
///
/// # Errors
///
/// Returns an error when the database fails.
pub fn verify_password(store: &Store, scope: &Scope, password: &str) -> Result<bool, StoreError> {
    let hash: Option<String> = store.read(|connection| {
        let hash = connection
            .query_row(
                "SELECT password_hash FROM users
                 WHERE id = ?1 AND password_reset_expires_at IS NULL",
                [scope.user_id().as_str()],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        Ok(hash)
    })?;
    let Some(hash) = hash else {
        burn_hashing_cost(password);
        return Ok(false);
    };
    Ok(matches(password, &hash))
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

fn matches(password: &str, hash: &str) -> bool {
    hasher().verify_password(password.as_bytes(), hash).is_ok()
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
    use crate::scope;

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
        assert!(check_length(&"x".repeat(MIN_PASSWORD_CHARS)).is_ok());
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

    #[test]
    fn the_current_password_check_answers_only_for_a_chosen_password() {
        let (store, user_id) = store_with_user();
        let scope = scope::resolve(&store, &user_id, None).unwrap();
        assert!(!verify_password(&store, &scope, PASSWORD).unwrap());
        set_password(&store, &user_id, PASSWORD).unwrap();
        assert!(verify_password(&store, &scope, PASSWORD).unwrap());
        assert!(!verify_password(&store, &scope, "wrong password!").unwrap());
    }
}
