// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The proxy routes over HTTP: who reaches them, what the session
//! object looks like from the browser and where the limits sit.

mod common;
mod fake_dns;
mod jmap_upstream;
mod proxy_rig;
mod readers;
mod signin;
mod tls_server;

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use huliho_imap_bridge::testing::{PASSWORD, TOKEN};
use huliho_server::accounts::{
    self, AccountSettings, Credential, Endpoint, NewAccount, Provider, StopCause, TlsMode,
};
use huliho_server::events::Actor;
use huliho_server::gate::RUN_WINDOW;
use huliho_server::jmap::{JMAP_REQUEST_LIMIT, MAX_CONCURRENT_REQUESTS};
use huliho_server::scope;
use jmap_upstream::{HOST, INWARD_HOST, JmapUpstream, Script, UPSTREAM_ACCOUNT};
use proxy_rig::{Instance, Setup, query_request};
use serde_json::{Value, json};
use signin::{body_text, with_cookie};
use tokio::task::JoinSet;
use tower::ServiceExt;

const CORE: &str = "urn:ietf:params:jmap:core";
const MAIL: &str = "urn:ietf:params:jmap:mail";

/// A body well over the 16 KiB of the other API routes and well under
/// the proxy's own limit.
const WIDE_BODY_BYTES: usize = 20 * 1024;

/// How often and how long the cap test looks for the parked requests.
const POLL: Duration = Duration::from_millis(10);
const PATIENCE: u32 = 300;

fn bearer() -> Credential {
    Credential::Bearer {
        token: TOKEN.to_owned(),
    }
}

fn password() -> Credential {
    Credential::Password {
        password: PASSWORD.to_owned(),
    }
}

async fn instance() -> Instance {
    Instance::start(Setup {
        window: RUN_WINDOW,
        servers: &[],
    })
    .await
}

fn keys(value: &Value) -> Vec<&str> {
    value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect()
}

fn posts(instance: &Instance) -> usize {
    instance
        .upstream
        .lines()
        .iter()
        .filter(|line| line.contains("/jmap/api"))
        .count()
}

/// A POST on the request route as the client sends it, with the given
/// content type and body.
fn post(cookie: &str, id: &str, content_type: &str, body: Vec<u8>) -> Request<Body> {
    let mut request = with_cookie(Method::POST, &format!("/api/jmap/{id}"), cookie);
    request
        .headers_mut()
        .insert(header::CONTENT_TYPE, content_type.parse().unwrap());
    *request.body_mut() = Body::from(body);
    request
}

async fn send(instance: &Instance, request: Request<Body>) -> (StatusCode, String) {
    let response = instance.router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    (status, body_text(response).await)
}

#[tokio::test]
async fn another_users_account_is_not_found_on_both_routes_and_nothing_connects() {
    let instance = instance().await;
    let id = instance.add_account(bearer());
    let other = instance.sign_in_other().await;
    let (status, body) = instance.session(&other, &id).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (status, body) = instance.request(&other, &id, &query_request()).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert!(instance.upstream.lines().is_empty());
    assert_eq!(
        instance.account_events(),
        [(
            "account.linked".to_owned(),
            instance.user_id().as_str().to_owned()
        )]
    );
}

#[tokio::test]
async fn both_routes_need_a_session_and_the_request_route_the_header() {
    let instance = instance().await;
    let id = instance.add_account(bearer());
    let (status, _) = instance.session("huliho_session=stale", &id).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = instance
        .request("huliho_session=stale", &id, &query_request())
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let cookie = instance.sign_in().await;
    let bare = Request::builder()
        .method(Method::POST)
        .uri(format!("/api/jmap/{id}"))
        .header(header::COOKIE, &cookie)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(query_request().to_string()))
        .unwrap();
    let (status, text) = send(&instance, bare).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(text.contains("missing_csrf_header"), "{text}");
    assert!(instance.upstream.lines().is_empty());
}

#[tokio::test]
async fn an_imap_account_is_unsupported_without_a_connection() {
    let instance = instance().await;
    let endpoint = |port| Endpoint {
        host: HOST.to_owned(),
        port,
        tls: TlsMode::Implicit,
    };
    let new = NewAccount {
        address: jmap_upstream::ADDRESS.to_owned(),
        name: "Work".to_owned(),
        provider: Provider::Generic,
        settings: AccountSettings::Imap {
            username: "sanne".to_owned(),
            imap: endpoint(993),
            smtp: endpoint(465),
        },
        credential: password(),
    };
    let scope = scope::resolve(&instance.store, &instance.user_id(), None).unwrap();
    let id = accounts::add(&instance.store, &instance.api.keys, &scope, &new).unwrap();
    let cookie = instance.sign_in().await;
    for (status, body) in [
        instance.session(&cookie, id.id.as_str()).await,
        instance
            .request(&cookie, id.id.as_str(), &query_request())
            .await,
    ] {
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["error"], "upstream_unsupported");
    }
    assert!(instance.upstream.lines().is_empty());
    assert_eq!(instance.stopped_cause(id.id.as_str()), None);
}

