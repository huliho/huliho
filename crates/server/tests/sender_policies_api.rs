// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The sender policies over HTTP: the list, the grant, its removal,
//! the bounds and whose rows they are.

mod common;
mod signin;
mod user_routes;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use common::router_on;
use huliho_server::prefs::{MAX_AUTHSERV_BYTES, MAX_SENDER_BYTES, MAX_SENDER_POLICIES};
use serde_json::{Value, json};
use signin::{body_text, sign_in, store_with_account, with_cookie};
use tower::ServiceExt;
use user_routes::{get_json, put_json, sign_in_member, store_with_member};

const ROUTE: &str = "/api/sender-policies";
const SENDER: &str = "news@example.com";

fn grant(authserv: Option<&str>) -> Value {
    json!({ "key": "remoteContent", "value": { "allow": true, "authserv": authserv } })
}

async fn listed(router: &Router, cookie: &str) -> (StatusCode, Value) {
    get_json(router, cookie, ROUTE).await
}

async fn put(router: &Router, cookie: &str, sender: &str, body: &Value) -> (StatusCode, String) {
    put_json(router, cookie, &format!("{ROUTE}/{sender}"), body).await
}

async fn delete(router: &Router, cookie: &str, sender: &str, key: &str) -> (StatusCode, String) {
    let request = with_cookie(Method::DELETE, &format!("{ROUTE}/{sender}/{key}"), cookie);
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    (status, body_text(response).await)
}

#[tokio::test]
async fn every_route_needs_a_session_and_the_writes_the_header() {
    let router = router_on(store_with_account());
    let (status, _) = listed(&router, "huliho_session=stale").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = put(&router, "huliho_session=stale", SENDER, &grant(None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = delete(&router, "huliho_session=stale", SENDER, "remoteContent").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let cookie = sign_in(&router).await;
    for method in [Method::PUT, Method::DELETE] {
        let uri = if method == Method::PUT {
            format!("{ROUTE}/{SENDER}")
        } else {
            format!("{ROUTE}/{SENDER}/remoteContent")
        };
        let bare = Request::builder()
            .method(method.clone())
            .uri(uri)
            .header(header::COOKIE, &cookie)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(grant(None).to_string()))
            .unwrap();
        let response = router.clone().oneshot(bare).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{method}");
        assert!(body_text(response).await.contains("missing_csrf_header"));
    }
    let (_, policies) = listed(&router, &cookie).await;
    assert_eq!(policies, json!([]));
}

#[tokio::test]
async fn a_grant_round_trips_with_its_pin_lowercased_and_leaves_on_delete() {
    let router = router_on(store_with_account());
    let cookie = sign_in(&router).await;
    let (status, policies) = listed(&router, &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(policies, json!([]));
    let (status, text) = put(
        &router,
        &cookie,
        "News@Example.COM",
        &grant(Some("mx.google.com")),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{text}");
    let (status, text) = put(&router, &cookie, "shop@example.com", &grant(None)).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{text}");
    let (_, policies) = listed(&router, &cookie).await;
    assert_eq!(
        policies,
        json!([
            { "sender": SENDER, "key": "remoteContent", "value": { "allow": true, "authserv": "mx.google.com" } },
            { "sender": "shop@example.com", "key": "remoteContent", "value": { "allow": true, "authserv": null } }
        ])
    );
    let (status, _) = put(&router, &cookie, SENDER, &grant(None)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, policies) = listed(&router, &cookie).await;
    assert_eq!(policies[0]["value"]["authserv"], Value::Null);
    let (status, text) = delete(&router, &cookie, "NEWS@example.com", "remoteContent").await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{text}");
    let (status, _) = delete(&router, &cookie, SENDER, "remoteContent").await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, policies) = listed(&router, &cookie).await;
    assert_eq!(policies.as_array().unwrap().len(), 1);
    assert_eq!(policies[0]["sender"], "shop@example.com");
}

#[tokio::test]
async fn a_sender_a_key_or_a_value_off_the_shape_is_refused() {
    let router = router_on(store_with_account());
    let cookie = sign_in(&router).await;
    let long_sender = format!("{}@example.com", "a".repeat(MAX_SENDER_BYTES));
    let (status, text) = put(&router, &cookie, &long_sender, &grant(None)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{text}");
    assert!(text.contains("invalid_request"), "{text}");
    let (status, _) = put(&router, &cookie, "news%0A@example.com", &grant(None)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let long_authserv = "a".repeat(MAX_AUTHSERV_BYTES + 1);
    for body in [
        json!({ "key": "remoteContent", "value": { "allow": false, "authserv": null } }),
        json!({ "key": "remoteContent", "value": { "allow": true, "authserv": long_authserv } }),
        json!({ "key": "remoteContent", "value": { "allow": true, "authserv": "mx\r\n" } }),
    ] {
        let (status, text) = put(&router, &cookie, SENDER, &body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}: {text}");
        assert!(text.contains("invalid_request"), "{body}: {text}");
    }
    for body in [
        json!({ "key": "route", "value": { "allow": true, "authserv": null } }),
        json!({ "key": "remoteContent", "value": { "allow": true, "authserv": null, "since": 1 } }),
        json!({ "key": "remoteContent", "value": true }),
    ] {
        let (status, text) = put(&router, &cookie, SENDER, &body).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}: {text}");
    }
    let (status, text) = delete(&router, &cookie, SENDER, "route").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{text}");
    let (status, _) = delete(&router, &cookie, &long_sender, "remoteContent").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (_, policies) = listed(&router, &cookie).await;
    assert_eq!(policies, json!([]));
}

#[tokio::test]
async fn the_list_holds_five_thousand_rows_and_the_next_one_is_refused() {
    let router = router_on(store_with_account());
    let cookie = sign_in(&router).await;
    for index in 0..MAX_SENDER_POLICIES {
        let (status, text) = put(
            &router,
            &cookie,
            &format!("s{index}@example.com"),
            &grant(None),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{index}: {text}");
    }
    let (status, text) = put(&router, &cookie, "one-more@example.com", &grant(None)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{text}");
    assert!(text.contains("invalid_request"), "{text}");
    let (status, _) = put(
        &router,
        &cookie,
        "s7@example.com",
        &grant(Some("mx.example")),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, policies) = listed(&router, &cookie).await;
    assert_eq!(status, StatusCode::OK);
    let rows = policies.as_array().unwrap();
    assert_eq!(rows.len(), MAX_SENDER_POLICIES);
    assert!(
        rows.iter()
            .all(|row| row["sender"] != "one-more@example.com")
    );
    let (status, _) = delete(&router, &cookie, "s0@example.com", "remoteContent").await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = put(&router, &cookie, "one-more@example.com", &grant(None)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn policies_stay_with_their_user() {
    let router = router_on(store_with_member());
    let owner = sign_in(&router).await;
    let member = sign_in_member(&router).await;
    let (status, _) = put(&router, &owner, SENDER, &grant(Some("mx.example"))).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, policies) = listed(&router, &member).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(policies, json!([]));
    let (status, _) = delete(&router, &member, SENDER, "remoteContent").await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, policies) = listed(&router, &owner).await;
    assert_eq!(policies.as_array().unwrap().len(), 1);
    assert_eq!(policies[0]["sender"], SENDER);
}
