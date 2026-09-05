// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Connected mail accounts within a user's scope: the plain row, the
//! credential sealed beside it and the settings it connects with.

mod credentials;
mod settings;

use rusqlite::{OptionalExtension, Row, params};

pub use credentials::Credential;
pub use settings::{AccountSettings, Endpoint, TlsMode};

use crate::events::{Actor, DomainEvent, append};
use crate::ids::{AccountId, OrganizationId, UserId, text_enum};
use crate::scope::Scope;
use crate::secrets::Keys;
use crate::store::{Store, StoreError, now_ms};

text_enum!(
    /// The protocol family an account speaks upstream.
    AccountKind {
        Jmap => "jmap",
        Imap => "imap",
    }
);

text_enum!(
    /// How the account authenticates upstream.
    AuthMethod {
        Password => "password",
        Bearer => "bearer",
        Oauth2 => "oauth2",
    }
);

text_enum!(
    /// The preset an account was added under; it picks the copy and the
    /// credential kind.
    Provider {
        Gmail => "gmail",
        Microsoft => "microsoft",
        Fastmail => "fastmail",
        Icloud => "icloud",
        Yahoo => "yahoo",
        Generic => "generic",
    }
);

text_enum!(
    /// Why an account stopped: the user acts on a credential, the probe
    /// on a connection.
    StopCause {
        Credentials => "credentials",
        Connection => "connection",
    }
);

/// An account row as the list shows it: no credential, no settings.
#[derive(Debug, Clone)]
pub struct Account {
    pub id: AccountId,
    pub organization_id: OrganizationId,
    pub user_id: UserId,
    pub address: String,
    pub name: String,
    pub provider: Provider,
    pub kind: AccountKind,
    pub auth_method: AuthMethod,
    pub stopped_cause: Option<StopCause>,
    pub stopped_at: Option<i64>,
    pub created_at: i64,
}

/// What a user connects: the plain columns plus the credential that
/// gets sealed.
#[derive(Debug, Clone)]
pub struct NewAccount {
    pub address: String,
    pub name: String,
    pub provider: Provider,
    pub settings: AccountSettings,
    pub credential: Credential,
}

const ACCOUNT_COLUMNS: &str = "id, organization_id, user_id, address, name, provider, kind, \
                               auth_method, stopped_cause, stopped_at, created_at";

/// Adds an account to the scope's user, its credential sealed under the
/// new row's id.
///
/// # Errors
///
/// Returns an error when sealing or the database fails.
pub fn add(
    store: &Store,
    keys: &Keys,
    scope: &Scope,
    new: &NewAccount,
) -> Result<Account, StoreError> {
    let account = Account {
        id: AccountId::generate(),
        organization_id: scope.organization_id().clone(),
        user_id: scope.user_id().clone(),
        address: new.address.clone(),
        name: new.name.clone(),
        provider: new.provider,
        kind: new.settings.kind(),
        auth_method: new.credential.auth_method(),
        stopped_cause: None,
        stopped_at: None,
        created_at: now_ms(),
    };
    let settings = serde_json::to_string(&new.settings)?;
    let sealed = credentials::seal(keys, &account.id, &new.credential)?;
    store.write(|transaction| {
        transaction.execute(
            "INSERT INTO accounts
             (id, organization_id, user_id, address, name, provider, kind, auth_method,
              settings, credentials, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                account.id.as_str(),
                account.organization_id.as_str(),
                account.user_id.as_str(),
                account.address,
                account.name,
                account.provider.as_str(),
                account.kind.as_str(),
                account.auth_method.as_str(),
                settings,
                sealed,
                account.created_at
            ],
        )?;
        let linked = DomainEvent::AccountLinked {
            account_id: account.id.clone(),
            kind: account.kind,
        };
        let actor = Actor::User(scope.user_id().clone());
        append(transaction, scope.organization_id(), &actor, &linked)
    })?;
    Ok(account)
}

