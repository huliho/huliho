// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Fixtures for the gate tests: an owner with one account and ready
//! outcomes to hand the gate.

use std::future;
use std::sync::Arc;

use huliho_server::accounts::{self, AccountSettings, Credential, NewAccount, Provider, StopCause};
use huliho_server::events::{self, Actor};
use huliho_server::gate::{AttemptError, Gate};
use huliho_server::identity;
use huliho_server::ids::UserId;
use huliho_server::probe::ProbeError;
use huliho_server::scope::{self, Scope};
use huliho_server::secrets::{InstanceSecret, Keys};
use huliho_server::store::Store;

pub const LOGIN: &str = "mira@example.com";

fn keys() -> Keys {
    Keys::derive(&InstanceSecret::from_bytes(b"0123456789abcdef0123456789abcdef".to_vec()).unwrap())
}

/// An owner with one account: the gate over the store, the user's id
/// and the account scope.
pub fn rig(store: Arc<Store>) -> (Gate, UserId, Scope) {
    let (_, user) = identity::create_personal_user(&store, LOGIN).unwrap();
    let scope = scope::resolve(&store, &user.id, None).unwrap();
    let new = NewAccount {
        address: LOGIN.to_owned(),
        name: "Fastmail".to_owned(),
        provider: Provider::Fastmail,
        settings: AccountSettings::Jmap {
            session_url: "https://api.fastmail.com/jmap/session".parse().unwrap(),
        },
        credential: Credential::Bearer {
            token: "fmu1-token".to_owned(),
        },
    };
    let account = accounts::add(&store, &keys(), &scope, &new).unwrap();
    let scoped = scope::resolve(&store, &user.id, Some(&account.id)).unwrap();
    (Gate::new(store), user.id, scoped)
}

pub fn in_memory() -> (Gate, UserId, Scope) {
    rig(Arc::new(Store::in_memory().unwrap()))
}

/// One attempt that ends in `error`, signed by `actor`.
pub async fn fail(
    gate: &Gate,
    scope: &Scope,
    actor: &Actor,
    error: ProbeError,
) -> Result<(), AttemptError> {
    gate.attempt(scope, actor, future::ready(Err(AttemptError::from(error))))
        .await
}

pub async fn pass(gate: &Gate, scope: &Scope, actor: &Actor) -> Result<(), AttemptError> {
    gate.attempt(scope, actor, future::ready(Ok(()))).await
}

pub fn stopped_cause(gate: &Gate, scope: &Scope) -> Option<StopCause> {
    accounts::get(gate.store(), scope).unwrap().stopped_cause
}

/// The account events as `(type, actor)`, oldest first.
pub fn account_events(gate: &Gate, scope: &Scope) -> Vec<(String, String)> {
    let plain = scope::resolve(gate.store(), scope.user_id(), None).unwrap();
    events::for_organization(gate.store(), &plain)
        .unwrap()
        .into_iter()
        .filter(|record| record.event_type.starts_with("account."))
        .map(|record| (record.event_type, record.actor))
        .collect()
}

pub fn event(kind: &str, actor: &str) -> (String, String) {
    (kind.to_owned(), actor.to_owned())
}
