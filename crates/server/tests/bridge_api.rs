// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The bridge behind the JMAP routes: who reaches an IMAP account, what
//! its session object looks like, how a request runs against the cache
//! the sync fills, what a request the bridge does not run answers, how
//! the connector feeds the gate and what removing the account leaves.

mod answers;
mod bridge_rig;
mod common;
mod fake_dns;
mod log_capture;
mod readers;
mod signin;

use std::time::Duration;

use answers::answer;
use axum::http::{Method, StatusCode};
use bridge_rig::{ADDRESS, Instance, mailboxes, using};
use huliho_imap_bridge::jmap::{CORE_CAPABILITY, HULIHO_CAPABILITY, MAIL_CAPABILITY};
use huliho_imap_bridge::runtime::{ConnectError, Connector};
use huliho_imap_bridge::store::AccountKey;
use huliho_imap_bridge::testing::imap::HOST;
use huliho_imap_bridge::testing::{CLOSED, Extension, Folder, Mailboxes, Message, PASSWORD};
use huliho_server::accounts::{self, StopCause};
use huliho_server::bridge::{self, HostConnector};
use huliho_server::events::Actor;
use huliho_server::gate::{MAX_REFUSED_RUN, RUN_WINDOW, Reconnect};
use huliho_server::ids::AccountId;
use huliho_server::jmap::MAX_CONCURRENT_REQUESTS;
use log_capture::Capture;
use serde_json::{Value, json};
use signin::with_cookie;
use tokio::time::sleep;

/// The messages in the inbox of the model.
const INBOX_MAIL: u32 = 3;

/// How often and how long a test looks for the account to stop or for
/// the runtime to sit still.
const LOOK: Duration = Duration::from_millis(25);
const LOOKS: usize = 40;

/// The model: an inbox with mail and the English folders, every
/// extension on.
fn model() -> Mailboxes {
    Mailboxes::new(
        vec![
            Folder::new("INBOX").with_mail((1..=INBOX_MAIL).map(Message::new).collect()),
            Folder::special("Drafts", "\\Drafts"),
            Folder::special("Sent", "\\Sent"),
            Folder::special("Junk", "\\Junk"),
            Folder::special("Trash", "\\Trash"),
            Folder::special("Archive", "\\Archive"),
        ],
        Extension::all(),
    )
}

async fn instance() -> Instance {
    Instance::start(model(), RUN_WINDOW).await
}

fn keys(value: &Value) -> Vec<&str> {
    value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect()
}

/// Waits for the account to stop; its cause once it has.
async fn stopped(instance: &Instance, id: &str) -> Option<String> {
    for _ in 0..LOOKS {
        if let Some(cause) = instance.stopped_cause(id) {
            return Some(cause);
        }
        sleep(LOOK).await;
    }
    None
}

#[tokio::test]
async fn another_users_imap_account_is_not_found_on_both_routes_and_nothing_connects() {
    let instance = instance().await;
    let id = instance.add_account(PASSWORD);
    let other = instance.sign_in_other().await;
    let (status, body) = instance.session(&other, &id).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (status, body) = instance
        .request(&other, &id, &using(&[mailboxes(&id)]))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert!(instance.fake.lines().is_empty());
}

#[tokio::test]
async fn a_stopped_imap_account_answers_409_with_its_cause_before_anything_connects() {
    let instance = instance().await;
    let id = instance.add_account(PASSWORD);
    let cookie = instance.sign_in().await;
    accounts::stop(
        &instance.store,
        &instance.scope(Some(&id)),
        StopCause::Connection,
        &Actor::System,
    )
    .unwrap();
    for (status, body) in [
        instance.session(&cookie, &id).await,
        instance
            .request(&cookie, &id, &using(&[mailboxes(&id)]))
            .await,
    ] {
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert_eq!(body["error"], "still_stopped");
        assert_eq!(body["cause"], "connection");
    }
    assert!(instance.fake.lines().is_empty());
}

