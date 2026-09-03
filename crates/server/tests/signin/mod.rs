// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Fixtures for tests that sign a user in over HTTP.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use tower::ServiceExt;

use huliho_server::store::Store;
use huliho_server::{auth, identity};

pub const LOGIN: &str = "mira@example.com";
pub const PASSWORD: &str = "example passphrase";

/// A store holding one owner who can sign in.
pub fn store_with_account() -> Arc<Store> {
    let store = Arc::new(Store::in_memory().unwrap());
    let (_, user) = identity::create_personal_user(&store, LOGIN).unwrap();
    auth::set_password(&store, &user.id, PASSWORD).unwrap();
    store
}

pub fn login_request(login: &str, password: &str) -> Request<Body> {
    login_request_as(login, password, "test")
}

/// A sign-in carrying the given User-Agent.
pub fn login_request_as(login: &str, password: &str, user_agent: &str) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri("/api/session")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::USER_AGENT, user_agent)
        .header("x-requested-with", "huliho")
        .body(Body::from(format!(
            "{{\"login\":\"{login}\",\"password\":\"{password}\"}}"
        )))
        .unwrap()
}

pub fn with_cookie(method: Method, uri: &str, cookie: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::COOKIE, cookie)
        .header("x-requested-with", "huliho")
        .body(Body::empty())
        .unwrap()
}

pub async fn sign_in(router: &Router) -> String {
    sign_in_as(router, "test").await
}

/// Signs the fixture user in from a client with the given User-Agent
/// and returns the cookie pair.
pub async fn sign_in_as(router: &Router, user_agent: &str) -> String {
    let response = router
        .clone()
        .oneshot(login_request_as(LOGIN, PASSWORD, user_agent))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    cookie_of(&response)
}

/// The cookie pair a response set.
pub fn cookie_of(response: &axum::http::Response<Body>) -> String {
    let set_cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap();
    set_cookie.split(';').next().unwrap().to_owned()
}

pub async fn body_text(response: axum::http::Response<Body>) -> String {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}
