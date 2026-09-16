// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The proxy and the gate over HTTP: the stop after one refusal, the
//! credential kept out of every log line, the window and the refresh
//! under the account lock.

mod common;
mod fake_dns;
mod jmap_upstream;
mod log_capture;
mod proxy_rig;
mod readers;
mod signin;
mod tls_server;
mod token_endpoint;

use std::sync::Arc;
use std::time::Duration;

use axum::http::StatusCode;
use huliho_imap_bridge::testing::{PASSWORD, TOKEN};
use huliho_server::accounts::{self, Credential};
use huliho_server::events::Actor;
use huliho_server::gate::{MAX_REFUSED_RUN, RUN_WINDOW, Reconnect};
use huliho_server::identity;
use huliho_server::ids::AccountId;
use huliho_server::jmap::MAX_CONCURRENT_REQUESTS;
use huliho_server::providers::{self, OauthClient, OauthProvider};
use huliho_server::scope::{self, Scope};
use jmap_upstream::{ADDRESS, HOST, JmapUpstream, Script};
use log_capture::Capture;
use proxy_rig::{Instance, Setup, query_request};
use signin::LOGIN;
use tls_server::TlsServer;
use token_endpoint::{Answer, CLIENT_ID, CLIENT_SECRET, Forms, REFRESH_TOKEN, routes};
use tokio::net::TcpListener;
use tokio::task::JoinSet;

/// The host the Google preset's token endpoint lives on.
const TOKEN_HOST: &str = "oauth2.googleapis.com";
const STALE_TOKEN: &str = "ya29.stale";

fn bearer() -> Credential {
    Credential::Bearer {
        token: TOKEN.to_owned(),
    }
}

fn password(password: &str) -> Credential {
    Credential::Password {
        password: password.to_owned(),
    }
}

/// Provider tokens whose access token ran out, so the next use refreshes.
fn stale_tokens() -> Credential {
    Credential::Oauth2 {
        provider: OauthProvider::Google,
        refresh_token: REFRESH_TOKEN.to_owned(),
        access_token: STALE_TOKEN.to_owned(),
        expires_at: 0,
    }
}

fn event(kind: &str, actor: &str) -> (String, String) {
    (kind.to_owned(), actor.to_owned())
}

async fn instance(window: Duration) -> Instance {
    Instance::start(Setup {
        window,
        servers: &[],
    })
    .await
}

/// An instance with the Google client registered and a token endpoint
/// answering as given; the forms that endpoint received.
async fn oauth_instance(answer: Answer) -> (Instance, TlsServer, Forms) {
    let forms = Forms::default();
    let token_server = TlsServer::start(routes(answer, Arc::clone(&forms))).await;
    let instance = Instance::start(Setup {
        window: RUN_WINDOW,
        servers: &[(TOKEN_HOST, &token_server)],
    })
    .await;
    identity::grant_instance_admin(&instance.store, LOGIN).unwrap();
    let scope = scope::resolve(&instance.store, &instance.user_id(), None).unwrap();
    let client = OauthClient {
        provider: OauthProvider::Google,
        id: CLIENT_ID.to_owned(),
        secret: CLIENT_SECRET.to_owned(),
    };
    providers::set_client(&instance.store, &instance.api.keys, &scope, &client).unwrap();
    identity::revoke_instance_admin(&instance.store, LOGIN).unwrap();
    (instance, token_server, forms)
}

fn refreshes(forms: &Forms) -> usize {
    forms
        .lock()
        .unwrap()
        .iter()
        .filter(|form| form.get("grant_type").map(String::as_str) == Some("refresh_token"))
        .count()
}

fn posts(instance: &Instance) -> usize {
    instance
        .upstream
        .lines()
        .iter()
        .filter(|line| line.contains("/jmap/api"))
        .count()
}

fn account_scope(instance: &Instance, id: &str) -> Scope {
    scope::resolve(
        &instance.store,
        &instance.user_id(),
        Some(&AccountId::from(id.to_owned())),
    )
    .unwrap()
}

/// The echo script with its API endpoint on a port nothing listens on.
async fn closed_endpoint(instance: &Instance) -> Script {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let closed = listener.local_addr().unwrap().port();
    drop(listener);
    Script {
        api_url: format!("https://{HOST}:{closed}/jmap/api"),
        ..Script::echo(instance.upstream.port())
    }
}

