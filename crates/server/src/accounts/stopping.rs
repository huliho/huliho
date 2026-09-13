// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Stopping and resuming an account: the stop lives on the row with its
//! cause; the probe reads the rows stopped on refused connections.

use rusqlite::params;

use super::{Account, StopCause, read_account};
use crate::events::{Actor, DomainEvent, append};
use crate::ids::{AccountId, UserId};
use crate::scope::Scope;
use crate::store::{Store, StoreError, now_ms};

/// What a resume did: the row as it stands and whether a stop cleared.
#[derive(Debug, Clone)]
pub struct Resumed {
    /// The row as it stands afterwards.
    pub account: Account,
    /// Whether a stop cleared.
    pub was_stopped: bool,
}

/// Stops the account with `cause` and appends the fact; whether the row
/// changed. An account already stopped with that cause stays as it is;
/// so does one stopped on its credentials, since the user has to act
/// either way.
///
/// # Errors
///
/// Returns [`StoreError::MissingAccount`] for a scope without an account
/// and [`StoreError::NotFound`] when the row is gone.
pub fn stop(
    store: &Store,
    scope: &Scope,
    cause: StopCause,
    actor: &Actor,
) -> Result<bool, StoreError> {
    let account_id = scope.account()?.clone();
    store.write(|transaction| {
        let current = read_account(transaction, scope, &account_id)?;
        let outranked = current.stopped_cause == Some(StopCause::Credentials);
        if current.stopped_cause == Some(cause) || outranked {
            return Ok(false);
        }
        transaction.execute(
            "UPDATE accounts SET stopped_cause = ?1, stopped_at = ?2 WHERE id = ?3",
            params![cause.as_str(), now_ms(), account_id.as_str()],
        )?;
        let event = DomainEvent::AccountStopped { account_id, cause };
        append(transaction, scope.organization_id(), actor, &event)?;
        Ok(true)
    })
}

/// Clears the stop and appends the fact; an account that is not stopped
/// stays as it is and appends nothing.
///
/// # Errors
///
/// Returns [`StoreError::MissingAccount`] for a scope without an account
/// and [`StoreError::NotFound`] when the row is gone.
pub fn resume(store: &Store, scope: &Scope, actor: &Actor) -> Result<Resumed, StoreError> {
    let account_id = scope.account()?.clone();
    store.write(|transaction| {
        let current = read_account(transaction, scope, &account_id)?;
        if current.stopped_cause.is_none() {
            return Ok(Resumed {
                account: current,
                was_stopped: false,
            });
        }
        transaction.execute(
            "UPDATE accounts SET stopped_cause = NULL, stopped_at = NULL WHERE id = ?1",
            [account_id.as_str()],
        )?;
        let event = DomainEvent::AccountResumed { account_id };
        append(transaction, scope.organization_id(), actor, &event)?;
        Ok(Resumed {
            account: Account {
                stopped_cause: None,
                stopped_at: None,
                ..current
            },
            was_stopped: true,
        })
    })
}

