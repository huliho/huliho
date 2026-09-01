// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Connected mail accounts within a user's scope.

use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ValueRef};
use rusqlite::{OptionalExtension, Row, params};
use serde::Serialize;

use crate::events::{Actor, DomainEvent, append};
use crate::ids::{AccountId, OrganizationId, UserId};
use crate::scope::Scope;
use crate::store::{Store, StoreError, now_ms};

/// The protocol family an account speaks upstream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AccountKind {
    Jmap,
    Imap,
}

impl AccountKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Jmap => "jmap",
            Self::Imap => "imap",
        }
    }
}

impl FromSql for AccountKind {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        match String::column_result(value)?.as_str() {
            "jmap" => Ok(Self::Jmap),
            "imap" => Ok(Self::Imap),
            other => Err(FromSqlError::Other(
                format!("unknown account kind {other}").into(),
            )),
        }
    }
}

/// How the account authenticates upstream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthMethod {
    Password,
    Bearer,
    Oauth2,
}

impl AuthMethod {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Password => "password",
            Self::Bearer => "bearer",
            Self::Oauth2 => "oauth2",
        }
    }
}

impl FromSql for AuthMethod {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        match String::column_result(value)?.as_str() {
            "password" => Ok(Self::Password),
            "bearer" => Ok(Self::Bearer),
            "oauth2" => Ok(Self::Oauth2),
            other => Err(FromSqlError::Other(
                format!("unknown auth method {other}").into(),
            )),
        }
    }
}

/// An account row without credential material.
#[derive(Debug, Clone)]
pub struct Account {
    pub id: AccountId,
    pub organization_id: OrganizationId,
    pub user_id: UserId,
    pub kind: AccountKind,
    pub auth_method: AuthMethod,
    pub stopped_cause: Option<String>,
    pub stopped_at: Option<i64>,
    pub created_at: i64,
}

const ACCOUNT_COLUMNS: &str =
    "id, organization_id, user_id, kind, auth_method, stopped_cause, stopped_at, created_at";

/// Links a new account to the scope's user.
///
/// # Errors
///
/// Returns an error when the database fails.
pub fn link(
    store: &Store,
    scope: &Scope,
    kind: AccountKind,
    auth_method: AuthMethod,
) -> Result<Account, StoreError> {
    store.write(|transaction| {
        let account = Account {
            id: AccountId::generate(),
            organization_id: scope.organization_id().clone(),
            user_id: scope.user_id().clone(),
            kind,
            auth_method,
            stopped_cause: None,
            stopped_at: None,
            created_at: now_ms(),
        };
        transaction.execute(
            "INSERT INTO accounts (id, organization_id, user_id, kind, auth_method, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                account.id.as_str(),
                account.organization_id.as_str(),
                account.user_id.as_str(),
                kind.as_str(),
                auth_method.as_str(),
                account.created_at
            ],
        )?;
        let linked = DomainEvent::AccountLinked {
            account_id: account.id.clone(),
            kind,
        };
        let actor = Actor::User(scope.user_id().clone());
        append(transaction, scope.organization_id(), &actor, &linked)?;
        Ok(account)
    })
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

/// Removes the account the scope was resolved for, with its snooze rows.
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
        kind: row.get(3)?,
        auth_method: row.get(4)?,
        stopped_cause: row.get(5)?,
        stopped_at: row.get(6)?,
        created_at: row.get(7)?,
    })
}
