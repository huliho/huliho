// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! One-time passwords: what an admin hands a user for the first or a
//! fresh sign-in.

use rusqlite::{Connection, OptionalExtension, params};

use super::{AuthError, hash_password};
use crate::events::{Actor, DomainEvent, append};
use crate::identity::{self, NewUser, User};
use crate::ids::{Role, UserId};
use crate::scope::Scope;
use crate::session::{MS_PER_MINUTE, revoke_all};
use crate::store::{Store, StoreError, now_ms};

/// Thirty-two symbols without the look-alikes 0, O, 1 and l, so a
/// secret survives being read aloud or written down. A byte modulo 32
/// is uniform, since 256 splits evenly over the alphabet.
const ALPHABET: &[u8; 32] = b"abcdefghijkmnpqrstuvwxyz23456789";

/// Twelve symbols carry sixty bits, plenty for a secret that works once
/// and sits behind the sign-in limiter.
const SECRET_CHARS: usize = 12;

/// Groups of four read and type more easily than one run of twelve.
const GROUP_CHARS: usize = 4;

/// A handed-over secret needs a working day plus a night at most.
const ONE_TIME_PASSWORD_TTL_MS: i64 = 24 * 60 * MS_PER_MINUTE;

/// The secret in its display form plus when it stops working.
pub struct OneTimePassword {
    pub secret: String,
    pub expires_at: i64,
}

struct Issued {
    secret: String,
    hash: String,
    expires_at: i64,
}

/// Creates a user inside the scope's organization and hands out their
/// first, one-time password in the same transaction, so no user exists
/// without a credential. Admin or higher; the granted role never exceeds
/// the actor's own.
///
/// # Errors
///
/// Returns [`StoreError::Forbidden`] below the admin role or above the
/// actor's role, [`StoreError::LoginTaken`] for a sign-in name already
/// in use; randomness, hashing and database failures pass through.
pub fn create_user(
    store: &Store,
    scope: &Scope,
    new: &NewUser,
) -> Result<(User, OneTimePassword), AuthError> {
    // Checked before the hash so a refused request costs none; the
    // insert checks again inside the transaction.
    identity::ensure_may_grant(scope, new.role)?;
    let issued = generate()?;
    let user = store.write(|transaction| {
        let user = identity::insert_organization_user(transaction, scope, new)?;
        store_secret(transaction, &user.id, &issued)?;
        Ok(user)
    })?;
    Ok((user, issued.into_password()))
}

/// Replaces a user's password with a one-time password and ends every
/// session of that user. Admin or higher; the target sits in the scope's
/// organization, does not outrank the actor and is not the actor.
///
/// # Errors
///
/// Returns [`AuthError::OwnPassword`] for the actor's own row,
/// [`StoreError::Forbidden`] below the admin role or above the actor's
/// role, [`StoreError::NotFound`] outside the organization; randomness,
/// hashing and database failures pass through.
pub fn reset_password(
    store: &Store,
    scope: &Scope,
    target: &UserId,
) -> Result<OneTimePassword, AuthError> {
    scope.require(Role::Admin)?;
    if target == scope.user_id() {
        return Err(AuthError::OwnPassword);
    }
    // Checked before the hash so a refused request costs none; the check
    // inside the transaction is the one that decides.
    store.read(|connection| ensure_within_reach(connection, scope, target))?;
    let issued = generate()?;
    store.write(|transaction| {
        ensure_within_reach(transaction, scope, target)?;
        store_secret(transaction, target, &issued)?;
        let actor = Actor::User(scope.user_id().clone());
        revoke_all(transaction, scope.organization_id(), &actor, target)?;
        let event = DomainEvent::UserPasswordReset {
            user_id: target.clone(),
        };
        append(transaction, scope.organization_id(), &actor, &event)
    })?;
    Ok(issued.into_password())
}

/// The typed form of a one-time password: the display hyphens are not
/// part of the secret.
pub(super) fn typed(entered: &str) -> String {
    entered.replace('-', "")
}

fn generate() -> Result<Issued, AuthError> {
    let mut bytes = [0u8; SECRET_CHARS];
    getrandom::fill(&mut bytes).map_err(|_| AuthError::Random)?;
    let secret: String = bytes
        .iter()
        .map(|byte| char::from(ALPHABET[usize::from(*byte) % ALPHABET.len()]))
        .collect();
    let hash = hash_password(&secret)?;
    Ok(Issued {
        secret,
        hash,
        expires_at: now_ms().saturating_add(ONE_TIME_PASSWORD_TTL_MS),
    })
}

