// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The gate's rules against ready outcomes: what stops an account, what
//! resumes it and what leaves it alone.

use std::future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use huliho_server::accounts::{self, AccountSettings, Credential, NewAccount, Provider, StopCause};
use huliho_server::events::{self, Actor};
use huliho_server::gate::{AttemptError, Gate, MAX_REFUSED_RUN};
use huliho_server::identity;
use huliho_server::ids::UserId;
use huliho_server::probe::ProbeError;
use huliho_server::scope::{self, Scope};
use huliho_server::secrets::{InstanceSecret, Keys};
use huliho_server::store::{Store, StoreError};
use tokio::sync::oneshot;

const LOGIN: &str = "mira@example.com";

/// Long enough for a spawned attempt to run, had the lock let it.
const SETTLE: Duration = Duration::from_millis(100);

fn keys() -> Keys {
    Keys::derive(&InstanceSecret::from_bytes(b"0123456789abcdef0123456789abcdef".to_vec()).unwrap())
}

/// An owner with one account: the gate over the store, the user's id
/// and the account scope.
fn rig(store: Arc<Store>) -> (Gate, UserId, Scope) {
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

fn in_memory() -> (Gate, UserId, Scope) {
    rig(Arc::new(Store::in_memory().unwrap()))
}

/// One attempt that ends in `error`, signed by `actor`.
async fn fail(
    gate: &Gate,
    scope: &Scope,
    actor: &Actor,
    error: ProbeError,
) -> Result<(), AttemptError> {
    gate.attempt(scope, actor, future::ready(Err(AttemptError::from(error))))
        .await
}

async fn pass(gate: &Gate, scope: &Scope, actor: &Actor) -> Result<(), AttemptError> {
    gate.attempt(scope, actor, future::ready(Ok(()))).await
}

fn unreachable() -> ProbeError {
    ProbeError::Unreachable("closed".to_owned())
}

fn stopped_cause(gate: &Gate, scope: &Scope) -> Option<StopCause> {
    accounts::get(gate.store(), scope).unwrap().stopped_cause
}

/// The account events as `(type, actor)`, oldest first.
fn account_events(gate: &Gate, scope: &Scope) -> Vec<(String, String)> {
    let plain = scope::resolve(gate.store(), scope.user_id(), None).unwrap();
    events::for_organization(gate.store(), &plain)
        .unwrap()
        .into_iter()
        .filter(|record| record.event_type.starts_with("account."))
        .map(|record| (record.event_type, record.actor))
        .collect()
}

fn event(kind: &str, actor: &str) -> (String, String) {
    (kind.to_owned(), actor.to_owned())
}

#[tokio::test]
async fn a_rejected_credential_stops_at_once_signed_by_the_actor() {
    let (gate, user_id, scope) = in_memory();
    let user = Actor::User(user_id.clone());
    let error = fail(&gate, &scope, &user, ProbeError::CredentialRejected)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        AttemptError::Upstream(ProbeError::CredentialRejected)
    ));
    assert_eq!(stopped_cause(&gate, &scope), Some(StopCause::Credentials));
    assert_eq!(
        account_events(&gate, &scope).last(),
        Some(&event("account.stopped", user_id.as_str()))
    );
}

#[tokio::test]
async fn five_connection_failures_stop_and_four_do_not() {
    let (gate, user_id, scope) = in_memory();
    for _ in 1..MAX_REFUSED_RUN {
        fail(&gate, &scope, &Actor::System, unreachable())
            .await
            .unwrap_err();
        assert_eq!(stopped_cause(&gate, &scope), None);
    }
    let refused = ProbeError::Insecure("refused".to_owned());
    fail(&gate, &scope, &Actor::User(user_id), refused)
        .await
        .unwrap_err();
    assert_eq!(stopped_cause(&gate, &scope), Some(StopCause::Connection));
    // The run stop is the system's, whoever ran the fifth attempt.
    assert_eq!(
        account_events(&gate, &scope).last(),
        Some(&event("account.stopped", "system"))
    );
}

#[tokio::test]
async fn a_credentials_stop_is_not_flipped_by_a_run_of_connection_failures() {
    let (gate, user_id, scope) = in_memory();
    fail(
        &gate,
        &scope,
        &Actor::User(user_id),
        ProbeError::CredentialRejected,
    )
    .await
    .unwrap_err();
    for _ in 0..MAX_REFUSED_RUN {
        fail(&gate, &scope, &Actor::System, unreachable())
            .await
            .unwrap_err();
    }
    assert_eq!(stopped_cause(&gate, &scope), Some(StopCause::Credentials));
    assert_eq!(
        account_events(&gate, &scope)
            .iter()
            .filter(|(kind, _)| kind == "account.stopped")
            .count(),
        1
    );
}

#[tokio::test]
async fn a_pass_resets_the_run_and_resumes_a_stopped_account() {
    let (gate, user_id, scope) = in_memory();
    for _ in 1..MAX_REFUSED_RUN {
        fail(&gate, &scope, &Actor::System, unreachable())
            .await
            .unwrap_err();
    }
    pass(&gate, &scope, &Actor::System).await.unwrap();
    for _ in 1..MAX_REFUSED_RUN {
        fail(&gate, &scope, &Actor::System, unreachable())
            .await
            .unwrap_err();
    }
    assert_eq!(stopped_cause(&gate, &scope), None);
    fail(&gate, &scope, &Actor::System, unreachable())
        .await
        .unwrap_err();
    assert_eq!(stopped_cause(&gate, &scope), Some(StopCause::Connection));
    pass(&gate, &scope, &Actor::User(user_id.clone()))
        .await
        .unwrap();
    assert_eq!(stopped_cause(&gate, &scope), None);
    let events = account_events(&gate, &scope);
    assert_eq!(
        events.last(),
        Some(&event("account.resumed", user_id.as_str()))
    );
    assert_eq!(
        events
            .iter()
            .filter(|(kind, _)| kind == "account.resumed")
            .count(),
        1
    );
}