/// The accounts stopped on refused connections, instance wide, as the
/// probe lists them; every read that follows resolves its own scope.
pub(crate) fn stopped_on_connection(store: &Store) -> Result<Vec<(UserId, AccountId)>, StoreError> {
    store.read(|connection| {
        let mut statement = connection.prepare(
            "SELECT user_id, id FROM accounts WHERE stopped_cause = ?1 ORDER BY stopped_at, id",
        )?;
        let rows = statement
            .query_map([StopCause::Connection.as_str()], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::{self, AccountSettings, Credential, NewAccount, Provider};
    use crate::events;
    use crate::identity;
    use crate::scope;
    use crate::secrets::{InstanceSecret, Keys};

    fn keys() -> Keys {
        Keys::derive(
            &InstanceSecret::from_bytes(b"0123456789abcdef0123456789abcdef".to_vec()).unwrap(),
        )
    }

    fn new_account(address: &str) -> NewAccount {
        NewAccount {
            address: address.to_owned(),
            name: "Fastmail".to_owned(),
            provider: Provider::Fastmail,
            settings: AccountSettings::Jmap {
                session_url: "https://api.fastmail.com/jmap/session".parse().unwrap(),
            },
            credential: Credential::Bearer {
                token: "fmu1-token".to_owned(),
            },
        }
    }

    /// An owner with one account: the plain scope and the account scope.
    fn owner_with_account(store: &Store, login: &str) -> (Scope, Scope) {
        let (_, user) = identity::create_personal_user(store, login).unwrap();
        let scope = scope::resolve(store, &user.id, None).unwrap();
        let account = accounts::add(store, &keys(), &scope, &new_account(login)).unwrap();
        let scoped = scope::resolve(store, &user.id, Some(&account.id)).unwrap();
        (scope, scoped)
    }

    #[test]
    fn a_resume_clears_the_stop_once_and_a_running_account_stays_silent() {
        let store = Store::in_memory().unwrap();
        let (scope, scoped) = owner_with_account(&store, "mira@example.com");
        let user = Actor::User(scope.user_id().clone());
        let silent = resume(&store, &scoped, &Actor::System).unwrap();
        assert!(!silent.was_stopped);
        assert_eq!(silent.account.stopped_cause, None);
        stop(&store, &scoped, StopCause::Connection, &Actor::System).unwrap();
        let resumed = resume(&store, &scoped, &user).unwrap();
        assert!(resumed.was_stopped);
        assert_eq!(resumed.account.stopped_cause, None);
        assert_eq!(resumed.account.stopped_at, None);
        assert_eq!(accounts::get(&store, &scoped).unwrap().stopped_cause, None);
        let resumed_by: Vec<String> = events::for_organization(&store, &scope)
            .unwrap()
            .into_iter()
            .filter(|record| record.event_type == "account.resumed")
            .map(|record| record.actor)
            .collect();
        assert_eq!(resumed_by, [scope.user_id().as_str()]);
        assert!(matches!(
            resume(&store, &scope, &Actor::System),
            Err(StoreError::MissingAccount)
        ));
    }

    #[test]
    fn a_credentials_stop_outranks_a_connection_stop() {
        let store = Store::in_memory().unwrap();
        let (scope, scoped) = owner_with_account(&store, "mira@example.com");
        assert!(stop(&store, &scoped, StopCause::Credentials, &Actor::System).unwrap());
        assert!(!stop(&store, &scoped, StopCause::Connection, &Actor::System).unwrap());
        assert!(!stop(&store, &scoped, StopCause::Credentials, &Actor::System).unwrap());
        assert_eq!(
            accounts::get(&store, &scoped).unwrap().stopped_cause,
            Some(StopCause::Credentials)
        );
        let stops = events::for_organization(&store, &scope)
            .unwrap()
            .into_iter()
            .filter(|record| record.event_type == "account.stopped")
            .count();
        assert_eq!(stops, 1);
    }

    #[test]
    fn the_probe_lists_connection_stops_across_users_and_nothing_else() {
        let store = Store::in_memory().unwrap();
        let (_, alpha) = owner_with_account(&store, "alpha@example.com");
        let (_, beta) = owner_with_account(&store, "beta@example.com");
        let (_, gamma) = owner_with_account(&store, "gamma@example.com");
        stop(&store, &alpha, StopCause::Connection, &Actor::System).unwrap();
        stop(&store, &beta, StopCause::Credentials, &Actor::System).unwrap();
        let listed = stopped_on_connection(&store).unwrap();
        assert_eq!(
            listed,
            [(alpha.user_id().clone(), alpha.account_id().unwrap().clone())]
        );
        assert!(!listed.iter().any(|(_, id)| Some(id) == gamma.account_id()));
        resume(&store, &alpha, &Actor::System).unwrap();
        assert!(stopped_on_connection(&store).unwrap().is_empty());
    }
}
