// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The preferences over HTTP: the listed keys, their words and whose
//! they are.

mod common;
mod signin;
mod user_routes;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use common::router_on;
use serde_json::{Value, json};
use signin::{body_text, sign_in, store_with_account};
use tower::ServiceExt;
use user_routes::{get_json, put_json, sign_in_member, store_with_member};

const ROUTE: &str = "/api/preferences";

async fn listed(router: &Router, cookie: &str) -> (StatusCode, Value) {
    get_json(router, cookie, ROUTE).await
}

async fn put(router: &Router, cookie: &str, key: &str, value: &Value) -> (StatusCode, String) {
    put_json(
        router,
        cookie,
        &format!("{ROUTE}/{key}"),
        &json!({ "value": value }),
    )
    .await
}

#[tokio::test]
async fn both_routes_need_a_session_and_the_write_the_header() {
    let router = router_on(store_with_account());
    let (status, _) = listed(&router, "huliho_session=stale").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = put(&router, "huliho_session=stale", "theme", &json!("dark")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let cookie = sign_in(&router).await;
    let bare = Request::builder()
        .method(Method::PUT)
        .uri(format!("{ROUTE}/theme"))
        .header(header::COOKIE, &cookie)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "value": "dark" }).to_string()))
        .unwrap();
    let response = router.clone().oneshot(bare).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(body_text(response).await.contains("missing_csrf_header"));
    let (_, preferences) = listed(&router, &cookie).await;
    assert_eq!(preferences, json!({}));
}

#[tokio::test]
async fn a_fresh_user_has_no_preferences_and_a_round_trip_holds() {
    let router = router_on(store_with_account());
    let cookie = sign_in(&router).await;
    let (status, preferences) = listed(&router, &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(preferences, json!({}));
    for (key, word) in [
        ("readingPane", "bottom"),
        ("theme", "dark"),
        ("density", "compact"),
        ("locale", "nl"),
        ("fontSize", "large"),
        ("lineHeight", "loose"),
        ("darkMail", "original"),
    ] {
        let (status, text) = put(&router, &cookie, key, &json!(word)).await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{key}: {text}");
        assert!(text.is_empty(), "{text}");
    }
    let (_, preferences) = listed(&router, &cookie).await;
    assert_eq!(
        preferences,
        json!({
            "readingPane": "bottom",
            "theme": "dark",
            "density": "compact",
            "locale": "nl",
            "fontSize": "large",
            "lineHeight": "loose",
            "darkMail": "original"
        })
    );
    let (status, _) = put(&router, &cookie, "readingPane", &json!("off")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = put(&router, &cookie, "fontSize", &json!("larger")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, preferences) = listed(&router, &cookie).await;
    assert_eq!(preferences["readingPane"], "off");
    assert_eq!(preferences["fontSize"], "larger");
    assert_eq!(preferences["lineHeight"], "loose");
}

#[tokio::test]
async fn a_word_off_the_list_is_invalid_and_a_key_off_the_list_is_not_found() {
    let router = router_on(store_with_account());
    let cookie = sign_in(&router).await;
    for (key, value) in [
        ("readingPane", json!("left")),
        ("theme", json!("")),
        ("density", json!("Compact")),
        ("locale", json!("en-XA")),
        ("fontSize", json!("small")),
        ("lineHeight", json!("tight")),
        ("darkMail", json!("Adapt")),
    ] {
        let (status, text) = put(&router, &cookie, key, &value).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{key}: {text}");
        assert!(text.contains("invalid_request"), "{text}");
    }
    let (status, text) = put(&router, &cookie, "compose_size", &json!("wide")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{text}");
    assert!(text.contains("not_found"), "{text}");
    let (status, _) = put(&router, &cookie, "theme", &json!(7)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (_, preferences) = listed(&router, &cookie).await;
    assert_eq!(preferences, json!({}));
}

#[tokio::test]
async fn preferences_stay_with_their_user() {
    let router = router_on(store_with_member());
    let owner = sign_in(&router).await;
    let member = sign_in_member(&router).await;
    let (status, _) = put(&router, &owner, "theme", &json!("dark")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, preferences) = listed(&router, &member).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(preferences, json!({}));
    let (status, _) = put(&router, &member, "density", &json!("compact")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, preferences) = listed(&router, &owner).await;
    assert_eq!(preferences, json!({ "theme": "dark" }));
    let (_, preferences) = listed(&router, &member).await;
    assert_eq!(preferences, json!({ "density": "compact" }));
}
