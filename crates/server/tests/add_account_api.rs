// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Adding an account over HTTP: the guards, the request shape, the rate
//! limit and every upstream refusal in its own code.

mod common;
mod fake_dns;
mod jmap_session;
mod signin;
mod tls_server;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use common::{api_state, router_on, router_with};
use fake_dns::FakeDns;
use huliho_imap_bridge::testing::PASSWORD;
use huliho_server::api::ApiState;
use huliho_server::config::UpstreamConfig;
use huliho_server::upstream::Upstream;
use jmap_session::{ADDRESS, HOST, routes, session_url};
use serde_json::{Value, json};
use signin::{
    LOGIN, PASSWORD as LOGIN_PASSWORD, body_text, login_request, sign_in, store_with_account,
    with_cookie,
};
use tls_server::TlsServer;
use tokio::net::TcpListener;
use tower::ServiceExt;

const ROUTE: &str = "/api/accounts";
const DISCOVER: &str = "/api/accounts/discover";

fn json_request(cookie: &str, uri: &str, body: &Value) -> Request<Body> {
    let mut request = with_cookie(Method::POST, uri, cookie);
    request
        .headers_mut()
        .insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
    *request.body_mut() = Body::from(body.to_string());
    request
}

async fn post(router: &Router, cookie: &str, uri: &str, body: &Value) -> (StatusCode, String) {
    let response = router
        .clone()
        .oneshot(json_request(cookie, uri, body))
        .await
        .unwrap();
    let status = response.status();
    (status, body_text(response).await)
}

fn jmap_body(session_url: &str, password: &str) -> Value {
    json!({
        "address": ADDRESS,
        "provider": "generic",
        "target": { "kind": "jmap", "sessionUrl": session_url },
        "credential": { "kind": "password", "password": password },
    })
}

/// A well-formed IMAP request; the tests that send it never reach a
/// server.
fn imap_body() -> Value {
    json!({
        "address": ADDRESS,
        "provider": "generic",
        "target": {
            "kind": "imap",
            "username": "sanne",
            "imap": { "host": "imap.example.test", "port": 993, "tls": "implicit" },
            "smtp": { "host": "smtp.example.test", "port": 587, "tls": "starttls" },
        },
        "credential": { "kind": "password", "password": PASSWORD },
    })
}

fn edited(mut body: Value, edit: fn(&mut Value)) -> Value {
    edit(&mut body);
    body
}

/// The test server's name at its address on port 0, so the URL's port
/// applies.
fn dns_at(server: &TlsServer) -> FakeDns {
    let mut dns = FakeDns::default();
    dns.addresses.insert(
        HOST.to_owned(),
        vec![SocketAddr::new(server.address.ip(), 0)],
    );
    dns
}

/// A router whose upstream runs on the given rules against the test
/// server.
fn router_against(server: &TlsServer, config: &UpstreamConfig) -> Router {
    let upstream = Upstream::with_dns(config, Arc::new(dns_at(server))).unwrap();
    router_with(ApiState {
        upstream: Arc::new(upstream),
        ..api_state(store_with_account())
    })
}

async fn listed(router: &Router, cookie: &str) -> usize {
    let response = router
        .clone()
        .oneshot(with_cookie(Method::GET, ROUTE, cookie))
        .await
        .unwrap();
    let body: Value = serde_json::from_str(&body_text(response).await).unwrap();
    body["accounts"].as_array().unwrap().len()
}

async fn refused(server: &TlsServer, config: &UpstreamConfig, url: &str) -> (StatusCode, String) {
    let router = router_against(server, config);
    let cookie = sign_in(&router).await;
    let answer = post(&router, &cookie, ROUTE, &jmap_body(url, PASSWORD)).await;
    assert_eq!(listed(&router, &cookie).await, 0);
    answer
}