/// Lists the scope user's own accounts.
///
/// # Errors
///
/// Returns an error when the database fails.
pub fn list(store: &Store, scope: &Scope) -> Result<Vec<Account>, StoreError> {
    store.read(|connection| {
        let mut statement = connection.prepare(&format!(
            "SELECT {ACCOUNT_COLUMNS} FROM accounts
             WHERE user_id = ?1 AND organization_id = ?2 ORDER BY created_at, id"
        ))?;
        let rows = statement
            .query_map(
                [scope.user_id().as_str(), scope.organization_id().as_str()],
                account_from_row,
            )?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

/// Reads the account the scope was resolved for.
///
/// # Errors
///
/// Returns [`StoreError::MissingAccount`] for a scope without an account
/// and [`StoreError::NotFound`] when the row is gone.
pub fn get(store: &Store, scope: &Scope) -> Result<Account, StoreError> {
    let account_id = scope.account()?;
    store.read(|connection| {
        connection
            .query_row(
                &format!(
                    "SELECT {ACCOUNT_COLUMNS} FROM accounts
                     WHERE id = ?1 AND user_id = ?2 AND organization_id = ?3"
                ),
                [
                    account_id.as_str(),
                    scope.user_id().as_str(),
                    scope.organization_id().as_str(),
                ],
                account_from_row,
            )
            .optional()?
            .ok_or(StoreError::NotFound)
    })
}

/// Opens the credential sealed on the account the scope was resolved
/// for.
///
/// # Errors
///
/// Returns [`StoreError::MissingAccount`] for a scope without an
/// account, [`StoreError::NotFound`] when the row is gone and
/// [`StoreError::Tampered`] when the row carries no blob or the blob was
/// sealed for another row or under another secret.
pub fn credential(store: &Store, keys: &Keys, scope: &Scope) -> Result<Credential, StoreError> {
    let account_id = scope.account()?;
    let blob: Option<Vec<u8>> = store.read(|connection| {
        connection
            .query_row(
                "SELECT credentials FROM accounts
                 WHERE id = ?1 AND user_id = ?2 AND organization_id = ?3",
                [
                    account_id.as_str(),
                    scope.user_id().as_str(),
                    scope.organization_id().as_str(),
                ],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(StoreError::NotFound)
    })?;
    blob.and_then(|blob| credentials::open(keys, account_id, &blob))
        .ok_or(StoreError::Tampered)
}

/// Removes the account the scope was resolved for; the sealed credential
/// leaves with the row and the snooze rows cascade.
///
/// # Errors
///
/// Returns [`StoreError::MissingAccount`] for a scope without an account
/// and [`StoreError::NotFound`] when the row is gone.
pub fn remove(store: &Store, scope: &Scope) -> Result<(), StoreError> {
    let account_id = scope.account()?.clone();
    store.write(|transaction| {
        let removed = transaction.execute(
            "DELETE FROM accounts WHERE id = ?1 AND user_id = ?2 AND organization_id = ?3",
            [
                account_id.as_str(),
                scope.user_id().as_str(),
                scope.organization_id().as_str(),
            ],
        )?;
        if removed == 0 {
            return Err(StoreError::NotFound);
        }
        let event = DomainEvent::AccountRemoved {
            account_id: account_id.clone(),
        };
        let actor = Actor::User(scope.user_id().clone());
        append(transaction, scope.organization_id(), &actor, &event)?;
        Ok(())
    })
}

fn account_from_row(row: &Row<'_>) -> rusqlite::Result<Account> {
    Ok(Account {
        id: row.get(0)?,
        organization_id: row.get(1)?,
        user_id: row.get(2)?,
        address: row.get(3)?,
        name: row.get(4)?,
        provider: row.get(5)?,
        kind: row.get(6)?,
        auth_method: row.get(7)?,
        stopped_cause: row.get(8)?,
        stopped_at: row.get(9)?,
        created_at: row.get(10)?,
    })
}
