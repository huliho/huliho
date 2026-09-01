// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Organizations and users: creation, listing and role changes.

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::events::{Actor, DomainEvent, append};
use crate::ids::{OrganizationId, Role, UserId};
use crate::scope::Scope;
use crate::store::{Store, StoreError, now_ms};

/// An organization row.
#[derive(Debug, Clone)]
pub struct Organization {
    pub id: OrganizationId,
    pub name: String,
    pub created_at: i64,
}

/// A user row without credential material.
#[derive(Debug, Clone)]
pub struct User {
    pub id: UserId,
    pub organization_id: OrganizationId,
    pub login: String,
    pub role: Role,
    pub external_issuer: Option<String>,
    pub external_subject: Option<String>,
    pub created_at: i64,
}

const USER_COLUMNS: &str =
    "id, organization_id, login, role, external_issuer, external_subject, created_at";

/// Creates a user owning a fresh personal organization. This is the only
/// way a user comes into existence outside an existing organization.
///
/// # Errors
///
/// Returns an error on a duplicate login or a database failure.
pub fn create_personal_user(
    store: &Store,
    login: &str,
) -> Result<(Organization, User), StoreError> {
    store.write(|transaction| {
        let organization = Organization {
            id: OrganizationId::generate(),
            name: login.to_owned(),
            created_at: now_ms(),
        };
        transaction.execute(
            "INSERT INTO organizations (id, name, created_at) VALUES (?1, ?2, ?3)",
            params![
                organization.id.as_str(),
                organization.name,
                organization.created_at
            ],
        )?;
        let user = insert_user(transaction, &organization.id, login, Role::Owner)?;
        append(
            transaction,
            &organization.id,
            &Actor::System,
            &DomainEvent::OrganizationCreated {},
        )?;
        let created = DomainEvent::UserCreated {
            user_id: user.id.clone(),
            role: user.role,
        };
        append(transaction, &organization.id, &Actor::System, &created)?;
        Ok((organization, user))
    })
}

/// Creates a user inside the scope's organization. Requires the admin
/// role; the granted role never exceeds the actor's own.
///
/// # Errors
///
/// Returns [`StoreError::Forbidden`] below the admin role or when the
/// granted role exceeds the actor's; duplicate logins and database
/// failures pass through.
pub fn create_organization_user(
    store: &Store,
    scope: &Scope,
    login: &str,
    role: Role,
) -> Result<User, StoreError> {
    scope.require(Role::Admin)?;
    if role > scope.role() {
        return Err(StoreError::Forbidden);
    }
    store.write(|transaction| {
        let user = insert_user(transaction, scope.organization_id(), login, role)?;
        let created = DomainEvent::UserCreated {
            user_id: user.id.clone(),
            role,
        };
        let actor = Actor::User(scope.user_id().clone());
        append(transaction, scope.organization_id(), &actor, &created)?;
        Ok(user)
    })
}

/// Reads the scope's organization.
///
/// # Errors
///
/// Returns an error when the database fails.
pub fn organization(store: &Store, scope: &Scope) -> Result<Organization, StoreError> {
    store.read(|connection| {
        connection
            .query_row(
                "SELECT id, name, created_at FROM organizations WHERE id = ?1",
                [scope.organization_id().as_str()],
                |row| {
                    Ok(Organization {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        created_at: row.get(2)?,
                    })
                },
            )
            .optional()?
            .ok_or(StoreError::NotFound)
    })
}

/// Reads the scope's own user row.
///
/// # Errors
///
/// Returns an error when the database fails.
pub fn user(store: &Store, scope: &Scope) -> Result<User, StoreError> {
    store.read(|connection| {
        connection
            .query_row(
                &format!("SELECT {USER_COLUMNS} FROM users WHERE id = ?1 AND organization_id = ?2"),
                [scope.user_id().as_str(), scope.organization_id().as_str()],
                user_from_row,
            )
            .optional()?
            .ok_or(StoreError::NotFound)
    })
}

/// Lists the organization's users. Requires the admin role.
///
/// # Errors
///
/// Returns [`StoreError::Forbidden`] below the admin role; database
/// failures pass through.
pub fn users(store: &Store, scope: &Scope) -> Result<Vec<User>, StoreError> {
    scope.require(Role::Admin)?;
    store.read(|connection| {
        let mut statement = connection.prepare(&format!(
            "SELECT {USER_COLUMNS} FROM users WHERE organization_id = ?1 ORDER BY login"
        ))?;
        let rows = statement
            .query_map([scope.organization_id().as_str()], user_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

/// Changes a role within the scope's organization. Requires the admin
/// role. Neither the target's current role nor the new one may exceed
/// the actor's own and the last owner stays.
///
/// # Errors
///
/// Returns [`StoreError::NotFound`] for a target outside the
/// organization, [`StoreError::Forbidden`] on a role ceiling violation
/// and [`StoreError::LastOwner`] when demoting the only owner.
pub fn change_role(
    store: &Store,
    scope: &Scope,
    target: &UserId,
    to: Role,
) -> Result<User, StoreError> {
    scope.require(Role::Admin)?;
    if to > scope.role() {
        return Err(StoreError::Forbidden);
    }
    store.write(|transaction| {
        let current = transaction
            .query_row(
                &format!("SELECT {USER_COLUMNS} FROM users WHERE id = ?1 AND organization_id = ?2"),
                [target.as_str(), scope.organization_id().as_str()],
                user_from_row,
            )
            .optional()?
            .ok_or(StoreError::NotFound)?;
        if current.role > scope.role() {
            return Err(StoreError::Forbidden);
        }
        if current.role == to {
            return Ok(current);
        }
        if current.role == Role::Owner {
            ensure_another_owner(transaction, scope.organization_id(), target)?;
        }
        transaction.execute(
            "UPDATE users SET role = ?1 WHERE id = ?2",
            params![to.as_str(), target.as_str()],
        )?;
        let changed = DomainEvent::UserRoleChanged {
            user_id: current.id.clone(),
            from: current.role,
            to,
        };
        let actor = Actor::User(scope.user_id().clone());
        append(transaction, scope.organization_id(), &actor, &changed)?;
        Ok(User {
            role: to,
            ..current
        })
    })
}

fn ensure_another_owner(
    connection: &Connection,
    organization_id: &OrganizationId,
    excluded: &UserId,
) -> Result<(), StoreError> {
    let others: i64 = connection.query_row(
        "SELECT COUNT(*) FROM users
         WHERE organization_id = ?1 AND role = 'owner' AND id <> ?2",
        [organization_id.as_str(), excluded.as_str()],
        |row| row.get(0),
    )?;
    if others == 0 {
        return Err(StoreError::LastOwner);
    }
    Ok(())
}

fn insert_user(
    connection: &Connection,
    organization_id: &OrganizationId,
    login: &str,
    role: Role,
) -> Result<User, StoreError> {
    let user = User {
        id: UserId::generate(),
        organization_id: organization_id.clone(),
        login: login.to_owned(),
        role,
        external_issuer: None,
        external_subject: None,
        created_at: now_ms(),
    };
    connection.execute(
        "INSERT INTO users (id, organization_id, login, role, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            user.id.as_str(),
            organization_id.as_str(),
            user.login,
            role.as_str(),
            user.created_at
        ],
    )?;
    Ok(user)
}

fn user_from_row(row: &Row<'_>) -> rusqlite::Result<User> {
    Ok(User {
        id: row.get(0)?,
        organization_id: row.get(1)?,
        login: row.get(2)?,
        role: row.get(3)?,
        external_issuer: row.get(4)?,
        external_subject: row.get(5)?,
        created_at: row.get(6)?,
    })
}
