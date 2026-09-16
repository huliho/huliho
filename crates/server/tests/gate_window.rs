// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The run of connection failures under the paused clock: what a window
//! absorbs, what five windows do and what an observed outcome touches.

mod gate_rig;

use std::sync::Arc;
use std::time::Duration;

use gate_rig::{account_events, event, fail, in_memory, pass, stopped_cause};
use huliho_server::accounts::{self, StopCause};
use huliho_server::events::Actor;
use huliho_server::gate::{AttemptError, Fault, Gate, MAX_REFUSED_RUN, RUN_WINDOW};
use huliho_server::probe::ProbeError;
use huliho_server::scope::Scope;
use huliho_server::store::{Store, StoreError};
use tokio::time::advance;

/// Failures in one window, more than the run needs, so a burst that
/// counted per failure would stop the account by itself.
const BURST: u32 = MAX_REFUSED_RUN + 2;

fn unreachable() -> ProbeError {
    ProbeError::Unreachable("closed".to_owned())
}

/// A failure in a window of its own; the clock moves past the window
/// first.
async fn fail_in_a_new_window(gate: &Gate, scope: &Scope) {
    advance(RUN_WINDOW).await;
    fail(gate, scope, &Actor::System, unreachable())
        .await
        .unwrap_err();
}

#[tokio::test(start_paused = true)]
async fn five_windows_of_failures_stop_and_four_do_not() {
    let (gate, user_id, scope) = in_memory();
    fail(&gate, &scope, &Actor::System, unreachable())
        .await
        .unwrap_err();
    for _ in 2..MAX_REFUSED_RUN {
        fail_in_a_new_window(&gate, &scope).await;
        assert_eq!(stopped_cause(&gate, &scope), None);
    }
    advance(RUN_WINDOW).await;
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

#[tokio::test(start_paused = true)]
async fn failures_inside_one_window_count_once() {
    let (gate, _, scope) = in_memory();
    for _ in 0..BURST {
        fail(&gate, &scope, &Actor::System, unreachable())
            .await
            .unwrap_err();
    }
    assert_eq!(stopped_cause(&gate, &scope), None);
    // The burst was one window; the stop needs four more.
    for _ in 2..MAX_REFUSED_RUN {
        fail_in_a_new_window(&gate, &scope).await;
        assert_eq!(stopped_cause(&gate, &scope), None);
    }
    fail_in_a_new_window(&gate, &scope).await;
    assert_eq!(stopped_cause(&gate, &scope), Some(StopCause::Connection));
}

#[tokio::test(start_paused = true)]
async fn a_failure_just_inside_the_window_edge_is_absorbed() {
    let (gate, _, scope) = in_memory();
    fail(&gate, &scope, &Actor::System, unreachable())
        .await
        .unwrap_err();
    advance(RUN_WINDOW.checked_sub(Duration::from_millis(1)).unwrap()).await;
    fail(&gate, &scope, &Actor::System, unreachable())
        .await
        .unwrap_err();
    for _ in 2..MAX_REFUSED_RUN {
        fail_in_a_new_window(&gate, &scope).await;
    }
    // The second failure fell inside the first window and counted for
    // nothing: three more windows leave the run at four, one short.
    assert_eq!(stopped_cause(&gate, &scope), None);
    fail_in_a_new_window(&gate, &scope).await;
    assert_eq!(stopped_cause(&gate, &scope), Some(StopCause::Connection));
}

#[tokio::test(start_paused = true)]
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
    fail(&gate, &scope, &Actor::System, unreachable())
        .await
        .unwrap_err();
    for _ in 2..=MAX_REFUSED_RUN {
        fail_in_a_new_window(&gate, &scope).await;
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

#[tokio::test(start_paused = true)]
async fn a_pass_resets_the_run_and_resumes_a_stopped_account() {
    let (gate, user_id, scope) = in_memory();
    fail(&gate, &scope, &Actor::System, unreachable())
        .await
        .unwrap_err();
    for _ in 2..MAX_REFUSED_RUN {
        fail_in_a_new_window(&gate, &scope).await;
    }
    pass(&gate, &scope, &Actor::System).await.unwrap();
    fail_in_a_new_window(&gate, &scope).await;
    for _ in 2..MAX_REFUSED_RUN {
        fail_in_a_new_window(&gate, &scope).await;
    }
    assert_eq!(stopped_cause(&gate, &scope), None);
    fail_in_a_new_window(&gate, &scope).await;
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

#[tokio::test(start_paused = true)]
async fn an_undecided_outcome_leaves_the_row_and_the_run_alone() {
    let (gate, _, scope) = in_memory();
    fail(&gate, &scope, &Actor::System, unreachable())
        .await
        .unwrap_err();
    for _ in 2..MAX_REFUSED_RUN {
        fail_in_a_new_window(&gate, &scope).await;
    }
    advance(RUN_WINDOW).await;
    for error in [
        ProbeError::Unsupported("odd".to_owned()),
        ProbeError::SmtpAuthUnavailable,
    ] {
        fail(&gate, &scope, &Actor::System, error)
            .await
            .unwrap_err();
        assert_eq!(stopped_cause(&gate, &scope), None);
    }
    let failed = std::future::ready(Err(AttemptError::Store(StoreError::Tampered)));
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

#[tokio::test(start_paused = true)]
async fn forgetting_an_account_starts_a_fresh_run() {
    let (gate, _, scope) = in_memory();
    fail(&gate, &scope, &Actor::System, unreachable())
        .await
        .unwrap_err();
    for _ in 2..MAX_REFUSED_RUN {
        fail_in_a_new_window(&gate, &scope).await;
    }
    gate.forget(scope.account_id().unwrap());
    fail_in_a_new_window(&gate, &scope).await;
    for _ in 2..MAX_REFUSED_RUN {
        fail_in_a_new_window(&gate, &scope).await;
    }
    assert_eq!(stopped_cause(&gate, &scope), None);
}

#[tokio::test(start_paused = true)]
async fn an_observed_pass_touches_a_stopped_row_only_while_a_run_stands() {
    let (gate, user_id, scope) = in_memory();
    let user = Actor::User(user_id);
    accounts::stop(gate.store(), &scope, StopCause::Connection, &Actor::System).unwrap();
    // No run in memory: the pass costs nothing and the stop stays.
    gate.observe(&scope, &user, None).await.unwrap();
    assert_eq!(stopped_cause(&gate, &scope), Some(StopCause::Connection));
    assert_eq!(
        account_events(&gate, &scope).last(),
        Some(&event("account.stopped", "system"))
    );
    gate.observe(&scope, &user, Some(Fault::Connection))
        .await
        .unwrap();
    gate.observe(&scope, &user, None).await.unwrap();
    assert_eq!(stopped_cause(&gate, &scope), None);
    assert_eq!(
        account_events(&gate, &scope).last(),
        Some(&event("account.resumed", scope.user_id().as_str()))
    );
}

#[tokio::test(start_paused = true)]
async fn observed_faults_follow_the_same_rules_as_attempts() {
    let (gate, user_id, scope) = in_memory();
    let user = Actor::User(user_id.clone());
    for _ in 0..BURST {
        gate.observe(&scope, &user, Some(Fault::Connection))
            .await
            .unwrap();
    }
    assert_eq!(stopped_cause(&gate, &scope), None);
    for _ in 2..=MAX_REFUSED_RUN {
        advance(RUN_WINDOW).await;
        gate.observe(&scope, &user, Some(Fault::Connection))
            .await
            .unwrap();
    }
    assert_eq!(stopped_cause(&gate, &scope), Some(StopCause::Connection));
    gate.observe(&scope, &user, Some(Fault::Undecided))
        .await
        .unwrap();
    assert_eq!(stopped_cause(&gate, &scope), Some(StopCause::Connection));
    // A credentials stop outranks a connection stop, signed by the actor.
    gate.observe(&scope, &user, Some(Fault::Credential))
        .await
        .unwrap();
    assert_eq!(stopped_cause(&gate, &scope), Some(StopCause::Credentials));
    assert_eq!(
        account_events(&gate, &scope).last(),
        Some(&event("account.stopped", user_id.as_str()))
    );
    // A full run of connection failures does not take the stop back.
    for _ in 1..=MAX_REFUSED_RUN {
        advance(RUN_WINDOW).await;
        gate.observe(&scope, &user, Some(Fault::Connection))
            .await
            .unwrap();
    }
    assert_eq!(stopped_cause(&gate, &scope), Some(StopCause::Credentials));
    assert_eq!(
        account_events(&gate, &scope)
            .iter()
            .filter(|(kind, _)| kind == "account.stopped")
            .count(),
        2
    );
}

#[tokio::test]
async fn a_gate_on_a_zero_window_counts_every_failure() {
    let store = Arc::new(Store::in_memory().unwrap());
    let (_, _, scope) = gate_rig::rig(Arc::clone(&store));
    let gate = Gate::with_window(store, Duration::ZERO);
    for _ in 1..MAX_REFUSED_RUN {
        fail(&gate, &scope, &Actor::System, unreachable())
            .await
            .unwrap_err();
        assert_eq!(stopped_cause(&gate, &scope), None);
    }
    fail(&gate, &scope, &Actor::System, unreachable())
        .await
        .unwrap_err();
    assert_eq!(stopped_cause(&gate, &scope), Some(StopCause::Connection));
}
