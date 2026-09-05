// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The append-only domain event log: lifecycle facts per organization.

use std::sync::Arc;
use std::time::Duration;

use rusqlite::{Connection, params};
use serde::Serialize;

use crate::accounts::{AccountKind, StopCause};
use crate::ids::{AccountId, OrganizationId, Role, UserId};
use crate::scope::Scope;
use crate::store::{Store, StoreError, now_ms};

/// Payload shape version stamped on every appended record.
const PAYLOAD_SCHEMA_VERSION: i64 = 1;

const MS_PER_DAY: i64 = 86_400_000;

/// Retention is day-grained, so a daily pruning pass suffices.
const PRUNE_INTERVAL: Duration = Duration::from_hours(24);

/// Who caused an event: a user in the organization or the system itself.
#[derive(Debug)]
pub enum Actor {
    System,
    User(UserId),
}

impl Actor {
    fn as_str(&self) -> &str {
        match self {
            Self::System => "system",
            Self::User(id) => id.as_str(),
        }
    }
}

/// Typed payloads; the variant fixes the stable hierarchical type name.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum DomainEvent {
    OrganizationCreated {},
    UserCreated {
        user_id: UserId,
        role: Role,
    },
    UserRoleChanged {
        user_id: UserId,
        from: Role,
        to: Role,
    },
    UserActive {
        user_id: UserId,
        period: String,
    },
    UserPasswordChanged {
        user_id: UserId,
    },
    UserPasswordReset {
        user_id: UserId,
    },
    AccountLinked {
        account_id: AccountId,
        kind: AccountKind,
    },
    AccountRemoved {
        account_id: AccountId,
    },
    AccountStopped {
        account_id: AccountId,
        cause: StopCause,
    },
    AccountResumed {
        account_id: AccountId,
    },
    AccountCredentialsUpdated {
        account_id: AccountId,
    },
    SessionCreated {},
    SessionRevoked {
        user_id: UserId,
    },
    SessionExpired {
        user_id: UserId,
    },
    LoginFailed {
        user_id: UserId,
    },
    LogPruned {
        removed: u64,
        cutoff_ms: i64,
    },
}

impl DomainEvent {
    #[must_use]
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::OrganizationCreated {} => "organization.created",
            Self::UserCreated { .. } => "user.created",
            Self::UserRoleChanged { .. } => "user.role_changed",
            Self::UserActive { .. } => "user.active",
            Self::UserPasswordChanged { .. } => "user.password_changed",
            Self::UserPasswordReset { .. } => "user.password_reset",
            Self::AccountLinked { .. } => "account.linked",
            Self::AccountRemoved { .. } => "account.removed",
            Self::AccountStopped { .. } => "account.stopped",
            Self::AccountResumed { .. } => "account.resumed",
            Self::AccountCredentialsUpdated { .. } => "account.credentials_updated",
            Self::SessionCreated {} => "session.created",
            Self::SessionRevoked { .. } => "session.revoked",
            Self::SessionExpired { .. } => "session.expired",
            Self::LoginFailed { .. } => "login.failed",
            Self::LogPruned { .. } => "log.pruned",
        }
    }
}

/// One stored event row.
#[derive(Debug)]
pub struct EventRecord {
    pub id: i64,
    pub organization_id: OrganizationId,
    pub actor: String,
    pub event_type: String,
    pub schema_version: i64,
    pub payload: String,
    pub created_at: i64,
}

pub(crate) fn append(
    connection: &Connection,
    organization_id: &OrganizationId,
    actor: &Actor,
    event: &DomainEvent,
) -> Result<(), StoreError> {
    let payload = serde_json::to_string(event)?;
    connection.execute(
        "INSERT INTO domain_events
         (organization_id, actor, event_type, schema_version, payload, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            organization_id.as_str(),
            actor.as_str(),
            event.event_type(),
            PAYLOAD_SCHEMA_VERSION,
            payload,
            now_ms()
        ],
    )?;
    Ok(())
}

/// Lists the organization's events oldest first. Requires the admin role.
///
/// # Errors
///
/// Returns [`StoreError::Forbidden`] below the admin role; database
/// failures pass through.
pub fn for_organization(store: &Store, scope: &Scope) -> Result<Vec<EventRecord>, StoreError> {
    scope.require(Role::Admin)?;
    store.read(|connection| {
        let mut statement = connection.prepare(
            "SELECT id, organization_id, actor, event_type, schema_version, payload, created_at
             FROM domain_events WHERE organization_id = ?1 ORDER BY id",
        )?;
        let records = statement
            .query_map([scope.organization_id().as_str()], |row| {
                Ok(EventRecord {
                    id: row.get(0)?,
                    organization_id: row.get(1)?,
                    actor: row.get(2)?,
                    event_type: row.get(3)?,
                    schema_version: row.get(4)?,
                    payload: row.get(5)?,
                    created_at: row.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(records)
    })
}

/// Removes events past the retention window and records the removal per
/// organization.
///
/// # Errors
///
/// Returns an error when the database fails; the transaction then rolls
/// back and nothing is removed.
pub fn prune(store: &Store, retention_days: u32) -> Result<u64, StoreError> {
    store.write(|transaction| {
        let cutoff_ms = now_ms().saturating_sub(i64::from(retention_days) * MS_PER_DAY);
        let counts = removed_per_organization(transaction, cutoff_ms)?;
        transaction.execute(
            "DELETE FROM domain_events WHERE created_at < ?1",
            [cutoff_ms],
        )?;
        let mut total = 0;
        for (organization_id, count) in counts {
            let removed = u64::try_from(count).unwrap_or_default();
            total += removed;
            let event = DomainEvent::LogPruned { removed, cutoff_ms };
            append(transaction, &organization_id, &Actor::System, &event)?;
        }
        Ok(total)
    })
}

fn removed_per_organization(
    connection: &Connection,
    cutoff_ms: i64,
) -> Result<Vec<(OrganizationId, i64)>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT organization_id, COUNT(*) FROM domain_events
         WHERE created_at < ?1 GROUP BY organization_id",
    )?;
    let counts = statement
        .query_map([cutoff_ms], |row| {
            Ok((row.get::<_, OrganizationId>(0)?, row.get::<_, i64>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(counts)
}

/// Prunes at startup and then daily; failures are logged and retried at
/// the next tick.
pub async fn prune_periodically(store: Arc<Store>, retention_days: u32) {
    let mut interval = tokio::time::interval(PRUNE_INTERVAL);
    loop {
        interval.tick().await;
        let store = Arc::clone(&store);
        match tokio::task::spawn_blocking(move || prune(&store, retention_days)).await {
            Ok(Ok(0)) => {}
            Ok(Ok(removed)) => tracing::info!(removed, "pruned expired domain events"),
            Ok(Err(error)) => tracing::warn!(%error, "domain event pruning failed"),
            Err(error) => tracing::warn!(%error, "domain event pruning task failed"),
        }
    }
}
