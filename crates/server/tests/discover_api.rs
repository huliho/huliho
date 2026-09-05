// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Discovery over HTTP: the guards, the answer shapes and the rate limit.

mod common;
mod fake_dns;
mod signin;
mod tls_server;

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use axum::response::Redirect;
use axum::routing::get;
use common::{api_state, router_on, router_with};
use fake_dns::FakeDns;
use huliho_server::api::ApiState;
use huliho_server::store::Store;
use huliho_server::upstream::{SrvTarget, Upstream};
use huliho_server::{auth, identity};
use serde::Deserialize;
use signin::{LOGIN, PASSWORD, body_text, login_request, sign_in, store_with_account, with_cookie};
use tls_server::TlsServer;
use tower::ServiceExt;
use url::Url;

const ROUTE: &str = "/api/accounts/discover";

/// The answer as the client reads it.
#[derive(Debug, Deserialize)]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum Answer {
    Found {
        provider: String,
        kind: String,
        target: serde_json::Value,
        credential_kind: String,
        host: String,
        oauth_available: bool,
    },
    NotFound,
}

fn discover_request(cookie: &str, address: &str) -> Request<Body> {
    let mut request = with_cookie(Method::POST, ROUTE, cookie);
    request
        .headers_mut()
        .insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
    *request.body_mut() = Body::from(serde_json::json!({ "address": address }).to_string());
    request
}

async fn discover(router: &Router, cookie: &str, address: &str) -> (StatusCode, String) {
    let response = router
        .clone()
        .oneshot(discover_request(cookie, address))
        .await
        .unwrap();
    let status = response.status();
    (status, body_text(response).await)
}

async fn answer(router: &Router, cookie: &str, address: &str) -> Answer {
    let (status, body) = discover(router, cookie, address).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    serde_json::from_str(&body).unwrap()
}

/// The fixture user on a database file, so a second connection can put a
/// provider row in.
fn store_on_disk() -> (tempfile::TempDir, Arc<Store>, rusqlite::Connection) {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let store = Store::open(&data).unwrap();
    let (_, user) = identity::create_personal_user(&store, LOGIN).unwrap();
    auth::set_password(&store, &user.id, PASSWORD).unwrap();
    let database = rusqlite::Connection::open(data.join("huliho.db")).unwrap();
    (dir, Arc::new(store), database)
}

fn register_google(database: &rusqlite::Connection) {
    database
        .execute(
            "INSERT INTO auth_providers (id, issuer, discovery_url, client_id, created_at)
             VALUES ('google', 'https://accounts.google.com',
                     'https://accounts.google.com/.well-known/openid-configuration',
                     'client-id', 0)",
            [],
        )
        .unwrap();
}

/// A router whose chain runs against the fake resolver and the test
/// server.
fn router_against(server: &TlsServer, dns: FakeDns) -> Router {
    let upstream = Upstream::with_dns(&server.config(true), Arc::new(dns)).unwrap();
    router_with(ApiState {
        upstream: Arc::new(upstream),
        ..api_state(store_with_account())
    })
}

fn challenge() -> (
    StatusCode,
    [(header::HeaderName, &'static str); 1],
    &'static str,
) {
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, "Basic realm=\"test\"")],
        "",
    )
}

