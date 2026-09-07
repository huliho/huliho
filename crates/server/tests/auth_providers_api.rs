// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The sign-in provider routes over HTTP: instance admins only, the
//! secret sealed and never shown.

mod common;
mod signin;

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, StatusCode, header};
use common::{api_state, router_on, router_with};
use huliho_server::auth;
use huliho_server::identity;
use huliho_server::providers::{self, OauthProvider};
use huliho_server::store::Store;
use serde_json::{Value, json};
use signin::{
    LOGIN, PASSWORD, body_text, cookie_of, login_request, sign_in, store_with_account, with_cookie,
};
use tower::ServiceExt;

const ROUTE: &str = "/api/auth-providers";
const OTHER_LOGIN: &str = "noor@example.com";
const CLIENT_ID: &str = "fixture-client-id.apps.googleusercontent.com";
const CLIENT_SECRET: &str = "GOCSPX-fixture-secret";

fn register_body(id: &str, secret: &str) -> Value {
    json!({ "clientId": id, "clientSecret": secret })
}

async fn put(router: &Router, cookie: &str, provider: &str, body: &Value) -> (StatusCode, String) {
    let mut request = with_cookie(Method::PUT, &format!("{ROUTE}/{provider}"), cookie);
    request
        .headers_mut()
        .insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
    *request.body_mut() = Body::from(body.to_string());
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    (status, body_text(response).await)
}

async fn list(router: &Router, cookie: &str) -> (StatusCode, String) {
    let response = router
        .clone()
        .oneshot(with_cookie(Method::GET, ROUTE, cookie))
        .await
        .unwrap();
    let status = response.status();
    (status, body_text(response).await)
}

/// The fixture owner with the flag plus a second organization's owner
/// without it.
fn store_with_instance_admin() -> Arc<Store> {
    let store = store_with_account();
    identity::grant_instance_admin(&store, LOGIN).unwrap();
    let (_, other) = identity::create_personal_user(&store, OTHER_LOGIN).unwrap();
    auth::set_password(&store, &other.id, PASSWORD).unwrap();
    store
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
async fn the_routes_need_a_session_and_the_write_needs_the_header() {
    let router = router_on(store_with_instance_admin());
    let (status, _) = list(&router, "huliho_session=stale").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = put(
        &router,
        "huliho_session=stale",
        "google",
        &register_body(CLIENT_ID, CLIENT_SECRET),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let cookie = sign_in(&router).await;
    let mut request = with_cookie(Method::PUT, &format!("{ROUTE}/google"), &cookie);
    request.headers_mut().remove("x-requested-with");
    request
        .headers_mut()
        .insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
    *request.body_mut() = Body::from(register_body(CLIENT_ID, CLIENT_SECRET).to_string());
    let response = router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(body_text(response).await.contains("missing_csrf_header"));
}

#[tokio::test]
async fn an_owner_without_the_flag_is_forbidden_on_both_routes() {
    let store = store_with_account();
    let router = router_on(Arc::clone(&store));
    let cookie = sign_in(&router).await;
    let (status, body) = list(&router, &cookie).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert!(body.contains("\"forbidden\""));
    let (status, body) = put(
        &router,
        &cookie,
        "google",
        &register_body(CLIENT_ID, CLIENT_SECRET),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert!(!providers::is_registered(&store, OauthProvider::Google).unwrap());
}

#[tokio::test]
async fn a_second_organizations_owner_is_forbidden_too() {
    let store = store_with_instance_admin();
    let router = router_on(Arc::clone(&store));
    let other = sign_in_other(&router).await;
    let (status, _) = list(&router, &other).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = put(
        &router,
        &other,
        "microsoft",
        &register_body(CLIENT_ID, CLIENT_SECRET),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(!providers::is_registered(&store, OauthProvider::Microsoft).unwrap());
}

#[tokio::test]
async fn an_instance_admin_registers_lists_and_replaces_without_seeing_the_secret() {
    let store = store_with_instance_admin();
    let api = api_state(Arc::clone(&store));
    let router = router_with(api.clone());
    let cookie = sign_in(&router).await;
    let (status, _) = list(&router, &cookie).await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = put(
        &router,
        &cookie,
        "google",
        &register_body(CLIENT_ID, CLIENT_SECRET),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (status, body) = list(&router, &cookie).await;
    assert_eq!(status, StatusCode::OK);
    let listed: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        listed,
        json!([{ "provider": "google", "clientId": CLIENT_ID }])
    );
    assert!(!body.contains(CLIENT_SECRET));
    let opened = providers::client(&store, &api.keys, OauthProvider::Google)
        .unwrap()
        .unwrap();
    assert_eq!(
        (opened.id.as_str(), opened.secret.as_str()),
        (CLIENT_ID, CLIENT_SECRET)
    );
    let (status, _) = put(
        &router,
        &cookie,
        "google",
        &register_body("second-id", "second-secret"),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = put(
        &router,
        &cookie,
        "microsoft",
        &register_body("ms-id", "ms-secret"),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, body) = list(&router, &cookie).await;
    let listed: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        listed,
        json!([
            { "provider": "google", "clientId": "second-id" },
            { "provider": "microsoft", "clientId": "ms-id" }
        ])
    );
}

#[tokio::test]
async fn an_unknown_provider_word_is_not_found_and_bad_fields_are_refused() {
    let store = store_with_instance_admin();
    let router = router_on(Arc::clone(&store));
    let cookie = sign_in(&router).await;
    let (status, body) = put(
        &router,
        &cookie,
        "yahoo",
        &register_body(CLIENT_ID, CLIENT_SECRET),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert!(body.contains("not_found"));
    for (label, body) in [
        ("an empty id", register_body("", CLIENT_SECRET)),
        (
            "a control character in the secret",
            register_body(CLIENT_ID, "sec\nret"),
        ),
        (
            "an oversized secret",
            register_body(CLIENT_ID, &"x".repeat(1025)),
        ),
    ] {
        let (status, text) = put(&router, &cookie, "google", &body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{label}: {text}");
        assert!(text.contains("invalid_request"), "{label}: {text}");
    }
    let (status, _) = put(
        &router,
        &cookie,
        "google",
        &json!({ "clientId": CLIENT_ID }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(!providers::is_registered(&store, OauthProvider::Google).unwrap());
}
