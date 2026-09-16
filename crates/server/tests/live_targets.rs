// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Discovery against the compose Cyrus and the public internet.

#![cfg(feature = "live-targets")]

mod common;
mod fake_dns;
mod signin;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, StatusCode, header};
use common::{api_state, router_on, router_with};
use fake_dns::FakeDns;
use huliho_server::accounts::{AccountSettings, Provider};
use huliho_server::api::ApiState;
use huliho_server::config::UpstreamConfig;
use huliho_server::discovery::{self, Address, Budget, Discovered};
use huliho_server::gate::{MAX_REFUSED_RUN, Reconnect};
use huliho_server::jmap::{JMAP_REQUEST_LIMIT, MAX_CONCURRENT_REQUESTS};
use huliho_server::upstream::{Dns, SrvTarget, Upstream};
use serde_json::{Value, json};
use signin::{
    LOGIN, PASSWORD as LOGIN_PASSWORD, body_text, login_request, sign_in, store_with_account,
    with_cookie,
};
use tower::ServiceExt;
use tracing_subscriber::EnvFilter;

/// The compose Cyrus: JMAP over TLS on the host's loopback.
const CYRUS_HOST: &str = "localhost";
const CYRUS_PORT: u16 = 8443;

/// The compose Dovecot on the same loopback: IMAPS and submission.
const IMAPS_PORT: u16 = 31993;
const SUBMISSION_PORT: u16 = 31587;

const CORE: &str = "urn:ietf:params:jmap:core";
const MAIL: &str = "urn:ietf:params:jmap:mail";

const MAIL_ADDRESS: &str = "sanne@huliho.test";
const MAIL_PASSWORD: &str = "password";
const ROUTE: &str = "/api/accounts";

/// Two tests reach the compose Dovecot and one of them pauses it, so
/// they take turns.
static DOVECOT: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn compose_file() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docker-compose.dev.yml")
}

/// `docker compose <verb> dovecot` against the dev file; whether it
/// succeeded.
fn compose(verb: &str) -> bool {
    Command::new("docker")
        .arg("compose")
        .arg("-f")
        .arg(compose_file())
        .arg(verb)
        .arg("dovecot")
        .status()
        .is_ok_and(|status| status.success())
}

/// Resumes Dovecot on the way out, however the test ended. No assert
/// here: a panic inside a drop during an unwind aborts the test binary.
struct Paused;

impl Drop for Paused {
    fn drop(&mut self) {
        compose("unpause");
    }
}

fn logging() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new("huliho_server=debug"))
        .with_test_writer()
        .try_init();
}

fn dev_ca() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/dev-certs/ca.pem")
}

fn compose_config(allow_loopback: bool, trust_dev_ca: bool) -> UpstreamConfig {
    let allow_private_networks = if allow_loopback {
        vec!["127.0.0.0/8".parse().unwrap(), "::1/128".parse().unwrap()]
    } else {
        Vec::new()
    };
    UpstreamConfig {
        allow_private_networks,
        additional_ca_file: trust_dev_ca.then(dev_ca),
        ..UpstreamConfig::default()
    }
}

/// `huliho.test` has no public records; its `_jmap._tcp` names Cyrus.
fn cyrus_dns() -> Arc<dyn Dns> {
    let mut dns = FakeDns::default();
    dns.addresses.insert(
        CYRUS_HOST.to_owned(),
        vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)],
    );
    dns.srv.insert(
        "_jmap._tcp.huliho.test".to_owned(),
        vec![SrvTarget {
            priority: 0,
            weight: 0,
            port: CYRUS_PORT,
            host: Some(CYRUS_HOST.to_owned()),
        }],
    );
    Arc::new(dns)
}

async fn discover(upstream: &Upstream, address: &str) -> Option<Discovered> {
    logging();
    discovery::discover(
        upstream,
        &Address::parse(address).unwrap(),
        Budget::default(),
    )
    .await
}

fn session_url_of(found: &Discovered) -> String {
    match &found.target {
        AccountSettings::Jmap { session_url } => session_url.to_string(),
        AccountSettings::Imap { .. } => panic!("not a JMAP target: {:?}", found.target),
    }
}

#[tokio::test]
async fn cyrus_is_found_through_its_srv_record_with_the_dev_ca() {
    let upstream = Upstream::with_dns(&compose_config(true, true), cyrus_dns()).unwrap();
    let found = discover(&upstream, "sanne@huliho.test").await.unwrap();
    assert_eq!(
        session_url_of(&found),
        format!("https://{CYRUS_HOST}:{CYRUS_PORT}/jmap")
    );
    assert_eq!(found.provider, Provider::Generic);
    assert_eq!(found.host(), CYRUS_HOST);
}

