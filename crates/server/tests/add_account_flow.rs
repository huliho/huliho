// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Adding an account end to end: the row, its sealed credential, the
//! event, the IMAP path through the bridge and the log lines.

mod common;
mod fake_dns;
mod jmap_session;
mod log_capture;
mod signin;
mod tls_server;

use std::io::Write;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use common::{api_state, router_on, router_with};
use fake_dns::FakeDns;
use huliho_imap_bridge::testing::imap::{self, FakeImap};
use huliho_imap_bridge::testing::smtp::{self, FakeSmtp};
use huliho_imap_bridge::testing::{PASSWORD, TOKEN, USER};
use huliho_server::accounts::{self, Credential};
use huliho_server::api::ApiState;
use huliho_server::auth::{self, LoginOutcome};
use huliho_server::config::UpstreamConfig;
use huliho_server::ids::{AccountId, UserId};
use huliho_server::store::Store;
use huliho_server::upstream::Upstream;
use huliho_server::{events, identity, scope};
use jmap_session::{ADDRESS, HOST, routes, session_url};
use log_capture::Capture;
use serde_json::{Value, json};
use signin::{
    LOGIN, PASSWORD as LOGIN_PASSWORD, body_text, login_request, sign_in, store_with_account,
    with_cookie,
};
use tempfile::NamedTempFile;
use tls_server::TlsServer;
use tower::ServiceExt;

const ROUTE: &str = "/api/accounts";
const OTHER_LOGIN: &str = "noor@example.com";

fn json_request(cookie: &str, body: &Value) -> Request<Body> {
    let mut request = with_cookie(Method::POST, ROUTE, cookie);
    request
        .headers_mut()
        .insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
    *request.body_mut() = Body::from(body.to_string());
    request
}

async fn add(router: &Router, cookie: &str, body: &Value) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(json_request(cookie, body))
        .await
        .unwrap();
    let status = response.status();
    let text = body_text(response).await;
    (
        status,
        serde_json::from_str(&text).unwrap_or(Value::String(text)),
    )
}

async fn list(router: &Router, cookie: &str) -> Value {
    let response = router
        .clone()
        .oneshot(with_cookie(Method::GET, ROUTE, cookie))
        .await
        .unwrap();
    serde_json::from_str(&body_text(response).await).unwrap()
}

fn jmap_body(session_url: &str, provider: &str, credential: &Value, name: Option<&str>) -> Value {
    let mut body = json!({
        "address": ADDRESS,
        "provider": provider,
        "target": { "kind": "jmap", "sessionUrl": session_url },
        "credential": credential,
    });
    if let Some(name) = name {
        body["name"] = json!(name);
    }
    body
}

fn password(password: &str) -> Value {
    json!({ "kind": "password", "password": password })
}

fn imap_body(imap: &FakeImap, smtp: &FakeSmtp) -> Value {
    json!({
        "address": ADDRESS,
        "provider": "generic",
        "target": {
            "kind": "imap",
            "username": USER,
            "imap": { "host": imap::HOST, "port": imap.address.port(), "tls": "implicit" },
            "smtp": { "host": smtp::HOST, "port": smtp.address.port(), "tls": "implicit" },
        },
        "credential": password(PASSWORD),
    })
}

/// The router with the store and the keys behind it, so a test can open
/// what the router sealed.
struct Instance {
    store: Arc<Store>,
    api: ApiState,
    router: Router,
}

impl Instance {
    /// Against the JMAP test server.
    fn jmap(server: &TlsServer) -> Self {
        let mut dns = FakeDns::default();
        dns.addresses.insert(
            HOST.to_owned(),
            vec![SocketAddr::new(server.address.ip(), 0)],
        );
        Self::on(Upstream::with_dns(&server.config(true), Arc::new(dns)).unwrap())
    }

