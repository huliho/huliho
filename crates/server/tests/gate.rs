// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The gate's rules against ready outcomes: what stops an account, what
//! resumes it and what leaves it alone.

mod gate_rig;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use gate_rig::{account_events, event, fail, in_memory, pass, rig, stopped_cause};
use huliho_server::accounts::{self, StopCause};
use huliho_server::events::Actor;
use huliho_server::gate::AttemptError;
use huliho_server::probe::ProbeError;
use huliho_server::scope;
use huliho_server::store::{Store, StoreError};
use tokio::sync::oneshot;

/// Long enough for a spawned attempt to run, had the lock let it.
const SETTLE: Duration = Duration::from_millis(100);

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