#[tokio::test]
async fn an_undecided_outcome_leaves_the_row_and_the_run_alone() {
    let (gate, _, scope) = in_memory();
    for _ in 1..MAX_REFUSED_RUN {
        fail(&gate, &scope, &Actor::System, unreachable())
            .await
            .unwrap_err();
    }
    for error in [
        ProbeError::Unsupported("odd".to_owned()),
        ProbeError::SmtpAuthUnavailable,
    ] {
        fail(&gate, &scope, &Actor::System, error)
            .await
            .unwrap_err();
        assert_eq!(stopped_cause(&gate, &scope), None);
    }
    let failed = future::ready(Err(AttemptError::Store(StoreError::Tampered)));
    let result: Result<(), AttemptError> = gate.attempt(&scope, &Actor::System, failed).await;
    assert!(matches!(
        result,
        Err(AttemptError::Store(StoreError::Tampered))
    ));
    assert_eq!(stopped_cause(&gate, &scope), None);
    fail(&gate, &scope, &Actor::System, unreachable())
        .await
        .unwrap_err();
    assert_eq!(stopped_cause(&gate, &scope), Some(StopCause::Connection));
    assert_eq!(
        account_events(&gate, &scope)
            .iter()
            .filter(|(kind, _)| kind == "account.stopped")
            .count(),
        1
    );
}

#[tokio::test]
async fn a_stop_survives_a_store_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let (gate, user_id, scope) = rig(Arc::new(Store::open(dir.path()).unwrap()));
    fail(
        &gate,
        &scope,
        &Actor::User(user_id.clone()),
        ProbeError::CredentialRejected,
    )
    .await
    .unwrap_err();
    drop(gate);
    let reopened = Store::open(dir.path()).unwrap();
    let again = scope::resolve(&reopened, &user_id, scope.account_id()).unwrap();
    let row = accounts::get(&reopened, &again).unwrap();
    assert_eq!(row.stopped_cause, Some(StopCause::Credentials));
    assert!(row.stopped_at.is_some());
}

#[tokio::test]
async fn attempts_on_one_account_run_one_at_a_time() {
    let (gate, _, scope) = in_memory();
    let (release, released) = oneshot::channel::<()>();
    let (entered, inside) = oneshot::channel::<()>();
    let second_started = Arc::new(AtomicBool::new(false));
    let first = {
        let (gate, scope) = (gate.clone(), scope.clone());
        tokio::spawn(async move {
            gate.attempt(&scope, &Actor::System, async {
                entered.send(()).ok();
                released.await.ok();
                Ok::<(), AttemptError>(())
            })
            .await
        })
    };
    // The first attempt holds the lock from here on.
    inside.await.unwrap();
    let second = {
        let (gate, scope) = (gate.clone(), scope.clone());
        let started = Arc::clone(&second_started);
        tokio::spawn(async move {
            gate.attempt(&scope, &Actor::System, async move {
                started.store(true, Ordering::SeqCst);
                Ok::<(), AttemptError>(())
            })
            .await
        })
    };
    tokio::time::sleep(SETTLE).await;
    assert!(!second_started.load(Ordering::SeqCst));
    release.send(()).unwrap();
    first.await.unwrap().unwrap();
    second.await.unwrap().unwrap();
    assert!(second_started.load(Ordering::SeqCst));
}

#[tokio::test]
async fn forgetting_an_account_starts_a_fresh_run() {
    let (gate, _, scope) = in_memory();
    for _ in 1..MAX_REFUSED_RUN {
        fail(&gate, &scope, &Actor::System, unreachable())
            .await
            .unwrap_err();
    }
    gate.forget(scope.account_id().unwrap());
    for _ in 1..MAX_REFUSED_RUN {
        fail(&gate, &scope, &Actor::System, unreachable())
            .await
            .unwrap_err();
    }
    assert_eq!(stopped_cause(&gate, &scope), None);
}

#[tokio::test]
async fn an_attempt_needs_an_account_scope() {
    let (gate, user_id, _) = in_memory();
    let plain = scope::resolve(gate.store(), &user_id, None).unwrap();
    let result = pass(&gate, &plain, &Actor::System).await;
    assert!(matches!(
        result,
        Err(AttemptError::Store(StoreError::MissingAccount))
    ));
}

#[tokio::test]
async fn a_pass_outside_an_attempt_resumes_and_answers_the_row() {
    let (gate, user_id, scope) = in_memory();
    fail(
        &gate,
        &scope,
        &Actor::System,
        ProbeError::CredentialRejected,
    )
    .await
    .unwrap_err();
    let row = gate.passed(&scope, &Actor::User(user_id.clone())).unwrap();
    assert_eq!(row.stopped_cause, None);
    assert_eq!(row.stopped_at, None);
    assert_eq!(
        account_events(&gate, &scope).last(),
        Some(&event("account.resumed", user_id.as_str()))
    );
}
