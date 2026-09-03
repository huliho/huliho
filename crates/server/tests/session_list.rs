// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The session list and revocation over HTTP.

mod common;
mod signin;

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Method, Request, StatusCode, header};
use serde::Deserialize;
use tower::ServiceExt;

use common::router_on;
use huliho_server::config::AuthConfig;
use huliho_server::session::SessionTimeouts;
use huliho_server::store::Store;
use huliho_server::{auth, identity};
use signin::{
    LOGIN, PASSWORD, body_text, cookie_of, login_request, sign_in, sign_in_as, store_with_account,
    with_cookie,
};

const FIREFOX: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0";
const OTHER_LOGIN: &str = "noor@example.com";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Listed {
    id: String,
    current: bool,
    device: ListedDevice,
    address: Option<String>,
    created_at: i64,
    last_seen_at: i64,
}

#[derive(Deserialize)]
struct ListedDevice {
    browser: Option<String>,
    os: Option<String>,
    phone: bool,
    installed: bool,
}

async fn listed(router: &Router, cookie: &str) -> Vec<Listed> {
    let response = router
        .clone()
        .oneshot(with_cookie(Method::GET, "/api/sessions", cookie))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    serde_json::from_str(&body_text(response).await).unwrap()
}

async fn status_of(router: &Router, method: Method, uri: &str, cookie: &str) -> StatusCode {
    router
        .clone()
        .oneshot(with_cookie(method, uri, cookie))
        .await
        .unwrap()
        .status()
}

fn store_with_two_accounts() -> Arc<Store> {
    let store = store_with_account();
    let (_, other) = identity::create_personal_user(&store, OTHER_LOGIN).unwrap();
    auth::set_password(&store, &other.id, PASSWORD).unwrap();
    store
}

#[tokio::test]
async fn the_list_puts_the_current_session_first_with_its_device() {
    let router = router_on(store_with_account());
    let phone = sign_in_as(&router, FIREFOX).await;
    let desktop = sign_in(&router).await;
    let rows = listed(&router, &desktop).await;
    assert_eq!(rows.len(), 2);
    assert!(rows[0].current);
    assert!(!rows[1].current);
    assert_eq!(rows[1].device.browser.as_deref(), Some("Firefox"));
    assert_eq!(rows[1].device.os.as_deref(), Some("Linux"));
    assert!(!rows[1].device.phone);
    assert!(!rows[1].device.installed);
    assert_eq!(rows[0].address, None);
    assert!(rows[0].created_at > 0 && rows[0].last_seen_at >= rows[0].created_at);
    let from_phone = listed(&router, &phone).await;
    assert!(from_phone[0].current);
    assert_eq!(from_phone[0].id, rows[1].id);
}

