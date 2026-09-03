// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The password change over HTTP: chosen, forced and abused.

mod common;
mod one_time;
mod signin;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, Response, StatusCode, header};
use tower::ServiceExt;

use common::router_on;
use huliho_server::auth::MAX_PASSWORD_CHARS;
use one_time::{issued, json_with_cookie};
use signin::{
    LOGIN, PASSWORD, body_text, cookie_of, login_request, sign_in, store_with_account, with_cookie,
};

const NEW_PASSWORD: &str = "a brand new passphrase";

/// Enough failures to pass the limiter's free run in one test.
const FAILURES_TO_TRIP: usize = 8;

fn change_body(current: Option<&str>, new: &str) -> String {
    match current {
        Some(current) => format!("{{\"current\":\"{current}\",\"new\":\"{new}\"}}"),
        None => format!("{{\"new\":\"{new}\"}}"),
    }
}

async fn change(router: &Router, cookie: &str, body: &str) -> Response<Body> {
    router
        .clone()
        .oneshot(json_with_cookie(Method::PUT, "/api/password", cookie, body))
        .await
        .unwrap()
}

async fn status_of(router: &Router, method: Method, uri: &str, cookie: &str) -> StatusCode {
    router
        .clone()
        .oneshot(with_cookie(method, uri, cookie))
        .await
        .unwrap()
        .status()
}

/// Creates a member through the API, signs in with the one-time password
/// and returns the member's cookie; the secret is dead afterwards.
async fn forced_session(router: &Router) -> String {
    let owner = sign_in(router).await;
    let created = router
        .clone()
        .oneshot(json_with_cookie(
            Method::POST,
            "/api/users",
            &owner,
            "{\"name\":\"Jonas Verhulst\",\"login\":\"jonas\",\"role\":\"member\"}",
        ))
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::CREATED);
    let secret = issued(created).await.one_time_password;
    let response = router
        .clone()
        .oneshot(login_request("jonas", &secret))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let member = cookie_of(&response);
    let again = router
        .clone()
        .oneshot(login_request("jonas", &secret))
        .await
        .unwrap();
    assert_eq!(again.status(), StatusCode::UNAUTHORIZED);
    member
}