#[tokio::test]
async fn a_stopped_account_answers_409_with_its_cause_before_anything_connects() {
    let instance = instance().await;
    let id = instance.add_account(bearer());
    let cookie = instance.sign_in().await;
    let scope = scope::resolve(
        &instance.store,
        &instance.user_id(),
        Some(&huliho_server::ids::AccountId::from(id.clone())),
    )
    .unwrap();
    accounts::stop(
        &instance.store,
        &scope,
        StopCause::Connection,
        &Actor::System,
    )
    .unwrap();
    for (status, body) in [
        instance.session(&cookie, &id).await,
        instance.request(&cookie, &id, &query_request()).await,
    ] {
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert_eq!(body["error"], "still_stopped");
        assert_eq!(body["cause"], "connection");
    }
    assert!(instance.upstream.lines().is_empty());
    assert_eq!(instance.stopped_cause(&id).as_deref(), Some("connection"));
}

#[tokio::test]
async fn the_session_object_comes_back_rewritten_and_filtered() {
    let instance = instance().await;
    let id = instance.add_account(bearer());
    let cookie = instance.sign_in().await;
    instance.upstream.set(Script {
        websocket_url: Some(format!("wss://{HOST}/jmap/ws")),
        ..Script::echo(instance.upstream.port())
    });
    let (status, session) = instance.session(&cookie, &id).await;
    assert_eq!(status, StatusCode::OK, "{session}");
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
    assert_eq!(keys(&session["capabilities"]), [CORE, MAIL]);
    assert_eq!(
        keys(&session["accounts"][UPSTREAM_ACCOUNT]["accountCapabilities"]),
        [CORE, MAIL]
    );
    assert_eq!(keys(&session["primaryAccounts"]), [MAIL]);
    let core = &session["capabilities"][CORE];
    assert_eq!(core["maxSizeRequest"], JMAP_REQUEST_LIMIT);
    assert_eq!(core["maxConcurrentRequests"], MAX_CONCURRENT_REQUESTS);
    assert_eq!(core["maxObjectsInGet"], 500);
    assert_eq!(session["username"], jmap_upstream::ADDRESS);
    assert_eq!(session["state"], "75128aab4b1b");
    let text = session.to_string();
    assert!(!text.contains("websocket"), "{text}");
    assert!(!text.contains("wss://"), "{text}");
    assert!(!text.contains(&format!("https://{HOST}")), "{text}");
    let lines = instance.upstream.lines();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].ends_with(&format!("Bearer {TOKEN}")), "{lines:?}");
}

#[tokio::test]
async fn a_request_is_forwarded_with_the_credential_and_the_answer_comes_back() {
    let instance = instance().await;
    let id = instance.add_account(password());
    let cookie = instance.sign_in().await;
    let (status, answer) = instance.request(&cookie, &id, &query_request()).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let call = &answer["methodResponses"][0];
    assert_eq!(call[0], "Email/query");
    assert_eq!(call[1]["echoed"]["accountId"], UPSTREAM_ACCOUNT);
    assert_eq!(call[2], "c1");
    // The first request learns the API endpoint from the session object;
    // the second one goes straight to it.
    let lines = instance.upstream.lines();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[0].contains("/jmap/session"), "{lines:?}");
    assert!(lines[1].contains("/jmap/api"), "{lines:?}");
    assert!(lines[1].ends_with(&JmapUpstream::basic()), "{lines:?}");
    let (status, _) = instance.request(&cookie, &id, &query_request()).await;
    assert_eq!(status, StatusCode::OK);
    let lines = instance.upstream.lines();
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(lines[2].contains("/jmap/api"), "{lines:?}");
}

#[tokio::test]
async fn a_relative_api_url_resolves_against_the_session_url() {
    let instance = instance().await;
    let id = instance.add_account(bearer());
    let cookie = instance.sign_in().await;
    instance.upstream.set(Script {
        api_url: "/jmap/api".to_owned(),
        ..Script::echo(instance.upstream.port())
    });
    let (status, session) = instance.session(&cookie, &id).await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let (status, answer) = instance.request(&cookie, &id, &query_request()).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(posts(&instance), 1);
}