    /// Against the scripted IMAP and SMTP servers, both trusted through
    /// one CA file.
    fn bridge(imap: &FakeImap, smtp: &FakeSmtp) -> Self {
        let mut ca_file = NamedTempFile::new().unwrap();
        write!(ca_file, "{}{}", imap.ca_pem(), smtp.ca_pem()).unwrap();
        let config = UpstreamConfig {
            allow_private_networks: vec!["127.0.0.0/8".parse().unwrap()],
            additional_ca_file: Some(ca_file.path().to_owned()),
            ..UpstreamConfig::default()
        };
        let mut dns = FakeDns::default();
        dns.addresses.insert(
            imap::HOST.to_owned(),
            vec![SocketAddr::new(imap.address.ip(), 0)],
        );
        dns.addresses.insert(
            smtp::HOST.to_owned(),
            vec![SocketAddr::new(smtp.address.ip(), 0)],
        );
        Self::on(Upstream::with_dns(&config, Arc::new(dns)).unwrap())
    }

    fn on(upstream: Upstream) -> Self {
        let store = store_with_account();
        let api = ApiState {
            upstream: Arc::new(upstream),
            ..api_state(Arc::clone(&store))
        };
        let router = router_with(api.clone());
        Self { store, api, router }
    }

    fn user_id(&self, login: &str) -> UserId {
        match auth::verify_login(&self.store, login, LOGIN_PASSWORD).unwrap() {
            LoginOutcome::Verified(id) => id,
            _ => panic!("the fixture user signs in"),
        }
    }

    /// The credential the router sealed on the row, opened with its
    /// keys.
    fn credential(&self, id: &str) -> Credential {
        let account = AccountId::from(id.to_owned());
        let scope = scope::resolve(&self.store, &self.user_id(LOGIN), Some(&account)).unwrap();
        accounts::credential(&self.store, &self.api.keys, &scope).unwrap()
    }

    fn event_types(&self) -> Vec<String> {
        let scope = scope::resolve(&self.store, &self.user_id(LOGIN), None).unwrap();
        events::for_organization(&self.store, &scope)
            .unwrap()
            .into_iter()
            .map(|record| record.event_type)
            .collect()
    }
}