#[tokio::test]
async fn cyrus_is_refused_without_the_private_network_rule() {
    let upstream = Upstream::with_dns(&compose_config(false, true), cyrus_dns()).unwrap();
    assert_eq!(discover(&upstream, "sanne@huliho.test").await, None);
}

#[tokio::test]
async fn cyrus_is_refused_without_the_dev_ca() {
    let upstream = Upstream::with_dns(&compose_config(true, false), cyrus_dns()).unwrap();
    assert_eq!(discover(&upstream, "sanne@huliho.test").await, None);
}

#[tokio::test]
async fn fastmail_fm_is_found_through_the_chain_live() {
    let upstream = Upstream::new(&UpstreamConfig::default()).unwrap();
    let found = discover(&upstream, "mira@fastmail.fm").await.unwrap();
    assert_eq!(
        session_url_of(&found),
        "https://api.fastmail.com/jmap/session"
    );
    assert_eq!(found.provider, Provider::Fastmail);
}

#[tokio::test]
async fn gmail_com_is_decided_without_a_lookup() {
    let upstream = Upstream::new(&UpstreamConfig::default()).unwrap();
    let found = discover(&upstream, "sanne@gmail.com").await.unwrap();
    assert_eq!(found.provider, Provider::Gmail);
    assert_eq!(found.host(), "imap.gmail.com");
}

#[tokio::test]
async fn google_com_is_found_through_the_chain_live() {
    let upstream = Upstream::new(&UpstreamConfig::default()).unwrap();
    let found = discover(&upstream, "someone@google.com").await.unwrap();
    assert_eq!(found.provider, Provider::Gmail);
    assert_eq!(found.host(), "imap.gmail.com");
}

fn cyrus_body(password: &str) -> Value {
    let session_url = format!("https://{CYRUS_HOST}:{CYRUS_PORT}/jmap");
    json!({
        "address": MAIL_ADDRESS,
        "provider": "generic",
        "target": { "kind": "jmap", "sessionUrl": session_url },
        "credential": { "kind": "password", "password": password },
    })
}

fn dovecot_body(password: &str) -> Value {
    json!({
        "address": MAIL_ADDRESS,
        "provider": "generic",
        "target": {
            "kind": "imap",
            "username": MAIL_ADDRESS,
            "imap": { "host": CYRUS_HOST, "port": IMAPS_PORT, "tls": "implicit" },
            "smtp": { "host": CYRUS_HOST, "port": SUBMISSION_PORT, "tls": "starttls" },
        },
        "credential": { "kind": "password", "password": password },
    })
}

async fn add(router: &Router, cookie: &str, body: &Value) -> (StatusCode, String) {
    let mut request = with_cookie(Method::POST, ROUTE, cookie);
    request
        .headers_mut()
        .insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
    *request.body_mut() = Body::from(body.to_string());
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    (status, body_text(response).await)
}

/// The router on the compose rules: the dev CA trusted, the loopback
/// allowed, `localhost` answered by the fake resolver; the state behind
/// it for the probe.
fn compose_router() -> (Router, ApiState) {
    let upstream = Upstream::with_dns(&compose_config(true, true), cyrus_dns()).unwrap();
    let api = ApiState {
        upstream: Arc::new(upstream),
        ..api_state(store_with_account())
    };
    (router_with(api.clone()), api)
}

async fn retry(router: &Router, cookie: &str, id: &str) -> (StatusCode, String) {
    let request = with_cookie(Method::POST, &format!("{ROUTE}/{id}/retry"), cookie);
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    (status, body_text(response).await)
}

async fn listed(router: &Router, cookie: &str) -> Value {
    let response = router
        .clone()
        .oneshot(with_cookie(Method::GET, ROUTE, cookie))
        .await
        .unwrap();
    serde_json::from_str(&body_text(response).await).unwrap()
}

/// One request through the proxy: the JSON body on the account's
/// endpoint with the header the router demands.
async fn proxied(router: &Router, cookie: &str, id: &str, body: &Value) -> (StatusCode, Value) {
    let mut request = with_cookie(Method::POST, &format!("/api/jmap/{id}"), cookie);
    request
        .headers_mut()
        .insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
    *request.body_mut() = Body::from(body.to_string());
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let text = body_text(response).await;
    (
        status,
        serde_json::from_str(&text).unwrap_or(Value::String(text)),
    )
}