#[tokio::test]
async fn a_401_stops_the_account_after_one_request_with_cause_credentials() {
    let instance = instance(RUN_WINDOW).await;
    let id = instance.add_account(password("wrong horse"));
    let cookie = instance.sign_in().await;
    let (status, body) = instance.session(&cookie, &id).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(body["error"], "upstream_credentials");
    assert_eq!(instance.stopped_cause(&id).as_deref(), Some("credentials"));
    assert_eq!(
        instance.account_events().last(),
        Some(&event("account.stopped", instance.user_id().as_str()))
    );
    assert_eq!(instance.upstream.lines().len(), 1);
    for (status, body) in [
        instance.session(&cookie, &id).await,
        instance.request(&cookie, &id, &query_request()).await,
    ] {
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert_eq!(body["error"], "still_stopped");
        assert_eq!(body["cause"], "credentials");
    }
    assert_eq!(instance.upstream.lines().len(), 1);
}

#[tokio::test]
async fn the_credential_rides_upstream_and_reaches_no_log_line() {
    let capture = Capture::install();
    let instance = instance(RUN_WINDOW).await;
    let by_password = instance.add_account(password(PASSWORD));
    let by_token = instance.add_account(bearer());
    let cookie = instance.sign_in().await;
    for id in [&by_password, &by_token] {
        let (status, answer) = instance.request(&cookie, id, &query_request()).await;
        assert_eq!(status, StatusCode::OK, "{answer}");
    }
    let wrong = instance.add_account(password("wrong horse"));
    instance.session(&cookie, &wrong).await;
    let lines = instance.upstream.lines();
    let basic = JmapUpstream::basic();
    let token = format!("Bearer {TOKEN}");
    for credential in [&basic, &token] {
        assert!(
            lines
                .iter()
                .any(|line| line.contains("/jmap/api") && line.ends_with(credential)),
            "{lines:?}"
        );
    }
    let text = capture.text();
    assert!(text.contains("account stopped"), "{text}");
    for secret in [
        PASSWORD,
        "wrong horse",
        TOKEN,
        ADDRESS,
        HOST,
        basic.as_str(),
        REFRESH_TOKEN,
        CLIENT_SECRET,
        STALE_TOKEN,
    ] {
        assert!(!text.contains(secret), "{secret} in {text}");
    }
}

#[tokio::test]
async fn a_burst_of_refused_connections_counts_once_and_the_account_runs_on() {
    let instance = Arc::new(instance(RUN_WINDOW).await);
    let id = instance.add_account(bearer());
    let cookie = instance.sign_in().await;
    instance.upstream.set(closed_endpoint(&instance).await);
    // The endpoint check resolves the host, not the port, so the session
    // object passes and the closed port is met on the first request.
    let (status, body) = instance.session(&cookie, &id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let mut burst = JoinSet::new();
    for _ in 0..MAX_CONCURRENT_REQUESTS {
        let (instance, cookie, id) = (Arc::clone(&instance), cookie.clone(), id.clone());
        burst.spawn(async move { instance.request(&cookie, &id, &query_request()).await });
    }
    while let Some(finished) = burst.join_next().await {
        let (status, body) = finished.unwrap();
        assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
        assert_eq!(body["error"], "upstream_unreachable");
    }
    for _ in 0..MAX_REFUSED_RUN {
        let (status, body) = instance.request(&cookie, &id, &query_request()).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
        assert_eq!(body["error"], "upstream_unreachable");
    }
    assert_eq!(instance.stopped_cause(&id), None);
    assert!(
        !instance
            .account_events()
            .iter()
            .any(|(kind, _)| kind == "account.stopped")
    );
}

#[tokio::test]
async fn failures_in_five_windows_stop_the_account_and_a_stopped_one_fetches_nothing() {
    let instance = instance(Duration::ZERO).await;
    let id = instance.add_account(bearer());
    let cookie = instance.sign_in().await;
    instance.upstream.set(closed_endpoint(&instance).await);
    let (status, _) = instance.session(&cookie, &id).await;
    assert_eq!(status, StatusCode::OK);
    for attempt in 1..=MAX_REFUSED_RUN {
        let (status, body) = instance.request(&cookie, &id, &query_request()).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
        if attempt < MAX_REFUSED_RUN {
            assert_eq!(instance.stopped_cause(&id), None, "attempt {attempt}");
        }
    }
    assert_eq!(instance.stopped_cause(&id).as_deref(), Some("connection"));
    assert_eq!(
        instance.account_events().last(),
        Some(&event("account.stopped", "system"))
    );
    let (status, body) = instance.request(&cookie, &id, &query_request()).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["cause"], "connection");
    // The one session fetch from the start; a stopped row fetches none.
    assert_eq!(instance.upstream.lines().len(), 1);
}

