// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The rows read instance wide, outside a user's scope: what the
//! bridge's runtime starts at boot and how it finds the owner of an
//! account it acts for as the system.

use rusqlite::OptionalExtension;

use super::{ACCOUNT_COLUMNS, Account, AccountKind, account_from_row};
use crate::ids::{AccountId, UserId};
use crate::scope::{self, Scope};
use crate::store::{Store, StoreError};

/// Every IMAP account of the instance, oldest first.
///
/// # Errors
///
/// Returns an error when the database fails.
pub fn imap_accounts(store: &Store) -> Result<Vec<Account>, StoreError> {
    store.read(|connection| {
        let mut statement = connection.prepare(&format!(
            "SELECT {ACCOUNT_COLUMNS} FROM accounts WHERE kind = ?1 ORDER BY created_at, id"
        ))?;
        let rows = statement
            .query_map([AccountKind::Imap.as_str()], account_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

/// The scope of the account's owner, resolved for the account; `None`
/// when the row left.
///
/// # Errors
///
/// Returns an error when the database fails.
pub fn owner_scope(store: &Store, account_id: &AccountId) -> Result<Option<Scope>, StoreError> {
    let owner: Option<UserId> = store.read(|connection| {
        Ok(connection
            .query_row(
                "SELECT user_id FROM accounts WHERE id = ?1",
                [account_id.as_str()],
                |row| row.get(0),
            )
            .optional()?)
    })?;
    let Some(owner) = owner else {
        return Ok(None);
    };
    match scope::resolve(store, &owner, Some(account_id)) {
        Ok(scope) => Ok(Some(scope)),
        Err(StoreError::NotFound) => Ok(None),
        Err(other) => Err(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::{
        self, AccountSettings, Credential, Endpoint, NewAccount, Provider, TlsMode,
    };
    use crate::identity;
    use crate::secrets::{InstanceSecret, Keys};

    fn keys() -> Keys {
        Keys::derive(
            &InstanceSecret::from_bytes(b"0123456789abcdef0123456789abcdef".to_vec()).unwrap(),
        )
    }

    fn endpoint(host: &str) -> Endpoint {
        Endpoint {
            host: host.to_owned(),
            port: 993,
            tls: TlsMode::Implicit,
        }
    }

    fn imap(address: &str) -> NewAccount {
        NewAccount {
            address: address.to_owned(),
            name: "Mail".to_owned(),
            provider: Provider::Generic,
            settings: AccountSettings::Imap {
                username: address.to_owned(),
                imap: endpoint("imap.example.test"),
                smtp: endpoint("smtp.example.test"),
            },
            credential: Credential::Password {
                password: "correct horse".to_owned(),
            },
        }
    }

    fn jmap(address: &str) -> NewAccount {
        NewAccount {
            settings: AccountSettings::Jmap {
                session_url: "https://api.example.test/jmap/session".parse().unwrap(),
            },
            ..imap(address)
        }
    }

    #[test]
    fn the_imap_rows_of_every_user_list_and_each_names_its_owner() {
        let store = Store::in_memory().unwrap();
        let (_, mira) = identity::create_personal_user(&store, "mira@example.com").unwrap();
        let (_, noor) = identity::create_personal_user(&store, "noor@example.com").unwrap();
        let mira_scope = scope::resolve(&store, &mira.id, None).unwrap();
        let noor_scope = scope::resolve(&store, &noor.id, None).unwrap();
        let first = accounts::add(&store, &keys(), &mira_scope, &imap("mira@example.com")).unwrap();
        accounts::add(&store, &keys(), &mira_scope, &jmap("mira@example.net")).unwrap();
        let second =
            accounts::add(&store, &keys(), &noor_scope, &imap("noor@example.com")).unwrap();
        let accounts = imap_accounts(&store).unwrap();
        let listed: Vec<&str> = accounts
            .iter()
            .map(|account| account.address.as_str())
            .collect();
        assert_eq!(listed, ["mira@example.com", "noor@example.com"]);
        let owner = owner_scope(&store, &second.id).unwrap().unwrap();
        assert_eq!(owner.user_id(), &noor.id);
        assert_eq!(owner.account_id(), Some(&second.id));
        assert_eq!(
            owner_scope(&store, &first.id).unwrap().unwrap().user_id(),
            &mira.id
        );
        assert!(
            owner_scope(&store, &AccountId::from("gone".to_owned()))
                .unwrap()
                .is_none()
        );
    }
}
