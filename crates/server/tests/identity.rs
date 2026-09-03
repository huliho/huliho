// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Organizations, users and roles under the scope rules.

mod organization;

use huliho_server::identity::{self, NewUser};
use huliho_server::ids::{Role, UserId};
use huliho_server::scope;
use huliho_server::store::StoreError;
use organization::{new_user, personal, scope_of, store};

#[test]
fn a_personal_user_owns_a_fresh_organization() {
    let store = store();
    let (organization, user) = personal(&store, "mira@example.com");
    assert_eq!(user.organization_id, organization.id);
    assert_eq!(user.role, Role::Owner);
    let scope = scope_of(&store, &user);
    let read = identity::organization(&store, &scope).unwrap();
    assert_eq!(read.id, organization.id);
    assert_eq!(identity::user(&store, &scope).unwrap().id, user.id);
}

#[test]
fn a_user_carries_a_name_that_starts_as_the_login() {
    let store = store();
    let (_, user) = personal(&store, "mira@example.com");
    assert_eq!(user.name, "mira@example.com");
    assert_eq!(user.last_active_at, None);
    let read = identity::user(&store, &scope_of(&store, &user)).unwrap();
    assert_eq!(read.name, "mira@example.com");
}

#[test]
fn a_duplicate_login_is_rejected() {
    let store = store();
    personal(&store, "mira@example.com");
    let result = identity::create_personal_user(&store, "mira@example.com");
    assert!(matches!(result, Err(StoreError::LoginTaken)));
}

#[test]
fn a_taken_login_is_reported_as_such() {
    let store = store();
    let (_, owner) = personal(&store, "owner@example.com");
    let owner_scope = scope_of(&store, &owner);
    let result = identity::create_organization_user(
        &store,
        &owner_scope,
        &NewUser {
            login: "owner@example.com".to_owned(),
            name: "Owner Again".to_owned(),
            role: Role::Member,
        },
    );
    assert!(matches!(result, Err(StoreError::LoginTaken)));
}

#[test]
fn a_created_user_carries_its_name() {
    let store = store();
    let (_, owner) = personal(&store, "owner@example.com");
    let owner_scope = scope_of(&store, &owner);
    let created = identity::create_organization_user(
        &store,
        &owner_scope,
        &NewUser {
            login: "jonas".to_owned(),
            name: "Jonas Verhulst".to_owned(),
            role: Role::Member,
        },
    )
    .unwrap();
    assert_eq!(created.name, "Jonas Verhulst");
    assert_eq!(created.login, "jonas");
    assert_eq!(created.role, Role::Member);
}

#[test]
fn resolving_an_unknown_user_is_not_found() {
    let store = store();
    let unknown = UserId::from("unknown".to_owned());
    let result = scope::resolve(&store, &unknown, None);
    assert!(matches!(result, Err(StoreError::NotFound)));
}

#[test]
fn a_user_listing_stays_inside_the_own_organization() {
    let store = store();
    let (_, alpha) = personal(&store, "alpha@example.com");
    let (_, beta) = personal(&store, "beta@example.com");
    let listed = identity::users(&store, &scope_of(&store, &alpha)).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, alpha.id);
    let listed = identity::users(&store, &scope_of(&store, &beta)).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, beta.id);
}

#[test]
fn a_member_cannot_manage_users() {
    let store = store();
    let (_, owner) = personal(&store, "owner@example.com");
    let owner_scope = scope_of(&store, &owner);
    let member = identity::create_organization_user(
        &store,
        &owner_scope,
        &new_user("member@example.com", Role::Member),
    )
    .unwrap();
    let member_scope = scope_of(&store, &member);
    let listed = identity::users(&store, &member_scope);
    assert!(matches!(listed, Err(StoreError::Forbidden)));
    let created = identity::create_organization_user(
        &store,
        &member_scope,
        &new_user("new@example.com", Role::Member),
    );
    assert!(matches!(created, Err(StoreError::Forbidden)));
    let changed = identity::change_role(&store, &member_scope, &owner.id, Role::Member);
    assert!(matches!(changed, Err(StoreError::Forbidden)));
}

#[test]
fn a_role_change_stops_at_the_organization_border() {
    let store = store();
    let (_, alpha) = personal(&store, "alpha@example.com");
    let (_, beta) = personal(&store, "beta@example.com");
    let result = identity::change_role(&store, &scope_of(&store, &alpha), &beta.id, Role::Member);
    assert!(matches!(result, Err(StoreError::NotFound)));
}

#[test]
fn an_admin_grants_no_role_above_their_own() {
    let store = store();
    let (_, owner) = personal(&store, "owner@example.com");
    let owner_scope = scope_of(&store, &owner);
    let admin = identity::create_organization_user(
        &store,
        &owner_scope,
        &new_user("admin@example.com", Role::Admin),
    )
    .unwrap();
    let admin_scope = scope_of(&store, &admin);
    let created = identity::create_organization_user(
        &store,
        &admin_scope,
        &new_user("new@example.com", Role::Owner),
    );
    assert!(matches!(created, Err(StoreError::Forbidden)));
    let demoted = identity::change_role(&store, &admin_scope, &owner.id, Role::Member);
    assert!(matches!(demoted, Err(StoreError::Forbidden)));
}

#[test]
fn the_last_owner_stays() {
    let store = store();
    let (_, owner) = personal(&store, "owner@example.com");
    let owner_scope = scope_of(&store, &owner);
    let result = identity::change_role(&store, &owner_scope, &owner.id, Role::Member);
    assert!(matches!(result, Err(StoreError::LastOwner)));
}

#[test]
fn an_owner_demotes_once_another_owner_exists() {
    let store = store();
    let (_, first) = personal(&store, "first@example.com");
    let first_scope = scope_of(&store, &first);
    identity::create_organization_user(
        &store,
        &first_scope,
        &new_user("second@example.com", Role::Owner),
    )
    .unwrap();
    let changed = identity::change_role(&store, &first_scope, &first.id, Role::Member).unwrap();
    assert_eq!(changed.role, Role::Member);
}

#[test]
fn a_role_change_to_the_same_role_is_a_no_op() {
    let store = store();
    let (_, owner) = personal(&store, "owner@example.com");
    let owner_scope = scope_of(&store, &owner);
    let unchanged = identity::change_role(&store, &owner_scope, &owner.id, Role::Owner).unwrap();
    assert_eq!(unchanged.role, Role::Owner);
}