#[tokio::test]
async fn a_revoked_session_is_rejected_at_its_next_request() {
    let router = router_on(store_with_account());
    let phone = sign_in(&router).await;
    let desktop = sign_in(&router).await;
    let phone_id = listed(&router, &phone).await[0].id.clone();
    let status = status_of(
        &router,
        Method::DELETE,
        &format!("/api/sessions/{phone_id}"),
        &desktop,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let status = status_of(&router, Method::GET, "/api/session", &phone).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(listed(&router, &desktop).await.len(), 1);
}

#[tokio::test]
async fn another_users_session_id_is_not_found() {
    let router = router_on(store_with_two_accounts());
    let mine = sign_in(&router).await;
    let response = router
        .clone()
        .oneshot(login_request(OTHER_LOGIN, PASSWORD))
        .await
        .unwrap();
    let theirs = cookie_of(&response);
    let their_id = listed(&router, &theirs).await[0].id.clone();
    let status = status_of(
        &router,
        Method::DELETE,
        &format!("/api/sessions/{their_id}"),
        &mine,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        status_of(&router, Method::GET, "/api/session", &theirs).await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn the_current_session_is_not_revoked_through_the_list() {
    let router = router_on(store_with_account());
    let cookie = sign_in(&router).await;
    let id = listed(&router, &cookie).await[0].id.clone();
    let status = status_of(
        &router,
        Method::DELETE,
        &format!("/api/sessions/{id}"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        status_of(&router, Method::GET, "/api/session", &cookie).await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn revoking_the_others_keeps_only_the_current_session() {
    let router = router_on(store_with_account());
    let first = sign_in(&router).await;
    let second = sign_in(&router).await;
    let current = sign_in(&router).await;
    assert_eq!(listed(&router, &current).await.len(), 3);
    let status = status_of(&router, Method::DELETE, "/api/sessions", &current).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    for cookie in [&first, &second] {
        assert_eq!(
            status_of(&router, Method::GET, "/api/session", cookie).await,
            StatusCode::UNAUTHORIZED
        );
    }
    let rows = listed(&router, &current).await;
    assert_eq!(rows.len(), 1);
    assert!(rows[0].current);
}

#[tokio::test]
async fn the_list_and_the_revocations_need_a_session() {
    let router = router_on(store_with_account());
    for (method, uri) in [
        (Method::GET, "/api/sessions"),
        (Method::DELETE, "/api/sessions"),
        (Method::DELETE, "/api/sessions/some-id"),
    ] {
        let status = status_of(&router, method.clone(), uri, "huliho_session=stale").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {uri}");
    }
}

#[tokio::test]
async fn a_revocation_without_the_header_is_refused() {
    let router = router_on(store_with_account());
    let cookie = sign_in(&router).await;
    for uri in ["/api/sessions", "/api/sessions/some-id"] {
        let request = Request::builder()
            .method(Method::DELETE)
            .uri(uri)
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{uri}");
        assert!(body_text(response).await.contains("missing_csrf_header"));
    }
    assert_eq!(listed(&router, &cookie).await.len(), 1);
}

#[tokio::test]
async fn the_address_the_listener_saw_lands_on_the_row() {
    let router = router_on(store_with_account());
    let mut request = login_request(LOGIN, PASSWORD);
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from((
            Ipv4Addr::new(203, 0, 113, 7),
            54_321,
        ))));
    let response = router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let cookie = cookie_of(&response);
    let rows = listed(&router, &cookie).await;
    assert_eq!(rows[0].address.as_deref(), Some("203.0.113.7"));
}

#[tokio::test]
async fn an_aged_session_leaves_the_list_and_its_row_can_still_be_revoked() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let store = Arc::new(Store::open(&data).unwrap());
    let (_, user) = identity::create_personal_user(&store, LOGIN).unwrap();
    auth::set_password(&store, &user.id, PASSWORD).unwrap();
    let router = router_on(store);
    let stale = sign_in(&router).await;
    let stale_id = listed(&router, &stale).await[0].id.clone();
    let current = sign_in(&router).await;
    let idle_ms = SessionTimeouts::from(&AuthConfig::default()).idle_ms;
    let database = rusqlite::Connection::open(data.join("huliho.db")).unwrap();
    database
        .execute(
            "UPDATE sessions SET last_seen_at = last_seen_at - ?1 WHERE id = ?2",
            rusqlite::params![idle_ms, stale_id],
        )
        .unwrap();
    let status = status_of(&router, Method::GET, "/api/session", &stale).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let rows = listed(&router, &current).await;
    assert_eq!(rows.len(), 1);
    assert!(rows[0].current);
    let status = status_of(
        &router,
        Method::DELETE,
        &format!("/api/sessions/{stale_id}"),
        &current,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let remaining: i64 = database
        .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
        .unwrap();
    assert_eq!(remaining, 1);
}

#[tokio::test]
async fn the_session_answer_carries_the_name() {
    let router = router_on(store_with_account());
    let cookie = sign_in(&router).await;
    let response = router
        .oneshot(with_cookie(Method::GET, "/api/session", &cookie))
        .await
        .unwrap();
    assert!(
        body_text(response)
            .await
            .contains("\"name\":\"mira@example.com\"")
    );
}
