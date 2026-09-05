// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The account list and removal over HTTP.

mod common;
mod connecting;
mod signin;

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use serde::Deserialize;
use tower::ServiceExt;

use common::{api_state, router_on};
use connecting::{ACCOUNT_TOKEN, keys, new_account};
use huliho_server::accounts::{self, Credential};
use huliho_server::auth::{self, LoginOutcome};
use huliho_server::config::UpstreamConfig;
use huliho_server::ids::{AccountId, UserId};
use huliho_server::store::Store;
use huliho_server::{identity, scope};
use signin::{
    LOGIN, PASSWORD, body_text, cookie_of, login_request, sign_in, store_with_account, with_cookie,
};

const OTHER_LOGIN: &str = "noor@example.com";
const ADDRESS: &str = "mira@fastmail.com";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Listed {
    accounts: Vec<ListedAccount>,
    probe_interval_minutes: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListedAccount {
    id: String,
    address: String,
    name: String,
    provider: String,
    kind: String,
    auth_method: String,
    stopped_cause: Option<String>,
    stopped_at: Option<i64>,
    created_at: i64,
}

fn user_id(store: &Store, login: &str) -> UserId {
    match auth::verify_login(store, login, PASSWORD).unwrap() {
        LoginOutcome::Verified(id) => id,
        _ => panic!("the fixture user signs in"),
    }
}

/// Adds a Fastmail account for `login` straight into the store and
/// returns its id.
fn add_account(store: &Store, login: &str, address: &str) -> String {
    let scope = scope::resolve(store, &user_id(store, login), None).unwrap();
    accounts::add(store, &keys(), &scope, &new_account(address))
        .unwrap()
        .id
        .as_str()
        .to_owned()
}

/// The fixture keys and the router keys derive from the same secret, so
/// a row sealed by the one opens under the other.
#[test]
fn the_router_keys_open_a_row_the_fixture_sealed() {
    let store = store_with_account();
    let id = add_account(&store, LOGIN, ADDRESS);
    let api = api_state(Arc::clone(&store));
    let scope =
        scope::resolve(&store, &user_id(&store, LOGIN), Some(&AccountId::from(id))).unwrap();
    let credential = accounts::credential(&store, &api.keys, &scope).unwrap();
    assert_eq!(
        credential,
        Credential::Bearer {
            token: ACCOUNT_TOKEN.to_owned()
        }
    );
}

fn store_with_two_users() -> Arc<Store> {
    let store = store_with_account();
    let (_, other) = identity::create_personal_user(&store, OTHER_LOGIN).unwrap();
    auth::set_password(&store, &other.id, PASSWORD).unwrap();
    store
}

async fn list_response(router: &Router, cookie: &str) -> String {
    let response = router
        .clone()
        .oneshot(with_cookie(Method::GET, "/api/accounts", cookie))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    body_text(response).await
}

async fn listed(router: &Router, cookie: &str) -> Listed {
    serde_json::from_str(&list_response(router, cookie).await).unwrap()
}

async fn status_of(router: &Router, method: Method, uri: &str, cookie: &str) -> StatusCode {
    router
        .clone()
        .oneshot(with_cookie(method, uri, cookie))
        .await
        .unwrap()
        .status()
}

async fn sign_in_other(router: &Router) -> String {
    let response = router
        .clone()
        .oneshot(login_request(OTHER_LOGIN, PASSWORD))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    cookie_of(&response)
}

#[tokio::test]
async fn another_users_account_id_is_not_found() {
    let store = store_with_two_users();
    let theirs = add_account(&store, OTHER_LOGIN, "noor@fastmail.com");
    let router = router_on(store);
    let mine = sign_in(&router).await;
    let status = status_of(
        &router,
        Method::DELETE,
        &format!("/api/accounts/{theirs}"),
        &mine,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let their_cookie = sign_in_other(&router).await;
    assert_eq!(listed(&router, &their_cookie).await.accounts.len(), 1);
}

#[tokio::test]
async fn the_list_stays_inside_the_own_user() {
    let store = store_with_two_users();
    let mine = add_account(&store, LOGIN, ADDRESS);
    add_account(&store, OTHER_LOGIN, "noor@fastmail.com");
    let router = router_on(store);
    let cookie = sign_in(&router).await;
    let rows = listed(&router, &cookie).await.accounts;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, mine);
    let other_cookie = sign_in_other(&router).await;
    let other_rows = listed(&router, &other_cookie).await.accounts;
    assert_eq!(other_rows.len(), 1);
    assert_eq!(other_rows[0].address, "noor@fastmail.com");
}

#[tokio::test]
async fn the_list_carries_no_secret_and_no_settings() {
    let store = store_with_account();
    add_account(&store, LOGIN, ADDRESS);
    let router = router_on(store);
    let cookie = sign_in(&router).await;
    let body = list_response(&router, &cookie).await;
    assert!(!body.contains(ACCOUNT_TOKEN));
    assert!(!body.contains("sessionUrl"));
    assert!(!body.contains("/jmap/session"));
}

#[tokio::test]
async fn the_list_and_the_removal_need_a_session() {
    let router = router_on(store_with_account());
    for (method, uri) in [
        (Method::GET, "/api/accounts"),
        (Method::DELETE, "/api/accounts/some-id"),
    ] {
        let status = status_of(&router, method.clone(), uri, "huliho_session=stale").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {uri}");
    }
}

#[tokio::test]
async fn a_removal_without_the_header_is_refused() {
    let store = store_with_account();
    let id = add_account(&store, LOGIN, ADDRESS);
    let router = router_on(store);
    let cookie = sign_in(&router).await;
    let request = Request::builder()
        .method(Method::DELETE)
        .uri(format!("/api/accounts/{id}"))
        .header(header::COOKIE, &cookie)
        .body(Body::empty())
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(body_text(response).await.contains("missing_csrf_header"));
    assert_eq!(listed(&router, &cookie).await.accounts.len(), 1);
}

#[tokio::test]
async fn the_list_carries_the_rows_and_the_probe_interval() {
    let store = store_with_account();
    let first = add_account(&store, LOGIN, ADDRESS);
    let second = add_account(&store, LOGIN, "mira@example.net");
    let router = router_on(store);
    let cookie = sign_in(&router).await;
    let listed = listed(&router, &cookie).await;
    assert_eq!(
        listed.probe_interval_minutes,
        UpstreamConfig::default().probe_interval_minutes.get()
    );
    let mut ids: Vec<&str> = listed.accounts.iter().map(|row| row.id.as_str()).collect();
    ids.sort_unstable();
    let mut expected = [first.as_str(), second.as_str()];
    expected.sort_unstable();
    assert_eq!(ids, expected);
    let row = listed.accounts.iter().find(|row| row.id == first).unwrap();
    assert_eq!(row.address, ADDRESS);
    assert_eq!(row.name, "Fastmail");
    assert_eq!(row.provider, "fastmail");
    assert_eq!(row.kind, "jmap");
    assert_eq!(row.auth_method, "bearer");
    assert_eq!(row.stopped_cause, None);
    assert_eq!(row.stopped_at, None);
    assert!(row.created_at > 0);
}

#[tokio::test]
async fn removing_an_account_answers_no_content_and_then_not_found() {
    let store = store_with_account();
    let id = add_account(&store, LOGIN, ADDRESS);
    let router = router_on(store);
    let cookie = sign_in(&router).await;
    let uri = format!("/api/accounts/{id}");
    let status = status_of(&router, Method::DELETE, &uri, &cookie).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(listed(&router, &cookie).await.accounts.is_empty());
    let status = status_of(&router, Method::DELETE, &uri, &cookie).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
