// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! How a stopped account comes back: the probe once the server answers
//! again, the loop that runs it and a replaced credential.

mod common;
mod reconnect;
mod signin;

use std::time::Duration;

use axum::http::{Method, StatusCode};
use huliho_imap_bridge::testing::{PASSWORD, imap};
use huliho_server::accounts::Credential;
use huliho_server::gate::MAX_REFUSED_RUN;
use reconnect::{ADDRESS, Instance, password};
use serde_json::Value;
use signin::{body_text, with_cookie};
use tower::ServiceExt;

/// The loop's interval in the test; the wait for a resume is a bounded
/// number of them.
const TICK: Duration = Duration::from_millis(50);
const PATIENCE: u32 = 40;

fn event(kind: &str, actor: &str) -> (String, String) {
    (kind.to_owned(), actor.to_owned())
}

/// Stops the account on refused connections through five retries.
async fn stop_on_connection(instance: &Instance, cookie: &str, id: &str) {
    instance.server_down();
    for _ in 0..MAX_REFUSED_RUN {
        instance.retry(cookie, id).await;
    }
    assert_eq!(instance.stopped_cause(id).as_deref(), Some("connection"));
}

async fn listed(instance: &Instance, cookie: &str) -> Value {
    let request = with_cookie(Method::GET, "/api/accounts", cookie);
    let response = instance.router.clone().oneshot(request).await.unwrap();
    serde_json::from_str(&body_text(response).await).unwrap()
}

#[tokio::test]
async fn the_probe_resumes_the_account_once_the_server_answers_again() {
    let instance = Instance::start(imap::Script::tls()).await;
    let id = instance.add_account(PASSWORD);
    let cookie = instance.sign_in().await;
    stop_on_connection(&instance, &cookie, &id).await;
    let reconnect = instance.reconnect();
    assert_eq!(reconnect.probe_once().await, 0);
    assert_eq!(instance.stopped_cause(&id).as_deref(), Some("connection"));
    instance.server_up();
    assert_eq!(reconnect.probe_once().await, 1);
    assert_eq!(instance.stopped_cause(&id), None);
    assert_eq!(
        instance.account_events().last(),
        Some(&event("account.resumed", "system"))
    );
    assert_eq!(reconnect.probe_once().await, 0);
}

#[tokio::test]
async fn the_probe_leaves_a_credentials_stop_to_the_user() {
    let instance = Instance::start(imap::Script::tls()).await;
    let id = instance.add_account("wrong horse");
    let cookie = instance.sign_in().await;
    instance.retry(&cookie, &id).await;
    assert_eq!(instance.stopped_cause(&id).as_deref(), Some("credentials"));
    let lines_before = instance.imap.lines().len();
    assert_eq!(instance.reconnect().probe_once().await, 0);
    assert_eq!(instance.imap.lines().len(), lines_before);
    assert_eq!(instance.stopped_cause(&id).as_deref(), Some("credentials"));
}

#[tokio::test]
async fn another_user_cannot_resume_the_account() {
    let instance = Instance::start(imap::Script::tls()).await;
    let id = instance.add_account(PASSWORD);
    let cookie = instance.sign_in().await;
    stop_on_connection(&instance, &cookie, &id).await;
    instance.server_up();
    let other = instance.sign_in_other().await;
    let (status, _) = instance.retry(&other, &id).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = instance
        .put_credentials(&other, &id, &password(PASSWORD))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(instance.stopped_cause(&id).as_deref(), Some("connection"));
}

#[tokio::test]
async fn a_passing_candidate_resumes_a_connection_stop_too() {
    let instance = Instance::start(imap::Script::tls()).await;
    let id = instance.add_account(PASSWORD);
    let cookie = instance.sign_in().await;
    stop_on_connection(&instance, &cookie, &id).await;
    instance.server_up();
    let (status, row) = instance
        .put_credentials(&cookie, &id, &password(PASSWORD))
        .await;
    assert_eq!(status, StatusCode::OK, "{row}");
    assert!(row["stoppedCause"].is_null());
    assert_eq!(instance.stopped_cause(&id), None);
    assert_eq!(
        instance.credential(&id),
        Credential::Password {
            password: PASSWORD.to_owned()
        }
    );
    let user = instance.user_id();
    let tail: Vec<(String, String)> = instance
        .account_events()
        .into_iter()
        .rev()
        .take(2)
        .collect();
    assert_eq!(
        tail,
        [
            event("account.resumed", user.as_str()),
            event("account.credentials_updated", user.as_str())
        ]
    );
}

#[tokio::test]
async fn the_loop_probes_at_its_interval() {
    let instance = Instance::start(imap::Script::tls()).await;
    let id = instance.add_account(PASSWORD);
    let cookie = instance.sign_in().await;
    stop_on_connection(&instance, &cookie, &id).await;
    let probe = tokio::spawn(instance.reconnect().probe_periodically(TICK));
    tokio::time::sleep(TICK * 2).await;
    assert_eq!(instance.stopped_cause(&id).as_deref(), Some("connection"));
    instance.server_up();
    let mut waited = 0;
    while instance.stopped_cause(&id).is_some() && waited < PATIENCE {
        tokio::time::sleep(TICK).await;
        waited += 1;
    }
    probe.abort();
    let rows = listed(&instance, &cookie).await;
    let row = rows["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == id)
        .unwrap();
    assert_eq!(row["address"], ADDRESS);
    assert!(row["stoppedCause"].is_null(), "{row}");
}
