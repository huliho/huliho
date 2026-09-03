// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The domain event log: recording, isolation and retention.

mod organization;

use std::time::Duration;

use huliho_server::accounts::{self, AccountKind, AuthMethod};
use huliho_server::events;
use huliho_server::identity::{self, NewUser};
use huliho_server::ids::{Role, UserId};
use huliho_server::scope;
use huliho_server::store::StoreError;
use organization::{new_user, personal, scope_of, store};

/// Long enough that earlier rows sit strictly before a fresh cutoff.
const CLOCK_TICK: Duration = Duration::from_millis(10);

#[test]
fn lifecycle_facts_land_in_order_with_monotonic_ids() {
    let store = store();
    let (organization, owner) = personal(&store, "owner@example.com");
    let owner_scope = scope_of(&store, &owner);
    let member = identity::create_organization_user(
        &store,
        &owner_scope,
        &new_user("member@example.com", Role::Member),
    )
    .unwrap();
    identity::change_role(&store, &owner_scope, &member.id, Role::Admin).unwrap();
    let account =
        accounts::link(&store, &owner_scope, AccountKind::Jmap, AuthMethod::Bearer).unwrap();
    let scoped = scope::resolve(&store, &owner.id, Some(&account.id)).unwrap();
    accounts::remove(&store, &scoped).unwrap();

    let records = events::for_organization(&store, &owner_scope).unwrap();
    let types: Vec<&str> = records
        .iter()
        .map(|record| record.event_type.as_str())
        .collect();
    assert_eq!(
        types,
        [
            "organization.created",
            "user.created",
            "user.created",
            "user.role_changed",
            "account.linked",
            "account.removed"
        ]
    );
    assert!(records.windows(2).all(|pair| pair[0].id < pair[1].id));
    assert!(records.iter().all(|record| record.schema_version == 1));
    assert!(
        records
            .iter()
            .all(|record| record.organization_id == organization.id)
    );
}

#[test]
fn actors_are_the_system_or_the_acting_user() {
    let store = store();
    let (_, owner) = personal(&store, "owner@example.com");
    let owner_scope = scope_of(&store, &owner);
    identity::create_organization_user(
        &store,
        &owner_scope,
        &new_user("member@example.com", Role::Member),
    )
    .unwrap();

    let records = events::for_organization(&store, &owner_scope).unwrap();
    assert_eq!(records[0].actor, "system");
    assert_eq!(records[1].actor, "system");
    assert_eq!(records[2].actor, owner.id.as_str());
}

#[test]
fn payloads_carry_no_login() {
    let store = store();
    let (_, owner) = personal(&store, "owner@example.com");
    let owner_scope = scope_of(&store, &owner);
    identity::create_organization_user(
        &store,
        &owner_scope,
        &NewUser {
            login: "member@example.com".to_owned(),
            name: "Jonas Verhulst".to_owned(),
            role: Role::Member,
        },
    )
    .unwrap();

    let records = events::for_organization(&store, &owner_scope).unwrap();
    assert!(
        records
            .iter()
            .all(|record| !record.payload.contains("example.com"))
    );
    assert!(
        records
            .iter()
            .all(|record| !record.payload.contains("Jonas"))
    );
}

#[test]
fn a_member_reads_no_event_log() {
    let store = store();
    let (_, owner) = personal(&store, "owner@example.com");
    let owner_scope = scope_of(&store, &owner);
    let member = identity::create_organization_user(
        &store,
        &owner_scope,
        &new_user("member@example.com", Role::Member),
    )
    .unwrap();
    let result = events::for_organization(&store, &scope_of(&store, &member));
    assert!(matches!(result, Err(StoreError::Forbidden)));
}

#[test]
fn the_log_stays_inside_the_organization() {
    let store = store();
    let (_, alpha) = personal(&store, "alpha@example.com");
    let (organization, beta) = personal(&store, "beta@example.com");
    accounts::link(
        &store,
        &scope_of(&store, &alpha),
        AccountKind::Jmap,
        AuthMethod::Bearer,
    )
    .unwrap();

    let records = events::for_organization(&store, &scope_of(&store, &beta)).unwrap();
    assert_eq!(records.len(), 2);
    assert!(
        records
            .iter()
            .all(|record| record.organization_id == organization.id)
    );
}

#[test]
fn pruning_removes_only_expired_rows_and_records_itself() {
    let store = store();
    let (_, owner) = personal(&store, "owner@example.com");
    let owner_scope = scope_of(&store, &owner);
    assert_eq!(events::prune(&store, 365).unwrap(), 0);
    std::thread::sleep(CLOCK_TICK);
    assert_eq!(events::prune(&store, 0).unwrap(), 2);

    let records = events::for_organization(&store, &owner_scope).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].event_type, "log.pruned");
    assert_eq!(records[0].actor, "system");
    assert!(records[0].payload.contains("\"removed\":2"));
}

#[test]
fn the_new_lifecycle_types_carry_stable_names() {
    use huliho_server::events::DomainEvent;
    let user_id = UserId::from("u".to_owned());
    let named = [
        (
            DomainEvent::SessionRevoked {
                user_id: user_id.clone(),
            },
            "session.revoked",
        ),
        (
            DomainEvent::SessionExpired {
                user_id: user_id.clone(),
            },
            "session.expired",
        ),
        (
            DomainEvent::UserPasswordChanged {
                user_id: user_id.clone(),
            },
            "user.password_changed",
        ),
        (
            DomainEvent::UserPasswordReset {
                user_id: user_id.clone(),
            },
            "user.password_reset",
        ),
        (
            DomainEvent::UserActive {
                user_id,
                period: "2026-09".to_owned(),
            },
            "user.active",
        ),
    ];
    for (event, name) in named {
        assert_eq!(event.event_type(), name);
    }
}

#[test]
fn pruning_counts_per_organization() {
    let store = store();
    let (_, alpha) = personal(&store, "alpha@example.com");
    let (_, beta) = personal(&store, "beta@example.com");
    let beta_scope = scope_of(&store, &beta);
    identity::create_organization_user(
        &store,
        &beta_scope,
        &new_user("member@example.com", Role::Member),
    )
    .unwrap();
    std::thread::sleep(CLOCK_TICK);
    assert_eq!(events::prune(&store, 0).unwrap(), 5);

    let alpha_records = events::for_organization(&store, &scope_of(&store, &alpha)).unwrap();
    assert_eq!(alpha_records.len(), 1);
    assert!(alpha_records[0].payload.contains("\"removed\":2"));
    let beta_records = events::for_organization(&store, &beta_scope).unwrap();
    assert_eq!(beta_records.len(), 1);
    assert!(beta_records[0].payload.contains("\"removed\":3"));
}
