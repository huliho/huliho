// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Account linking, listing and removal under the scope rules.

use huliho_server::accounts::{self, AccountKind, AuthMethod};
use huliho_server::identity::{self, User};
use huliho_server::ids::AccountId;
use huliho_server::scope::{self, Scope};
use huliho_server::store::{Store, StoreError};

fn store() -> Store {
    Store::in_memory().expect("in-memory store opens")
}

fn personal(store: &Store, login: &str) -> User {
    identity::create_personal_user(store, login)
        .expect("personal user creates")
        .1
}

fn scope_of(store: &Store, user: &User) -> Scope {
    scope::resolve(store, &user.id, None).expect("scope resolves")
}

#[test]
fn linking_lists_and_reads_within_scope() {
    let store = store();
    let user = personal(&store, "mira@example.com");
    let scope = scope_of(&store, &user);
    let account = accounts::link(&store, &scope, AccountKind::Jmap, AuthMethod::Bearer).unwrap();
    let listed = accounts::list(&store, &scope).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, account.id);
    let scoped = scope::resolve(&store, &user.id, Some(&account.id)).unwrap();
    let read = accounts::get(&store, &scoped).unwrap();
    assert_eq!(read.id, account.id);
    assert_eq!(read.kind, AccountKind::Jmap);
    assert_eq!(read.auth_method, AuthMethod::Bearer);
    assert!(read.stopped_cause.is_none());
}

#[test]
fn an_account_never_resolves_for_another_user() {
    let store = store();
    let alpha = personal(&store, "alpha@example.com");
    let beta = personal(&store, "beta@example.com");
    let account = accounts::link(
        &store,
        &scope_of(&store, &alpha),
        AccountKind::Imap,
        AuthMethod::Password,
    )
    .unwrap();
    let result = scope::resolve(&store, &beta.id, Some(&account.id));
    assert!(matches!(result, Err(StoreError::NotFound)));
}

#[test]
fn a_listing_stays_inside_the_own_scope() {
    let store = store();
    let alpha = personal(&store, "alpha@example.com");
    let beta = personal(&store, "beta@example.com");
    accounts::link(
        &store,
        &scope_of(&store, &alpha),
        AccountKind::Jmap,
        AuthMethod::Password,
    )
    .unwrap();
    assert!(
        accounts::list(&store, &scope_of(&store, &beta))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn resolving_an_unknown_account_is_not_found() {
    let store = store();
    let user = personal(&store, "mira@example.com");
    let unknown = AccountId::from("unknown".to_owned());
    let result = scope::resolve(&store, &user.id, Some(&unknown));
    assert!(matches!(result, Err(StoreError::NotFound)));
}

#[test]
fn an_account_read_needs_an_account_scope() {
    let store = store();
    let user = personal(&store, "mira@example.com");
    let scope = scope_of(&store, &user);
    let result = accounts::get(&store, &scope);
    assert!(matches!(result, Err(StoreError::MissingAccount)));
}

#[test]
fn removing_an_account_deletes_the_row() {
    let store = store();
    let user = personal(&store, "mira@example.com");
    let scope = scope_of(&store, &user);
    let account = accounts::link(&store, &scope, AccountKind::Imap, AuthMethod::Oauth2).unwrap();
    let scoped = scope::resolve(&store, &user.id, Some(&account.id)).unwrap();
    accounts::remove(&store, &scoped).unwrap();
    assert!(accounts::list(&store, &scope).unwrap().is_empty());
    let resolved = scope::resolve(&store, &user.id, Some(&account.id));
    assert!(matches!(resolved, Err(StoreError::NotFound)));
    let repeated = accounts::remove(&store, &scoped);
    assert!(matches!(repeated, Err(StoreError::NotFound)));
}