impl Issued {
    fn into_password(self) -> OneTimePassword {
        let symbols: Vec<char> = self.secret.chars().collect();
        let groups: Vec<String> = symbols
            .chunks(GROUP_CHARS)
            .map(|group| group.iter().collect())
            .collect();
        OneTimePassword {
            secret: groups.join("-"),
            expires_at: self.expires_at,
        }
    }
}

fn ensure_within_reach(
    connection: &Connection,
    scope: &Scope,
    target: &UserId,
) -> Result<(), StoreError> {
    let role: Role = connection
        .query_row(
            "SELECT role FROM users WHERE id = ?1 AND organization_id = ?2",
            [target.as_str(), scope.organization_id().as_str()],
            |row| row.get(0),
        )
        .optional()?
        .ok_or(StoreError::NotFound)?;
    if role > scope.role() {
        return Err(StoreError::Forbidden);
    }
    Ok(())
}

fn store_secret(
    connection: &Connection,
    target: &UserId,
    issued: &Issued,
) -> Result<(), StoreError> {
    connection.execute(
        "UPDATE users SET password_hash = ?1, password_reset_expires_at = ?2 WHERE id = ?3",
        params![issued.hash, issued.expires_at, target.as_str()],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{LoginOutcome, verify_login, verify_password};
    use crate::scope;
    use crate::session::fixtures::{keys, session_rows};
    use crate::session::{Client, create};

    const OWNER: &str = "owner@example.com";
    const MEMBER: &str = "member@example.com";

    fn new_user(login: &str, role: Role) -> NewUser {
        NewUser {
            login: login.to_owned(),
            name: login.to_owned(),
            role,
        }
    }

    fn store_with_owner_and_member() -> (Store, UserId, UserId) {
        let store = Store::in_memory().unwrap();
        let (_, owner) = identity::create_personal_user(&store, OWNER).unwrap();
        let owner_scope = scope::resolve(&store, &owner.id, None).unwrap();
        let member = identity::create_organization_user(
            &store,
            &owner_scope,
            &new_user(MEMBER, Role::Member),
        )
        .unwrap();
        (store, owner.id, member.id)
    }

    fn event_types(store: &Store, scope: &Scope) -> Vec<String> {
        crate::events::for_organization(store, scope)
            .unwrap()
            .into_iter()
            .map(|record| record.event_type)
            .collect()
    }

    #[test]
    fn a_reset_secret_has_the_display_shape_and_signs_in_typed_or_pasted() {
        let (store, owner_id, member_id) = store_with_owner_and_member();
        let owner_scope = scope::resolve(&store, &owner_id, None).unwrap();
        let before = now_ms();
        let issued = reset_password(&store, &owner_scope, &member_id).unwrap();
        assert_eq!(issued.secret.len(), SECRET_CHARS + 2);
        assert_eq!(issued.secret.chars().filter(|c| *c == '-').count(), 2);
        assert!(
            issued
                .secret
                .bytes()
                .all(|byte| byte == b'-' || ALPHABET.contains(&byte))
        );
        assert!(issued.expires_at >= before + ONE_TIME_PASSWORD_TTL_MS);
        assert!(issued.expires_at <= now_ms() + ONE_TIME_PASSWORD_TTL_MS);
        let outcome = verify_login(&store, MEMBER, &issued.secret).unwrap();
        assert!(matches!(outcome, LoginOutcome::VerifiedOneTime(id) if id == member_id));
        let outcome = verify_login(&store, MEMBER, &typed(&issued.secret)).unwrap();
        assert!(matches!(outcome, LoginOutcome::VerifiedOneTime(_)));
        let outcome = verify_login(&store, MEMBER, "k7fq-2mzp-x4rt").unwrap();
        assert!(matches!(outcome, LoginOutcome::Rejected(Some(_))));
    }

    #[test]
    fn a_reset_ends_every_session_of_the_target_and_records_itself() {
        let (store, owner_id, member_id) = store_with_owner_and_member();
        let keys = keys();
        create(&store, &keys, &member_id, &Client::default()).unwrap();
        create(&store, &keys, &member_id, &Client::default()).unwrap();
        create(&store, &keys, &owner_id, &Client::default()).unwrap();
        let owner_scope = scope::resolve(&store, &owner_id, None).unwrap();
        reset_password(&store, &owner_scope, &member_id).unwrap();
        assert_eq!(session_rows(&store), 1);
        let types = event_types(&store, &owner_scope);
        assert_eq!(types.iter().filter(|t| *t == "session.revoked").count(), 2);
        assert!(types.contains(&"user.password_reset".to_owned()));
    }

    #[test]
    fn an_expired_secret_is_a_wrong_password() {
        let (store, owner_id, member_id) = store_with_owner_and_member();
        let owner_scope = scope::resolve(&store, &owner_id, None).unwrap();
        let issued = reset_password(&store, &owner_scope, &member_id).unwrap();
        store
            .write(|transaction| {
                transaction
                    .execute(
                        "UPDATE users SET password_reset_expires_at = ?1 WHERE id = ?2",
                        params![now_ms() - 1, member_id.as_str()],
                    )
                    .map_err(StoreError::from)?;
                Ok(())
            })
            .unwrap();
        let outcome = verify_login(&store, MEMBER, &issued.secret).unwrap();
        assert!(matches!(outcome, LoginOutcome::Rejected(Some(id)) if id == member_id));
    }

    #[test]
    fn a_one_time_password_is_no_current_password() {
        let (store, owner_id, member_id) = store_with_owner_and_member();
        let owner_scope = scope::resolve(&store, &owner_id, None).unwrap();
        let issued = reset_password(&store, &owner_scope, &member_id).unwrap();
        let member_scope = scope::resolve(&store, &member_id, None).unwrap();
        assert!(!verify_password(&store, &member_scope, &issued.secret).unwrap());
        assert!(!verify_password(&store, &member_scope, &typed(&issued.secret)).unwrap());
    }

    #[test]
    fn nobody_resets_themselves_and_a_member_resets_nobody() {
        let (store, owner_id, member_id) = store_with_owner_and_member();
        let owner_scope = scope::resolve(&store, &owner_id, None).unwrap();
        let own = reset_password(&store, &owner_scope, &owner_id);
        assert!(matches!(own, Err(AuthError::OwnPassword)));
        let member_scope = scope::resolve(&store, &member_id, None).unwrap();
        let upward = reset_password(&store, &member_scope, &owner_id);
        assert!(matches!(
            upward,
            Err(AuthError::Store(StoreError::Forbidden))
        ));
        let outcome = verify_login(&store, OWNER, "anything at all").unwrap();
        assert!(matches!(outcome, LoginOutcome::Rejected(Some(_))));
    }

    #[test]
    fn an_admin_cannot_reset_an_owner_and_reaches_no_other_organization() {
        let (store, owner_id, _) = store_with_owner_and_member();
        let owner_scope = scope::resolve(&store, &owner_id, None).unwrap();
        let (admin, _) = create_user(
            &store,
            &owner_scope,
            &new_user("admin@example.com", Role::Admin),
        )
        .unwrap();
        let admin_scope = scope::resolve(&store, &admin.id, None).unwrap();
        let upward = reset_password(&store, &admin_scope, &owner_id);
        assert!(matches!(
            upward,
            Err(AuthError::Store(StoreError::Forbidden))
        ));
        let (_, stranger) = identity::create_personal_user(&store, "stranger@example.com").unwrap();
        let outside = reset_password(&store, &admin_scope, &stranger.id);
        assert!(matches!(
            outside,
            Err(AuthError::Store(StoreError::NotFound))
        ));
        let boss = create_user(
            &store,
            &admin_scope,
            &new_user("boss@example.com", Role::Owner),
        );
        assert!(matches!(boss, Err(AuthError::Store(StoreError::Forbidden))));
    }

    #[test]
    fn a_created_user_signs_in_once_with_the_first_password() {
        let (store, owner_id, _) = store_with_owner_and_member();
        let owner_scope = scope::resolve(&store, &owner_id, None).unwrap();
        let (user, issued) =
            create_user(&store, &owner_scope, &new_user("jonas", Role::Member)).unwrap();
        assert_eq!(user.login, "jonas");
        let outcome = verify_login(&store, "jonas", &issued.secret).unwrap();
        assert!(matches!(outcome, LoginOutcome::VerifiedOneTime(id) if id == user.id));
        let types = event_types(&store, &owner_scope);
        assert_eq!(types.iter().filter(|t| *t == "user.created").count(), 3);
        assert!(!types.contains(&"user.password_reset".to_owned()));
    }

    #[test]
    fn a_taken_login_creates_nothing() {
        let (store, owner_id, member_id) = store_with_owner_and_member();
        let owner_scope = scope::resolve(&store, &owner_id, None).unwrap();
        let taken = create_user(&store, &owner_scope, &new_user(MEMBER, Role::Member));
        assert!(matches!(
            taken,
            Err(AuthError::Store(StoreError::LoginTaken))
        ));
        assert_eq!(identity::users(&store, &owner_scope).unwrap().len(), 2);
        let member_scope = scope::resolve(&store, &member_id, None).unwrap();
        assert!(!verify_password(&store, &member_scope, "anything at all").unwrap());
        let outcome = verify_login(&store, MEMBER, "anything at all").unwrap();
        assert!(matches!(outcome, LoginOutcome::Rejected(Some(_))));
    }
}
