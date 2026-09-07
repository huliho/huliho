// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! One resolver turns ids into the typed scope every storage call requires.

use rusqlite::OptionalExtension;

use crate::ids::{AccountId, OrganizationId, Role, UserId};
use crate::store::{Store, StoreError};

/// Proof of an authorized (organization, user, account) resolution.
///
/// Fields are private on purpose: the only way to obtain a scope is
/// [`resolve`], so a storage call carrying one has passed authorization.
#[derive(Debug, Clone)]
pub struct Scope {
    organization_id: OrganizationId,
    user_id: UserId,
    role: Role,
    /// An instance property beside the role: whoever holds it writes the
    /// rows the whole instance shares.
    instance_admin: bool,
    account_id: Option<AccountId>,
}

impl Scope {
    #[must_use]
    pub fn organization_id(&self) -> &OrganizationId {
        &self.organization_id
    }

    #[must_use]
    pub fn user_id(&self) -> &UserId {
        &self.user_id
    }

    #[must_use]
    pub fn role(&self) -> Role {
        self.role
    }

    #[must_use]
    pub fn account_id(&self) -> Option<&AccountId> {
        self.account_id.as_ref()
    }

    #[must_use]
    pub fn instance_admin(&self) -> bool {
        self.instance_admin
    }

    pub(crate) fn require(&self, minimum: Role) -> Result<(), StoreError> {
        if self.role >= minimum {
            Ok(())
        } else {
            Err(StoreError::Forbidden)
        }
    }

    pub(crate) fn require_instance_admin(&self) -> Result<(), StoreError> {
        if self.instance_admin {
            Ok(())
        } else {
            Err(StoreError::Forbidden)
        }
    }

    pub(crate) fn account(&self) -> Result<&AccountId, StoreError> {
        self.account_id.as_ref().ok_or(StoreError::MissingAccount)
    }
}

/// Resolves organization, user and optional account into a typed scope.
///
/// # Errors
///
/// Returns [`StoreError::NotFound`] for an unknown user or an account
/// outside the user's scope; database failures pass through.
pub fn resolve(
    store: &Store,
    user_id: &UserId,
    account_id: Option<&AccountId>,
) -> Result<Scope, StoreError> {
    store.read(|connection| {
        let (organization_id, role, instance_admin) = connection
            .query_row(
                "SELECT organization_id, role, instance_admin FROM users WHERE id = ?1",
                [user_id.as_str()],
                |row| {
                    Ok((
                        row.get::<_, OrganizationId>(0)?,
                        row.get::<_, Role>(1)?,
                        row.get::<_, bool>(2)?,
                    ))
                },
            )
            .optional()?
            .ok_or(StoreError::NotFound)?;
        let account_id = match account_id {
            None => None,
            Some(id) => connection
                .query_row(
                    "SELECT id FROM accounts
                     WHERE id = ?1 AND user_id = ?2 AND organization_id = ?3",
                    [id.as_str(), user_id.as_str(), organization_id.as_str()],
                    |row| row.get(0),
                )
                .optional()?
                .ok_or(StoreError::NotFound)
                .map(Some)?,
        };
        Ok(Scope {
            organization_id,
            user_id: user_id.clone(),
            role,
            instance_admin,
            account_id,
        })
    })
}