#[tokio::test]
async fn a_server_error_an_oversized_or_a_non_json_answer_decides_nothing() {
    let instance = instance(Duration::ZERO).await;
    let id = instance.add_account(bearer());
    let cookie = instance.sign_in().await;
    let port = instance.upstream.port();
    instance.upstream.set(Script {
        status: StatusCode::SERVICE_UNAVAILABLE.as_u16(),
        ..Script::echo(port)
    });
    for _ in 0..MAX_REFUSED_RUN {
        let (status, body) = instance.request(&cookie, &id, &query_request()).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
        assert_eq!(body["error"], "upstream_failed");
    }
    for script in [
        Script {
            oversized: true,
            ..Script::echo(port)
        },
        Script {
            json: false,
            ..Script::echo(port)
        },
        Script {
            status: StatusCode::NOT_FOUND.as_u16(),
            ..Script::echo(port)
        },
    ] {
        instance.upstream.set(script);
        let (status, body) = instance.request(&cookie, &id, &query_request()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["error"], "upstream_unsupported");
    }
    assert_eq!(instance.stopped_cause(&id), None);
    assert!(
        !instance
            .account_events()
            .iter()
            .any(|(kind, _)| kind == "account.stopped")
    );
    instance.upstream.set(Script::echo(port));
    let (status, _) = instance.request(&cookie, &id, &query_request()).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn a_refused_refresh_stops_the_account_and_a_broken_endpoint_is_unreachable() {
    let (refused, _endpoint, forms) = oauth_instance(Answer::InvalidGrant).await;
    let id = refused.add_account(stale_tokens());
    let cookie = refused.sign_in().await;
    let (status, body) = refused.session(&cookie, &id).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(body["error"], "upstream_credentials");
    assert_eq!(refused.stopped_cause(&id).as_deref(), Some("credentials"));
    assert_eq!(refreshes(&forms), 1);
    assert!(refused.upstream.lines().is_empty());
    let (broken, _endpoint, forms) = oauth_instance(Answer::Broken).await;
    let id = broken.add_account(stale_tokens());
    let cookie = broken.sign_in().await;
    let (status, body) = broken.request(&cookie, &id, &query_request()).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert_eq!(body["error"], "upstream_unreachable");
    assert_eq!(broken.stopped_cause(&id), None);
    assert_eq!(refreshes(&forms), 1);
    assert!(broken.upstream.lines().is_empty());
    let stored = accounts::credential(
        &broken.store,
        &broken.api.keys,
        &account_scope(&broken, &id),
    )
    .unwrap();
    assert_eq!(stored, stale_tokens());
}

#[tokio::test]
async fn two_requests_on_an_expiring_oauth_account_redeem_the_refresh_token_once() {
    let (instance, _endpoint, forms) = oauth_instance(Answer::Tokens {
        refresh: Some(REFRESH_TOKEN),
    })
    .await;
    let instance = Arc::new(instance);
    let id = instance.add_account(stale_tokens());
    let cookie = instance.sign_in().await;
    let mut both = JoinSet::new();
    for _ in 0..2 {
        let (instance, cookie, id) = (Arc::clone(&instance), cookie.clone(), id.clone());
        both.spawn(async move { instance.request(&cookie, &id, &query_request()).await });
    }
    while let Some(finished) = both.join_next().await {
        let (status, answer) = finished.unwrap();
        assert_eq!(status, StatusCode::OK, "{answer}");
    }
    assert_eq!(refreshes(&forms), 1);
    let lines = instance.upstream.lines();
    assert!(
        lines
            .iter()
            .all(|line| line.ends_with(&format!("Bearer {TOKEN}"))),
        "{lines:?}"
    );
    assert_eq!(posts(&instance), 2);
}

#[tokio::test]
async fn a_request_and_the_probe_on_the_same_expiring_account_both_finish() {
    let (instance, _endpoint, forms) = oauth_instance(Answer::Tokens {
        refresh: Some(REFRESH_TOKEN),
    })
    .await;
    let instance = Arc::new(instance);
    let id = instance.add_account(stale_tokens());
    let cookie = instance.sign_in().await;
    let scope = account_scope(&instance, &id);
    let check = {
        let instance = Arc::clone(&instance);
        tokio::spawn(async move {
            Reconnect::from(&instance.api)
                .retry(&scope, &Actor::System)
                .await
        })
    };
    let (status, answer) = instance.request(&cookie, &id, &query_request()).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let row = check.await.unwrap().unwrap();
    assert_eq!(row.stopped_cause, None);
    assert_eq!(refreshes(&forms), 1);
    assert_eq!(posts(&instance), 1);
}

#[tokio::test]
async fn another_user_redeems_nothing_and_reaches_nothing() {
    let (instance, _endpoint, forms) = oauth_instance(Answer::Tokens {
        refresh: Some(REFRESH_TOKEN),
    })
    .await;
    let id = instance.add_account(stale_tokens());
    let other = instance.sign_in_other().await;
    let (status, _) = instance.session(&other, &id).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = instance.request(&other, &id, &query_request()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(refreshes(&forms), 0);
    assert!(instance.upstream.lines().is_empty());
    assert_eq!(instance.stopped_cause(&id), None);
}