#[tokio::test]
async fn a_session_object_pointing_inward_or_off_https_is_refused() {
    let instance = instance().await;
    let id = instance.add_account(bearer());
    let cookie = instance.sign_in().await;
    let port = instance.upstream.port();
    for api_url in [
        format!("https://{INWARD_HOST}:{port}/jmap/api"),
        format!("https://127.0.0.1:{port}/jmap/api"),
        format!("http://{HOST}:{port}/jmap/api"),
        format!("https://sanne:secret@{HOST}:{port}/jmap/api"),
        "mailto:sanne@example.test".to_owned(),
    ] {
        instance.upstream.set(Script {
            api_url: api_url.clone(),
            ..Script::echo(port)
        });
        let (status, body) = instance.session(&cookie, &id).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{api_url}: {body}");
        assert_eq!(body["error"], "upstream_unsupported", "{api_url}");
        let (status, body) = instance.request(&cookie, &id, &query_request()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{api_url}: {body}");
        assert_eq!(body["error"], "upstream_unsupported", "{api_url}");
        assert_eq!(instance.stopped_cause(&id), None, "{api_url}");
    }
    assert_eq!(posts(&instance), 0);
    instance.upstream.set(Script::echo(port));
    let (status, _) = instance.request(&cookie, &id, &query_request()).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn a_wide_body_passes_and_one_past_the_limit_answers_413() {
    let instance = instance().await;
    let id = instance.add_account(bearer());
    let cookie = instance.sign_in().await;
    let mut wide = query_request();
    wide["padding"] = json!("x".repeat(WIDE_BODY_BYTES));
    let (status, answer) = instance.request(&cookie, &id, &wide).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let mut past = query_request();
    past["padding"] = json!("x".repeat(JMAP_REQUEST_LIMIT));
    let request = post(
        &cookie,
        &id,
        "application/json",
        past.to_string().into_bytes(),
    );
    let response = instance.router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let content_type = response.headers()[header::CONTENT_TYPE]
        .to_str()
        .unwrap()
        .to_owned();
    assert!(content_type.starts_with("text/plain"), "{content_type}");
    assert_eq!(posts(&instance), 1);
}

#[tokio::test]
async fn a_request_without_a_json_content_type_is_invalid_before_anything_connects() {
    let instance = instance().await;
    let id = instance.add_account(bearer());
    let cookie = instance.sign_in().await;
    for content_type in ["text/plain", "application/x-www-form-urlencoded"] {
        let request = post(
            &cookie,
            &id,
            content_type,
            query_request().to_string().into_bytes(),
        );
        let (status, text) = send(&instance, request).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{content_type}: {text}");
        assert!(text.contains("invalid_request"), "{text}");
    }
    let request = post(
        &cookie,
        &id,
        "application/json; charset=utf-8",
        query_request().to_string().into_bytes(),
    );
    let (status, _) = send(&instance, request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(instance.upstream.lines().len(), 2);
}

#[tokio::test]
async fn a_fifth_concurrent_request_gets_the_limit_error_while_four_run() {
    let instance = Arc::new(instance().await);
    let id = instance.add_account(bearer());
    let cookie = instance.sign_in().await;
    let (status, _) = instance.session(&cookie, &id).await;
    assert_eq!(status, StatusCode::OK);
    let port = instance.upstream.port();
    instance.upstream.set(Script {
        hold: true,
        ..Script::echo(port)
    });
    let mut running = JoinSet::new();
    for _ in 0..MAX_CONCURRENT_REQUESTS {
        let (instance, cookie, id) = (Arc::clone(&instance), cookie.clone(), id.clone());
        running.spawn(async move { instance.request(&cookie, &id, &query_request()).await });
    }
    let mut waited = 0;
    while posts(&instance) < MAX_CONCURRENT_REQUESTS && waited < PATIENCE {
        tokio::time::sleep(POLL).await;
        waited += 1;
    }
    assert_eq!(posts(&instance), MAX_CONCURRENT_REQUESTS);
    let (status, problem) = instance.request(&cookie, &id, &query_request()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{problem}");
    assert_eq!(problem["type"], "urn:ietf:params:jmap:error:limit");
    assert_eq!(problem["limit"], "maxConcurrentRequests");
    assert_eq!(problem["status"], 400);
    assert_eq!(posts(&instance), MAX_CONCURRENT_REQUESTS);
    instance.upstream.set(Script::echo(port));
    while let Some(finished) = running.join_next().await {
        let (status, answer) = finished.unwrap();
        assert_eq!(status, StatusCode::OK, "{answer}");
    }
    let (status, _) = instance.request(&cookie, &id, &query_request()).await;
    assert_eq!(status, StatusCode::OK);
}