#[tokio::test]
async fn a_wrong_current_password_is_refused_and_counted() {
    let router = router_on(store_with_account());
    let cookie = sign_in(&router).await;
    let mut last = StatusCode::UNAUTHORIZED;
    for _ in 0..FAILURES_TO_TRIP {
        let body = change_body(Some("not it, sorry!"), NEW_PASSWORD);
        let response = change(&router, &cookie, &body).await;
        last = response.status();
        if last == StatusCode::TOO_MANY_REQUESTS {
            assert!(response.headers().contains_key(header::RETRY_AFTER));
            break;
        }
        assert_eq!(last, StatusCode::UNAUTHORIZED);
        assert!(body_text(response).await.contains("invalid_credentials"));
    }
    assert_eq!(last, StatusCode::TOO_MANY_REQUESTS);
    let status = status_of(&router, Method::GET, "/api/session", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    let same_address = router
        .oneshot(login_request(LOGIN, PASSWORD))
        .await
        .unwrap();
    assert_eq!(same_address.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn a_change_rotates_the_cookie_and_ends_the_other_sessions() {
    let router = router_on(store_with_account());
    let phone = sign_in(&router).await;
    let desktop = sign_in(&router).await;
    let body = change_body(Some(PASSWORD), NEW_PASSWORD);
    let response = change(&router, &desktop, &body).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let rotated = cookie_of(&response);
    assert_ne!(rotated, desktop);
    for cookie in [&desktop, &phone] {
        let status = status_of(&router, Method::GET, "/api/session", cookie).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
    let status = status_of(&router, Method::GET, "/api/session", &rotated).await;
    assert_eq!(status, StatusCode::OK);
    let old = router
        .clone()
        .oneshot(login_request(LOGIN, PASSWORD))
        .await
        .unwrap();
    assert_eq!(old.status(), StatusCode::UNAUTHORIZED);
    let new = router
        .oneshot(login_request(LOGIN, NEW_PASSWORD))
        .await
        .unwrap();
    assert_eq!(new.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn a_missing_short_or_oversized_password_is_a_bad_request() {
    let router = router_on(store_with_account());
    let cookie = sign_in(&router).await;
    let oversized = "x".repeat(MAX_PASSWORD_CHARS + 1);
    for body in [
        change_body(None, NEW_PASSWORD),
        change_body(Some(PASSWORD), "short"),
        change_body(Some(PASSWORD), &oversized),
        change_body(Some(&oversized), NEW_PASSWORD),
    ] {
        let response = change(&router, &cookie, &body).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{body}");
    }
    let status = status_of(&router, Method::GET, "/api/session", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    let same = router
        .oneshot(login_request(LOGIN, PASSWORD))
        .await
        .unwrap();
    assert_eq!(same.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn the_new_routes_need_a_session() {
    let router = router_on(store_with_account());
    for (method, uri) in [
        (Method::PUT, "/api/password"),
        (Method::GET, "/api/users"),
        (Method::POST, "/api/users"),
        (Method::POST, "/api/users/some-id/password-reset"),
    ] {
        let status = status_of(&router, method.clone(), uri, "huliho_session=stale").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {uri}");
    }
}

#[tokio::test]
async fn a_change_without_the_header_is_refused() {
    let router = router_on(store_with_account());
    let cookie = sign_in(&router).await;
    let request = Request::builder()
        .method(Method::PUT)
        .uri("/api/password")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::COOKIE, &cookie)
        .body(Body::from(change_body(Some(PASSWORD), NEW_PASSWORD)))
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(body_text(response).await.contains("missing_csrf_header"));
    let same = router
        .oneshot(login_request(LOGIN, PASSWORD))
        .await
        .unwrap();
    assert_eq!(same.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn a_one_time_sign_in_reaches_only_the_password_change() {
    let router = router_on(store_with_account());
    let member = forced_session(&router).await;
    let response = router
        .clone()
        .oneshot(with_cookie(Method::GET, "/api/session", &member))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_text(response).await;
    assert!(body.contains("\"passwordChangeRequired\":true"));
    assert!(body.contains("\"name\":\"Jonas Verhulst\""));
    for (method, uri) in [
        (Method::GET, "/api/sessions"),
        (Method::DELETE, "/api/sessions"),
        (Method::DELETE, "/api/sessions/some-id"),
        (Method::GET, "/api/users"),
        (Method::POST, "/api/users"),
        (Method::POST, "/api/users/some-id/password-reset"),
    ] {
        let response = router
            .clone()
            .oneshot(with_cookie(method.clone(), uri, &member))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{method} {uri}");
        assert!(
            body_text(response)
                .await
                .contains("password_change_required")
        );
    }
    let body = change_body(Some("ignored in the forced step"), NEW_PASSWORD);
    let response = change(&router, &member, &body).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let rotated = cookie_of(&response);
    let status = status_of(&router, Method::GET, "/api/session", &member).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let response = router
        .clone()
        .oneshot(with_cookie(Method::GET, "/api/session", &rotated))
        .await
        .unwrap();
    assert!(
        body_text(response)
            .await
            .contains("\"passwordChangeRequired\":false")
    );
    let status = status_of(&router, Method::GET, "/api/sessions", &rotated).await;
    assert_eq!(status, StatusCode::OK);
    let signed_in = router
        .oneshot(login_request("jonas", NEW_PASSWORD))
        .await
        .unwrap();
    assert_eq!(signed_in.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn a_forced_session_can_still_sign_out() {
    let router = router_on(store_with_account());
    let member = forced_session(&router).await;
    let status = status_of(&router, Method::DELETE, "/api/session", &member).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let status = status_of(&router, Method::GET, "/api/session", &member).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