#[tokio::test]
async fn the_session_object_of_a_bridge_account_carries_the_vendor_capability_and_its_own_state() {
    let instance = instance().await;
    let id = instance.add_account(PASSWORD);
    let cookie = instance.sign_in().await;
    let (status, session) = instance.session(&cookie, &id).await;
    assert_eq!(status, StatusCode::OK, "{session}");
    // The URLs are the ones the proxy rewrites a native account's to.
    let urls = bridge::urls(&AccountId::from(id.clone()));
    assert_eq!(session["apiUrl"], urls.api);
    assert_eq!(session["downloadUrl"], urls.download);
    assert_eq!(session["uploadUrl"], urls.upload);
    assert_eq!(session["eventSourceUrl"], urls.event_source);
    assert_eq!(
        keys(&session["capabilities"]),
        [HULIHO_CAPABILITY, CORE_CAPABILITY, MAIL_CAPABILITY]
    );
    assert_eq!(
        keys(&session["accounts"][&id]["accountCapabilities"]),
        [HULIHO_CAPABILITY, MAIL_CAPABILITY]
    );
    assert_eq!(session["primaryAccounts"][MAIL_CAPABILITY], id);
    assert_eq!(session["accounts"][&id]["isReadOnly"], true);
    assert_eq!(session["username"], ADDRESS);
    let core = &session["capabilities"][CORE_CAPABILITY];
    assert_eq!(core["maxConcurrentRequests"], MAX_CONCURRENT_REQUESTS);
    let text = session.to_string();
    assert!(!text.contains(HOST), "{text}");
    // The state is the host's and every response carries the same value.
    let state = session["state"].as_str().unwrap().to_owned();
    assert!(!state.is_empty());
    let (status, answer) = instance
        .request(&cookie, &id, &using(&[mailboxes(&id)]))
        .await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer["sessionState"], state);
    assert!(
        instance.wait_synced(&cookie, &id).await,
        "{:?}",
        instance.fake.lines()
    );
    let (_, later) = instance
        .request(&cookie, &id, &using(&[mailboxes(&id)]))
        .await;
    assert_eq!(later["sessionState"], state);
}

#[tokio::test]
async fn a_request_answers_from_the_cache_the_sync_fills_on_one_connection() {
    let capture = Capture::install();
    let instance = instance().await;
    let id = instance.add_account(PASSWORD);
    let cookie = instance.sign_in().await;
    assert!(
        instance.wait_synced(&cookie, &id).await,
        "{:?}",
        instance.fake.lines()
    );
    let (status, listed) = instance.call(&cookie, &id, mailboxes(&id)).await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    let roles: Vec<&str> = listed["list"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|mailbox| mailbox["role"].as_str())
        .collect();
    assert_eq!(
        roles,
        ["inbox", "drafts", "sent", "archive", "junk", "trash"]
    );
    let inbox = listed["list"][0]["id"].clone();
    let query = json!([
        "Email/query",
        { "accountId": id, "filter": { "inMailbox": inbox }, "calculateTotal": true },
        "c1"
    ]);
    let (status, window) = instance.call(&cookie, &id, query).await;
    assert_eq!(status, StatusCode::OK, "{window}");
    assert_eq!(window["total"], INBOX_MAIL);
    let ids = window["ids"].clone();
    let get = json!([
        "Email/get",
        { "accountId": id, "ids": ids, "properties": ["subject", "preview"] },
        "c1"
    ]);
    let (_, emails) = instance.call(&cookie, &id, get).await;
    let subjects: Vec<&str> = emails["list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|email| email["subject"].as_str().unwrap())
        .collect();
    assert_eq!(subjects, ["Message 3", "Message 2", "Message 1"]);
    assert_eq!(emails["list"][0]["preview"], "Body of message 3.");
    // The sync, the refreshes and the preview fetch shared one conversation.
    assert_eq!(instance.logins(), 1, "{:?}", instance.fake.lines());
    let text = capture.text();
    for secret in [PASSWORD, ADDRESS, HOST] {
        assert!(!text.contains(secret), "{secret} in {text}");
    }
}

#[tokio::test]
async fn a_request_the_bridge_does_not_run_answers_problem_details_rfc8620_3_6_1() {
    let instance = instance().await;
    let id = instance.add_account(PASSWORD);
    let cookie = instance.sign_in().await;
    let unknown = json!({
        "using": [CORE_CAPABILITY, "urn:ietf:params:jmap:websocket"],
        "methodCalls": [mailboxes(&id)],
    });
    let (status, problem) = instance.request(&cookie, &id, &unknown).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{problem}");
    assert_eq!(
        problem["type"],
        "urn:ietf:params:jmap:error:unknownCapability"
    );
    assert_eq!(problem["status"], 400);
    let shapeless = json!({ "using": [CORE_CAPABILITY], "methodCalls": "none" });
    let (status, problem) = instance.request(&cookie, &id, &shapeless).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{problem}");
    assert_eq!(problem["type"], "urn:ietf:params:jmap:error:notRequest");
    let calls: Vec<Value> = (0..17)
        .map(|index| json!(["Core/echo", {}, format!("c{index}")]))
        .collect();
    let (status, problem) = instance.request(&cookie, &id, &using(&calls)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{problem}");
    assert_eq!(problem["type"], "urn:ietf:params:jmap:error:limit");
    assert_eq!(problem["limit"], "maxCallsInRequest");
}

#[tokio::test]
async fn a_refused_credential_stops_the_account_after_one_sign_in() {
    let instance = instance().await;
    let id = instance.add_account("wrong horse");
    let cookie = instance.sign_in().await;
    let (status, answer) = instance.call(&cookie, &id, mailboxes(&id)).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer["list"], json!([]));
    assert_eq!(
        stopped(&instance, &id).await.as_deref(),
        Some("credentials")
    );
    assert_eq!(
        instance.account_events().last(),
        Some(&("account.stopped".to_owned(), "system".to_owned()))
    );
    assert_eq!(instance.logins(), 1, "{:?}", instance.fake.lines());
    let (status, body) = instance
        .request(&cookie, &id, &using(&[mailboxes(&id)]))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["cause"], "credentials");
    sleep(LOOK * 8).await;
    assert_eq!(instance.logins(), 1);
}

