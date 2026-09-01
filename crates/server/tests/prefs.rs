// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Per-user preferences and per-sender policies stay with their user.

use serde::{Deserialize, Serialize};

use huliho_server::identity::{self, User};
use huliho_server::prefs::{self, PolicyKey};
use huliho_server::scope::{self, Scope};
use huliho_server::store::Store;

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

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct ComposeSize {
    width: u32,
    height: u32,
}

#[test]
fn a_preference_round_trips_and_updates() {
    let store = store();
    let user = personal(&store, "mira@example.com");
    let scope = scope_of(&store, &user);
    prefs::set_preference(&store, &scope, "theme", &"dark").unwrap();
    let read = prefs::preference::<String>(&store, &scope, "theme").unwrap();
    assert_eq!(read.as_deref(), Some("dark"));
    prefs::set_preference(&store, &scope, "theme", &"light").unwrap();
    let read = prefs::preference::<String>(&store, &scope, "theme").unwrap();
    assert_eq!(read.as_deref(), Some("light"));
}

#[test]
fn a_missing_preference_is_none() {
    let store = store();
    let user = personal(&store, "mira@example.com");
    let scope = scope_of(&store, &user);
    let read = prefs::preference::<String>(&store, &scope, "density").unwrap();
    assert_eq!(read, None);
}

#[test]
fn a_structured_preference_round_trips() {
    let store = store();
    let user = personal(&store, "mira@example.com");
    let scope = scope_of(&store, &user);
    let size = ComposeSize {
        width: 800,
        height: 600,
    };
    prefs::set_preference(&store, &scope, "compose_size", &size).unwrap();
    let read = prefs::preference::<ComposeSize>(&store, &scope, "compose_size").unwrap();
    assert_eq!(read, Some(size));
}

#[test]
fn preferences_stay_with_their_user() {
    let store = store();
    let alpha = personal(&store, "alpha@example.com");
    let beta = personal(&store, "beta@example.com");
    prefs::set_preference(&store, &scope_of(&store, &alpha), "theme", &"dark").unwrap();
    let read = prefs::preference::<String>(&store, &scope_of(&store, &beta), "theme").unwrap();
    assert_eq!(read, None);
}

#[test]
fn a_sender_policy_round_trips_per_sender() {
    let store = store();
    let user = personal(&store, "mira@example.com");
    let scope = scope_of(&store, &user);
    let key = PolicyKey {
        sender: "news@example.com",
        name: "remote_content",
    };
    prefs::set_sender_policy(&store, &scope, key, &true).unwrap();
    let read = prefs::sender_policy::<bool>(&store, &scope, key).unwrap();
    assert_eq!(read, Some(true));
    let other_sender = PolicyKey {
        sender: "shop@example.com",
        name: "remote_content",
    };
    let read = prefs::sender_policy::<bool>(&store, &scope, other_sender).unwrap();
    assert_eq!(read, None);
    let other_name = PolicyKey {
        sender: "news@example.com",
        name: "route",
    };
    let read = prefs::sender_policy::<bool>(&store, &scope, other_name).unwrap();
    assert_eq!(read, None);
}

#[test]
fn sender_policies_stay_with_their_user() {
    let store = store();
    let alpha = personal(&store, "alpha@example.com");
    let beta = personal(&store, "beta@example.com");
    let key = PolicyKey {
        sender: "news@example.com",
        name: "remote_content",
    };
    prefs::set_sender_policy(&store, &scope_of(&store, &alpha), key, &true).unwrap();
    let read = prefs::sender_policy::<bool>(&store, &scope_of(&store, &beta), key).unwrap();
    assert_eq!(read, None);
}
