// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Response contract of the HTTP skeleton: liveness, headers and SPA fallback.

use std::path::PathBuf;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

fn fixture_router() -> Router {
    let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/spa");
    huliho_server::app::router(&assets)
}

async fn get(path: &str) -> axum::http::Response<axum::body::Body> {
    let request = Request::get(path).body(Body::empty()).unwrap();
    fixture_router().oneshot(request).await.unwrap()
}

async fn body_text(response: axum::http::Response<axum::body::Body>) -> String {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn healthz_answers_liveness_only() {
    let response = get("/healthz").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_text(response).await, "ok");
}

#[tokio::test]
async fn every_response_carries_the_security_headers() {
    for path in ["/healthz", "/", "/mail/inbox"] {
        let response = get(path).await;
        let headers = response.headers();
        assert_eq!(
            headers.get("content-security-policy").unwrap(),
            "default-src 'self'; frame-ancestors 'none'; base-uri 'self'; form-action 'self'",
            "csp on {path}"
        );
        assert_eq!(headers.get("x-content-type-options").unwrap(), "nosniff");
        assert_eq!(headers.get("referrer-policy").unwrap(), "no-referrer");
        assert_eq!(
            headers.get("permissions-policy").unwrap(),
            "camera=(), geolocation=(), microphone=()"
        );
        assert_eq!(
            headers.get("strict-transport-security").unwrap(),
            "max-age=63072000; includeSubDomains"
        );
    }
}

#[tokio::test]
async fn every_response_carries_a_request_id() {
    let response = get("/healthz").await;
    let request_id = response.headers().get("x-request-id").unwrap();
    assert!(!request_id.is_empty());
}

#[tokio::test]
async fn unknown_route_serves_the_spa_shell() {
    let response = get("/mail/inbox").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(body_text(response).await.contains("<p>shell</p>"));
}

#[tokio::test]
async fn existing_asset_is_served_directly() {
    let response = get("/index.html").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers().get("content-type").unwrap(), "text/html");
}