#[tokio::test]
async fn refused_connections_in_five_windows_stop_the_account_and_a_stopped_one_is_held_back() {
    let instance = Instance::start(model(), Duration::ZERO).await;
    let id = instance.add_account_at(CLOSED.port(), PASSWORD);
    let connector = HostConnector::new(Reconnect::from(&instance.api));
    let key = AccountKey::new(&id);
    for attempt in 1..=MAX_REFUSED_RUN {
        let outcome = connector.connect(&key).await;
        assert!(
            matches!(outcome, Err(ConnectError::Failed(_))),
            "attempt {attempt}"
        );
        if attempt < MAX_REFUSED_RUN {
            assert_eq!(instance.stopped_cause(&id), None, "attempt {attempt}");
        }
    }
    assert_eq!(instance.stopped_cause(&id).as_deref(), Some("connection"));
    assert!(matches!(
        connector.connect(&key).await,
        Err(ConnectError::Held)
    ));
    let running = instance.add_account(PASSWORD);
    assert!(connector.connect(&AccountKey::new(&running)).await.is_ok());
    assert_eq!(instance.stopped_cause(&running), None);
    assert!(matches!(
        connector.connect(&AccountKey::new("gone")).await,
        Err(ConnectError::Held)
    ));
}

#[tokio::test]
async fn removing_the_account_stops_its_runtime_and_removes_the_bridge_rows() {
    let instance = Instance::on_disk(model()).await;
    let id = instance.add_account(PASSWORD);
    let cookie = instance.sign_in().await;
    assert!(
        instance.wait_synced(&cookie, &id).await,
        "{:?}",
        instance.fake.lines()
    );
    assert!(instance.bridge_rows(&id) > 0);
    let logins = instance.logins();
    let request = with_cookie(Method::DELETE, &format!("/api/accounts/{id}"), &cookie);
    let (status, _) = answer(&instance.router, request).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(instance.bridge_rows(&id), 0);
    let (status, _) = instance.session(&cookie, &id).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    sleep(LOOK * 8).await;
    assert_eq!(instance.logins(), logins, "the runtime connected again");
}
