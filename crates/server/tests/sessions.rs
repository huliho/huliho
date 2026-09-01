// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Session endpoints over HTTP: cookie contract, guards and persistence.

mod common;

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use tower::ServiceExt;

use common::router_on;
use huliho_server::api::MAX_CONCURRENT_VERIFICATIONS;
use huliho_server::store::Store;
use huliho_server::{auth, identity};

const LOGIN: &str = "mira@example.com";
const PASSWORD: &str = "example passphrase";

/// Enough failures to pass the limiter's free run in one test.
const FAILURES_TO_TRIP: usize = 8;

/// Long enough to prove a held login is waiting, not failing.
const GATE_WAIT: Duration = Duration::from_millis(200);

fn store_with_account() -> Arc<Store> {
    let store = Arc::new(Store::in_memory().unwrap());
    let (_, user) = identity::create_personal_user(&store, LOGIN).unwrap();
    auth::set_password(&store, &user.id, PASSWORD).unwrap();
    store
}

fn login_request(login: &str, password: &str) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri("/api/session")
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-requested-with", "huliho")
        .body(Body::from(format!(
            "{{\"login\":\"{login}\",\"password\":\"{password}\"}}"
        )))
        .unwrap()
}

fn with_cookie(method: Method, cookie: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri("/api/session")
        .header(header::COOKIE, cookie)
        .header("x-requested-with", "huliho")
        .body(Body::empty())
        .unwrap()
}

async fn sign_in(router: &Router) -> String {
    let response = router
        .clone()
        .oneshot(login_request(LOGIN, PASSWORD))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let set_cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap();
    set_cookie.split(';').next().unwrap().to_owned()
}

async fn body_text(response: axum::http::Response<Body>) -> String {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn a_login_sets_a_locked_down_cookie() {
    let router = router_on(store_with_account());
    let response = router
        .oneshot(login_request(LOGIN, PASSWORD))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let set_cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(set_cookie.starts_with("huliho_session="));
    assert!(set_cookie.contains("HttpOnly"));
    assert!(set_cookie.contains("Secure"));
    assert!(set_cookie.contains("SameSite=Lax"));
    assert!(set_cookie.contains("Path=/"));
    assert!(set_cookie.contains("Max-Age="));
}

#[tokio::test]
async fn a_session_resolves_to_its_user_and_organization() {
    let router = router_on(store_with_account());
    let cookie = sign_in(&router).await;
    let response = router
        .oneshot(with_cookie(Method::GET, &cookie))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_text(response).await;
    assert!(body.contains(LOGIN));
    assert!(body.contains("\"role\":\"owner\""));
    assert!(body.contains("\"organization\""));
}

#[tokio::test]
async fn wrong_and_unknown_credentials_answer_identically() {
    let router = router_on(store_with_account());
    let wrong = router
        .clone()
        .oneshot(login_request(LOGIN, "not the password"))
        .await
        .unwrap();
    let unknown = router
        .oneshot(login_request("ghost@example.com", "not the password"))
        .await
        .unwrap();
    assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(unknown.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(body_text(wrong).await, body_text(unknown).await);
}

#[tokio::test]
async fn a_mutation_without_the_header_is_refused() {
    let router = router_on(store_with_account());
    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/session")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(body_text(response).await.contains("missing_csrf_header"));
}

#[tokio::test]
async fn the_first_request_after_a_revocation_is_rejected() {
    let router = router_on(store_with_account());
    let cookie = sign_in(&router).await;
    let response = router
        .clone()
        .oneshot(with_cookie(Method::DELETE, &cookie))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let removal = response
        .headers()
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(removal.contains("huliho_session="));
    let response = router
        .oneshot(with_cookie(Method::GET, &cookie))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn repeated_failures_trip_the_limiter() {
    let router = router_on(store_with_account());
    let mut last = StatusCode::UNAUTHORIZED;
    for _ in 0..FAILURES_TO_TRIP {
        let response = router
            .clone()
            .oneshot(login_request(LOGIN, "not the password"))
            .await
            .unwrap();
        last = response.status();
        if last == StatusCode::TOO_MANY_REQUESTS {
            assert!(response.headers().contains_key(header::RETRY_AFTER));
            return;
        }
    }
    panic!("the limiter never tripped, last status {last}");
}

#[tokio::test]
async fn login_verification_queues_behind_the_gate() {
    let api = common::api_state(store_with_account());
    let gate = Arc::clone(&api.verify_gate);
    let router = common::router_with(api);
    let permits = gate
        .acquire_many(u32::try_from(MAX_CONCURRENT_VERIFICATIONS).unwrap())
        .await
        .unwrap();
    let held = tokio::time::timeout(
        GATE_WAIT,
        router.clone().oneshot(login_request(LOGIN, PASSWORD)),
    )
    .await;
    assert!(held.is_err(), "a login ran past the exhausted gate");
    drop(permits);
    let response = router
        .oneshot(login_request(LOGIN, PASSWORD))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn a_session_survives_a_server_restart() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let cookie = {
        let store = Arc::new(Store::open(&data).unwrap());
        let (_, user) = identity::create_personal_user(&store, LOGIN).unwrap();
        auth::set_password(&store, &user.id, PASSWORD).unwrap();
        sign_in(&router_on(store)).await
    };
    let reopened = Arc::new(Store::open(&data).unwrap());
    let response = router_on(reopened)
        .oneshot(with_cookie(Method::GET, &cookie))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn the_license_text_is_served_by_the_binary() {
    let router = router_on(store_with_account());
    let request = Request::get("/license").body(Body::empty()).unwrap();
    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        body_text(response)
            .await
            .contains("GNU AFFERO GENERAL PUBLIC LICENSE")
    );
}