#[tokio::test]
async fn discovery_needs_a_session_and_the_header() {
    let router = router_on(store_with_account());
    let (status, _) = discover(&router, "huliho_session=stale", "sanne@gmail.com").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let cookie = sign_in(&router).await;
    let mut request = discover_request(&cookie, "sanne@gmail.com");
    request.headers_mut().remove("x-requested-with");
    let response = router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_malformed_address_is_a_bad_request() {
    let router = router_on(store_with_account());
    let cookie = sign_in(&router).await;
    for address in [
        "sanne",
        "sanne@",
        "@example.test",
        "sanne@localhost",
        "sanne@127.0.0.1",
        "sanne@exa mple.test",
    ] {
        let (status, body) = discover(&router, &cookie, address).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{address}");
        assert!(body.contains("invalid_request"));
    }
}

#[tokio::test]
async fn a_well_known_domain_answers_its_preset() {
    let router = router_on(store_with_account());
    let cookie = sign_in(&router).await;
    match answer(&router, &cookie, "sanne@gmail.com").await {
        Answer::Found {
            provider,
            kind,
            target,
            credential_kind,
            host,
            oauth_available,
        } => {
            assert_eq!((provider.as_str(), kind.as_str()), ("gmail", "imap"));
            assert_eq!(credential_kind, "appPassword");
            assert_eq!(host, "imap.gmail.com");
            assert!(!oauth_available);
            assert_eq!(target["kind"], "imap");
            assert_eq!(target["username"], "sanne@gmail.com");
            assert_eq!(target["imap"]["host"], "imap.gmail.com");
            assert_eq!(target["imap"]["port"], 993);
            assert_eq!(target["imap"]["tls"], "implicit");
            assert_eq!(target["smtp"]["port"], 465);
        }
        Answer::NotFound => panic!("nothing found"),
    }
    match answer(&router, &cookie, "mira@fastmail.com").await {
        Answer::Found {
            provider,
            kind,
            target,
            credential_kind,
            host,
            ..
        } => {
            assert_eq!((provider.as_str(), kind.as_str()), ("fastmail", "jmap"));
            assert_eq!(credential_kind, "apiToken");
            assert_eq!(host, "api.fastmail.com");
            assert_eq!(
                target["sessionUrl"],
                "https://api.fastmail.com/jmap/session"
            );
        }
        Answer::NotFound => panic!("nothing found"),
    }
    match answer(&router, &cookie, "noor@outlook.com").await {
        Answer::Found {
            credential_kind,
            oauth_available,
            ..
        } => {
            assert_eq!(credential_kind, "oauth");
            assert!(!oauth_available);
        }
        Answer::NotFound => panic!("nothing found"),
    }
}

#[tokio::test]
async fn oauth_is_available_with_a_provider_row_and_a_public_url() {
    let (_dir, store, database) = store_on_disk();
    register_google(&database);
    let public_url: Url = "https://mail.example.test".parse().unwrap();
    let router = router_with(ApiState {
        public_url: Some(public_url),
        ..api_state(Arc::clone(&store))
    });
    let cookie = sign_in(&router).await;
    let available = |answer: Answer| match answer {
        Answer::Found {
            oauth_available, ..
        } => oauth_available,
        Answer::NotFound => panic!("nothing found"),
    };
    assert!(available(answer(&router, &cookie, "sanne@gmail.com").await));
    assert!(!available(
        answer(&router, &cookie, "noor@outlook.com").await
    ));
    let without_url = router_with(api_state(store));
    let cookie = sign_in(&without_url).await;
    assert!(!available(
        answer(&without_url, &cookie, "sanne@gmail.com").await
    ));
}

#[tokio::test]
async fn a_chain_hit_reaches_the_route_and_its_requests_carry_no_address() {
    let app = Router::new()
        .route(
            "/.well-known/jmap",
            get(|| async { Redirect::permanent("/jmap/session") }),
        )
        .route("/jmap/session", get(|| async { challenge() }));
    let server = TlsServer::start(app).await;
    let mut dns = FakeDns::default();
    dns.addresses
        .insert("jmap.example.test".to_owned(), vec![server.address]);
    dns.srv.insert(
        "_jmap._tcp.example.test".to_owned(),
        vec![SrvTarget {
            priority: 0,
            weight: 0,
            port: server.address.port(),
            host: Some("jmap.example.test".to_owned()),
        }],
    );
    let router = router_against(&server, dns);
    let cookie = sign_in(&router).await;
    match answer(&router, &cookie, "sanne@example.test").await {
        Answer::Found {
            provider,
            kind,
            credential_kind,
            host,
            ..
        } => {
            assert_eq!((provider.as_str(), kind.as_str()), ("generic", "jmap"));
            assert_eq!(credential_kind, "password");
            assert_eq!(host, "jmap.example.test");
        }
        Answer::NotFound => panic!("nothing found"),
    }
    let requests = server.requests();
    assert!(!requests.is_empty());
    assert!(requests.iter().all(|line| !line.contains("sanne")));
}

#[tokio::test]
async fn an_unknown_domain_answers_not_found() {
    let server = TlsServer::start(Router::new()).await;
    let router = router_against(&server, FakeDns::default());
    let cookie = sign_in(&router).await;
    assert!(matches!(
        answer(&router, &cookie, "sanne@nowhere.test").await,
        Answer::NotFound
    ));
    assert!(server.requests().is_empty());
}

/// The free run of the sign-in limiter is three failures; the fourth
/// attempt still passes and starts the block the fifth runs into.
#[tokio::test]
async fn the_fifth_discovery_in_a_row_is_rate_limited_and_so_is_a_sign_in() {
    let router = router_on(store_with_account());
    let cookie = sign_in(&router).await;
    for _ in 0..4 {
        let (status, _) = discover(&router, &cookie, "sanne@gmail.com").await;
        assert_eq!(status, StatusCode::OK);
    }
    let response = router
        .clone()
        .oneshot(discover_request(&cookie, "sanne@gmail.com"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    let retry_after: u64 = response.headers()[header::RETRY_AFTER]
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!(retry_after >= 1);
    assert!(body_text(response).await.contains("rate_limited"));
    let sign_in = router
        .clone()
        .oneshot(login_request(LOGIN, PASSWORD))
        .await
        .unwrap();
    assert_eq!(sign_in.status(), StatusCode::TOO_MANY_REQUESTS);
}