#[tokio::test]
async fn a_jmap_account_adds_lists_and_seals_its_password() {
    let server = TlsServer::start(routes(true)).await;
    let instance = Instance::jmap(&server);
    let cookie = sign_in(&instance.router).await;
    let url = session_url(server.address.port());
    let (status, row) = add(
        &instance.router,
        &cookie,
        &jmap_body(&url, "generic", &password(PASSWORD), None),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{row}");
    assert_eq!(row["address"], ADDRESS);
    assert_eq!(row["name"], "example.test");
    assert_eq!(row["provider"], "generic");
    assert_eq!(row["kind"], "jmap");
    assert_eq!(row["authMethod"], "password");
    assert!(row["stoppedCause"].is_null());
    assert!(row["createdAt"].as_i64().unwrap() > 0);
    let text = row.to_string();
    assert!(!text.contains(PASSWORD) && !text.contains("sessionUrl"));
    let id = row["id"].as_str().unwrap();
    let listed = list(&instance.router, &cookie).await;
    assert_eq!(listed["accounts"].as_array().unwrap().len(), 1);
    assert_eq!(listed["accounts"][0]["id"], id);
    assert_eq!(
        instance.credential(id),
        Credential::Password {
            password: PASSWORD.to_owned()
        }
    );
    assert!(
        instance
            .event_types()
            .contains(&"account.linked".to_owned())
    );
    assert!(
        server
            .requests()
            .iter()
            .any(|line| line.ends_with("/jmap/session"))
    );
}

#[tokio::test]
async fn a_token_signs_in_over_jmap_and_the_name_follows_the_preset_or_the_user() {
    let server = TlsServer::start(routes(true)).await;
    let instance = Instance::jmap(&server);
    let cookie = sign_in(&instance.router).await;
    let url = session_url(server.address.port());
    let bearer = json!({ "kind": "bearer", "token": TOKEN });
    let (status, row) = add(
        &instance.router,
        &cookie,
        &jmap_body(&url, "fastmail", &bearer, None),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{row}");
    assert_eq!(row["name"], "Fastmail");
    assert_eq!(row["provider"], "fastmail");
    assert_eq!(row["authMethod"], "bearer");
    assert_eq!(
        instance.credential(row["id"].as_str().unwrap()),
        Credential::Bearer {
            token: TOKEN.to_owned()
        }
    );
    let (status, row) = add(
        &instance.router,
        &cookie,
        &jmap_body(&url, "fastmail", &bearer, Some("  Work  ")),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{row}");
    assert_eq!(row["name"], "Work");
    assert_eq!(
        list(&instance.router, &cookie).await["accounts"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn an_imap_account_passes_imap_and_smtp_and_lists() {
    let imap = FakeImap::start(imap::Script::tls()).await;
    let smtp = FakeSmtp::start(smtp::Script::tls()).await;
    let instance = Instance::bridge(&imap, &smtp);
    let cookie = sign_in(&instance.router).await;
    let (status, row) = add(&instance.router, &cookie, &imap_body(&imap, &smtp)).await;
    assert_eq!(status, StatusCode::CREATED, "{row}");
    assert_eq!(row["kind"], "imap");
    assert_eq!(row["authMethod"], "password");
    assert!(imap.lines().iter().any(|line| line.contains("LOGIN")));
    assert!(smtp.lines().iter().any(|line| line.contains("AUTH PLAIN")));
    assert_eq!(
        instance.credential(row["id"].as_str().unwrap()),
        Credential::Password {
            password: PASSWORD.to_owned()
        }
    );
    assert_eq!(
        list(&instance.router, &cookie).await["accounts"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn a_submission_server_without_auth_is_smtp_auth_unavailable_and_stores_nothing() {
    let imap = FakeImap::start(imap::Script::tls()).await;
    let smtp = FakeSmtp::start(smtp::Script {
        mechanisms: "",
        ..smtp::Script::tls()
    })
    .await;
    let instance = Instance::bridge(&imap, &smtp);
    let cookie = sign_in(&instance.router).await;
    let (status, body) = add(&instance.router, &cookie, &imap_body(&imap, &smtp)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "smtp_auth_unavailable");
    assert!(imap.lines().iter().any(|line| line.contains("LOGIN")));
    assert!(
        list(&instance.router, &cookie).await["accounts"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn the_added_row_stays_inside_its_user() {
    let server = TlsServer::start(routes(true)).await;
    let instance = Instance::jmap(&server);
    let cookie = sign_in(&instance.router).await;
    let url = session_url(server.address.port());
    let (status, _) = add(
        &instance.router,
        &cookie,
        &jmap_body(&url, "generic", &password(PASSWORD), None),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (_, other) = identity::create_personal_user(&instance.store, OTHER_LOGIN).unwrap();
    auth::set_password(&instance.store, &other.id, LOGIN_PASSWORD).unwrap();
    let plain = router_on(Arc::clone(&instance.store));
    let response = plain
        .clone()
        .oneshot(login_request(OTHER_LOGIN, LOGIN_PASSWORD))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let other_cookie = signin::cookie_of(&response);
    assert!(
        list(&plain, &other_cookie).await["accounts"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn no_log_line_carries_the_address_the_password_or_the_token() {
    let capture = Capture::install();
    let server = TlsServer::start(routes(true)).await;
    let instance = Instance::jmap(&server);
    let cookie = sign_in(&instance.router).await;
    let url = session_url(server.address.port());
    let (status, _) = add(
        &instance.router,
        &cookie,
        &jmap_body(&url, "generic", &password("wrong horse"), None),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = add(
        &instance.router,
        &cookie,
        &jmap_body(&url, "generic", &password(PASSWORD), None),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let bearer = json!({ "kind": "bearer", "token": TOKEN });
    let (status, _) = add(
        &instance.router,
        &cookie,
        &jmap_body(&url, "fastmail", &bearer, None),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let text = capture.text();
    assert!(text.contains("credential check failed"), "{text}");
    for secret in ["wrong horse", PASSWORD, TOKEN, ADDRESS, HOST] {
        assert!(!text.contains(secret), "{secret} in {text}");
    }
}
