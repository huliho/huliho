// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Fixtures for tests that work on organizations and users directly.

use huliho_server::identity::{self, NewUser, Organization, User};
use huliho_server::ids::Role;
use huliho_server::scope::{self, Scope};
use huliho_server::store::Store;

pub fn store() -> Store {
    Store::in_memory().expect("in-memory store opens")
}

pub fn personal(store: &Store, login: &str) -> (Organization, User) {
    identity::create_personal_user(store, login).expect("personal user creates")
}

pub fn scope_of(store: &Store, user: &User) -> Scope {
    scope::resolve(store, &user.id, None).expect("scope resolves")
}

/// A user whose display name starts as the login.
pub fn new_user(login: &str, role: Role) -> NewUser {
    NewUser {
        login: login.to_owned(),
        name: login.to_owned(),
        role,
    }
}