#[tokio::test]
async fn cyrus_and_dovecot_accounts_add_through_the_router_and_list() {
    logging();
    let _dovecot = DOVECOT.lock().await;
    let (router, _) = compose_router();
    let cookie = sign_in(&router).await;
    for body in [cyrus_body("wrong"), dovecot_body("wrong")] {
        let (status, text) = add(&router, &cookie, &body).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{text}");
        assert!(text.contains("upstream_credentials"));
    }
    for body in [cyrus_body(MAIL_PASSWORD), dovecot_body(MAIL_PASSWORD)] {
        let (status, text) = add(&router, &cookie, &body).await;
        assert_eq!(status, StatusCode::CREATED, "{text}");
    }
    let rows = listed(&router, &cookie).await;
    let kinds: Vec<&str> = rows["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["jmap", "imap"]);
}

#[tokio::test]
async fn a_paused_dovecot_stops_the_account_and_the_probe_resumes_it() {
    logging();
    let _dovecot = DOVECOT.lock().await;
    let (router, api) = compose_router();
    let cookie = sign_in(&router).await;
    let (status, text) = add(&router, &cookie, &dovecot_body(MAIL_PASSWORD)).await;
    assert_eq!(status, StatusCode::CREATED, "{text}");
    let row: Value = serde_json::from_str(&text).unwrap();
    let id = row["id"].as_str().unwrap().to_owned();
    assert!(compose("pause"));
    let paused = Paused;
    for attempt in 1..=MAX_REFUSED_RUN {
        let (status, text) = retry(&router, &cookie, &id).await;
        if attempt < MAX_REFUSED_RUN {
            assert_eq!(status, StatusCode::BAD_GATEWAY, "{text}");
        } else {
            assert_eq!(status, StatusCode::CONFLICT, "{text}");
            assert!(text.contains("\"cause\":\"connection\""), "{text}");
        }
    }
    let reconnect = Reconnect::from(&api);
    assert_eq!(reconnect.probe_once().await, 0);
    drop(paused);
    assert_eq!(reconnect.probe_once().await, 1);
    let rows = listed(&router, &cookie).await;
    let row = rows["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == id)
        .unwrap();
    assert!(row["stoppedCause"].is_null(), "{row}");
}

#[tokio::test]
async fn the_proxy_answers_the_cyrus_session_object_and_one_query_round_trip() {
    logging();
    let (router, _) = compose_router();
    let cookie = sign_in(&router).await;
    let (status, text) = add(&router, &cookie, &cyrus_body(MAIL_PASSWORD)).await;
    assert_eq!(status, StatusCode::CREATED, "{text}");
    let row: Value = serde_json::from_str(&text).unwrap();
    let id = row["id"].as_str().unwrap().to_owned();
    let response = router
        .clone()
        .oneshot(with_cookie(
            Method::GET,
            &format!("/api/jmap/{id}/session"),
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let session: Value = serde_json::from_str(&body_text(response).await).unwrap();
    assert_eq!(session["apiUrl"], format!("/api/jmap/{id}"));
    assert_eq!(
        session["downloadUrl"],
        format!("/api/jmap/{id}/download/{{accountId}}/{{blobId}}/{{name}}?type={{type}}")
    );
    assert_eq!(
        session["uploadUrl"],
        format!("/api/jmap/{id}/upload/{{accountId}}")
    );
    assert_eq!(
        session["eventSourceUrl"],
        format!("/api/jmap/{id}/events?types={{types}}&closeafter={{closeafter}}&ping={{ping}}")
    );
    let primary: Vec<&str> = session["primaryAccounts"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(primary, [MAIL], "{session}");
    let carried: Vec<&str> = session["capabilities"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(carried, [CORE, MAIL], "{session}");
    let core = &session["capabilities"][CORE];
    assert!(core["maxSizeRequest"].as_u64().unwrap() <= u64::try_from(JMAP_REQUEST_LIMIT).unwrap());
    assert!(
        core["maxConcurrentRequests"].as_u64().unwrap()
            <= u64::try_from(MAX_CONCURRENT_REQUESTS).unwrap()
    );
    let text = session.to_string();
    assert!(!text.contains(CYRUS_HOST), "{text}");
    let account = session["primaryAccounts"][MAIL].as_str().unwrap();
    let request = json!({
        "using": [CORE, MAIL],
        "methodCalls": [["Email/query", { "accountId": account, "limit": 10 }, "c1"]]
    });
    let (status, answer) = proxied(&router, &cookie, &id, &request).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let call = &answer["methodResponses"][0];
    assert_eq!(call[0], "Email/query", "{answer}");
    assert!(call[1]["ids"].is_array(), "{answer}");
    assert_eq!(call[2], "c1");
}

#[tokio::test]
async fn the_default_upstream_refuses_the_loopback_targets() {
    logging();
    let router = router_on(store_with_account());
    let response = router
        .clone()
        .oneshot(login_request(LOGIN, LOGIN_PASSWORD))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let cookie = signin::cookie_of(&response);
    for body in [cyrus_body(MAIL_PASSWORD), dovecot_body(MAIL_PASSWORD)] {
        let (status, text) = add(&router, &cookie, &body).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY, "{text}");
        assert!(text.contains("upstream_unreachable"));
    }
}
