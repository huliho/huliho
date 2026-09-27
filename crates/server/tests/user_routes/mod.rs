// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Fixtures the per-user routes share: a member beside the fixture
//! owner and the JSON calls a test sends.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, StatusCode, header};
use huliho_server::auth::{self, LoginOutcome};
use huliho_server::identity::{self, NewUser};
use huliho_server::ids::Role;
use huliho_server::scope;
use huliho_server::store::Store;
use serde_json::Value;
use tower::ServiceExt;

use crate::signin::{
    LOGIN, PASSWORD, body_text, cookie_of, login_request, store_with_account, with_cookie,
};

const MEMBER_LOGIN: &str = "noor@example.com";

/// The fixture owner's store with a member of the same organization
/// who can sign in.
pub fn store_with_member() -> Arc<Store> {
    let store = store_with_account();
    let LoginOutcome::Verified(owner) = auth::verify_login(&store, LOGIN, PASSWORD).unwrap() else {
        panic!("the fixture owner signs in")
    };
    let scope = scope::resolve(&store, &owner, None).unwrap();
    let member = identity::create_organization_user(
        &store,
        &scope,
        &NewUser {
            login: MEMBER_LOGIN.to_owned(),
            name: "Noor".to_owned(),
            role: Role::Member,
        },
    )
    .unwrap();
    auth::set_password(&store, &member.id, PASSWORD).unwrap();
    store
}

pub async fn sign_in_member(router: &Router) -> String {
    let response = router
        .clone()
        .oneshot(login_request(MEMBER_LOGIN, PASSWORD))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    cookie_of(&response)
}

/// A GET on `route`: the status and the body as JSON where it parses.
pub async fn get_json(router: &Router, cookie: &str, route: &str) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(with_cookie(Method::GET, route, cookie))
        .await
        .unwrap();
    let status = response.status();
    let text = body_text(response).await;
    (
        status,
        serde_json::from_str(&text).unwrap_or(Value::String(text)),
    )
}

/// A PUT of `body` on `uri`: the status and the body as text.
pub async fn put_json(
    router: &Router,
    cookie: &str,
    uri: &str,
    body: &Value,
) -> (StatusCode, String) {
    let mut request = with_cookie(Method::PUT, uri, cookie);
    request
        .headers_mut()
        .insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
    *request.body_mut() = Body::from(body.to_string());
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    (status, body_text(response).await)
}