#[tokio::test]
async fn adding_needs_a_session_and_the_header() {
    let router = router_on(store_with_account());
    let (status, _) = post(&router, "huliho_session=stale", ROUTE, &imap_body()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let cookie = sign_in(&router).await;
    let mut request = json_request(&cookie, ROUTE, &imap_body());
    request.headers_mut().remove("x-requested-with");
    let response = router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_malformed_request_is_refused_before_anything_connects() {
    let server = TlsServer::start(routes(true)).await;
    let router = router_against(&server, &server.config(true));
    let cookie = sign_in(&router).await;
    let url = session_url(server.address.port());
    let literal = format!("https://127.0.0.1:{}/jmap/session", server.address.port());
    let cases = [
        (
            "a malformed address",
            edited(jmap_body(&url, PASSWORD), |body| {
                body["address"] = json!("sanne");
            }),
        ),
        (
            "a plain session url",
            jmap_body(&url.replace("https", "http"), PASSWORD),
        ),
        ("an address literal", jmap_body(&literal, PASSWORD)),
        (
            "userinfo in the url",
            jmap_body(&url.replace("https://", "https://sanne:secret@"), PASSWORD),
        ),
        ("an empty password", jmap_body(&url, "")),
        (
            "a control character in the password",
            jmap_body(&url, "pass\nword"),
        ),
        (
            "a name longer than allowed",
            edited(jmap_body(&url, PASSWORD), |body| {
                body["name"] = json!("x".repeat(101));
            }),
        ),
        (
            "an address literal imap host",
            edited(imap_body(), |body| {
                body["target"]["imap"]["host"] = json!("127.0.0.1");
            }),
        ),
        (
            "a port of zero",
            edited(imap_body(), |body| {
                body["target"]["smtp"]["port"] = json!(0);
            }),
        ),
        (
            "an empty username",
            edited(imap_body(), |body| body["target"]["username"] = json!("")),
        ),
        (
            "a token on an imap target",
            edited(imap_body(), |body| {
                body["credential"] = json!({ "kind": "bearer", "token": "t" });
            }),
        ),
    ];
    for (label, body) in &cases {
        let (status, text) = post(&router, &cookie, ROUTE, body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{label}: {text}");
        assert!(text.contains("invalid_request"), "{label}: {text}");
    }
    assert!(server.requests().is_empty());
    assert_eq!(listed(&router, &cookie).await, 0);
}

#[tokio::test]
async fn a_body_of_the_wrong_shape_is_unprocessable() {
    let router = router_on(store_with_account());
    let cookie = sign_in(&router).await;
    for body in [
        edited(imap_body(), |body| body["provider"] = json!("aol")),
        edited(imap_body(), |body| {
            body["target"]["imap"]["tls"] = json!("plain");
        }),
    ] {
        let (status, _) = post(&router, &cookie, ROUTE, &body).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }
}

/// Discovery and connect share the limiter keys: four attempts pass,
/// the fifth waits and so does a sign-in from the same address.
#[tokio::test]
async fn the_fifth_attempt_in_a_row_is_rate_limited_across_discovery_and_connect() {
    let router = router_on(store_with_account());
    let cookie = sign_in(&router).await;
    for _ in 0..4 {
        let (status, _) = post(
            &router,
            &cookie,
            DISCOVER,
            &json!({ "address": "sanne@gmail.com" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }
    let response = router
        .clone()
        .oneshot(json_request(&cookie, ROUTE, &imap_body()))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(response.headers().contains_key(header::RETRY_AFTER));
    assert!(body_text(response).await.contains("rate_limited"));
    let sign_in = router
        .clone()
        .oneshot(login_request(LOGIN, LOGIN_PASSWORD))
        .await
        .unwrap();
    assert_eq!(sign_in.status(), StatusCode::TOO_MANY_REQUESTS);
}

/// Three refusals and a pass leave the run clear, so the discovery that
/// follows is not the fifth attempt.
#[tokio::test]
async fn a_passing_connect_clears_the_run() {
    let server = TlsServer::start(routes(true)).await;
    let router = router_against(&server, &server.config(true));
    let cookie = sign_in(&router).await;
    let url = session_url(server.address.port());
    for _ in 0..3 {
        let (status, _) = post(&router, &cookie, ROUTE, &jmap_body(&url, "wrong horse")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
    let (status, _) = post(&router, &cookie, ROUTE, &jmap_body(&url, PASSWORD)).await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = post(
        &router,
        &cookie,
        DISCOVER,
        &json!({ "address": "sanne@gmail.com" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn a_rejected_password_is_upstream_credentials_and_stores_nothing() {
    let server = TlsServer::start(routes(true)).await;
    let router = router_against(&server, &server.config(true));
    let cookie = sign_in(&router).await;
    let url = session_url(server.address.port());
    let (status, body) = post(&router, &cookie, ROUTE, &jmap_body(&url, "wrong horse")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert!(body.contains("upstream_credentials"));
    assert!(!body.contains("wrong horse"));
    assert_eq!(listed(&router, &cookie).await, 0);
    assert_eq!(server.requests().len(), 1);
}

#[tokio::test]
async fn a_session_resource_without_the_mail_capability_is_unsupported() {
    let server = TlsServer::start(routes(false)).await;
    let url = session_url(server.address.port());
    let (status, body) = refused(&server, &server.config(true), &url).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("upstream_unsupported"));
}

#[tokio::test]
async fn a_page_in_place_of_the_session_resource_is_unsupported() {
    let server = TlsServer::start(routes(true)).await;
    let url = format!("https://{HOST}:{}/page", server.address.port());
    let (status, body) = refused(&server, &server.config(true), &url).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("upstream_unsupported"));
}

#[tokio::test]
async fn a_certificate_outside_the_roots_is_insecure() {
    let server = TlsServer::start(routes(true)).await;
    let distrusting = UpstreamConfig {
        additional_ca_file: None,
        ..server.config(true)
    };
    let url = session_url(server.address.port());
    let (status, body) = refused(&server, &distrusting, &url).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("upstream_insecure"));
    assert!(server.requests().is_empty());
}

#[tokio::test]
async fn a_closed_port_is_unreachable() {
    let server = TlsServer::start(routes(true)).await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let closed = listener.local_addr().unwrap().port();
    drop(listener);
    let (status, body) = refused(&server, &server.config(true), &session_url(closed)).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert!(body.contains("upstream_unreachable"));
}

#[tokio::test]
async fn a_private_address_is_refused_before_the_connect() {
    let server = TlsServer::start(routes(true)).await;
    let url = session_url(server.address.port());
    let (status, body) = refused(&server, &server.config(false), &url).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert!(body.contains("upstream_unreachable"));
    assert!(server.requests().is_empty());
}
