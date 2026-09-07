// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A consent end to end: the row with its sealed tokens, the reconnect,
//! the refresh with its rotation and the log lines.

mod common;
mod consent;
mod fake_dns;
mod log_capture;
mod signin;
mod tls_server;
mod token_endpoint;

use std::sync::Arc;

use axum::http::{Method, StatusCode};
use common::router_on;
use consent::{GMAIL_ADDRESS, Rig, Visit, gmail_start_body, google_query};
use huliho_imap_bridge::testing::{TOKEN, imap, smtp};
use huliho_server::accounts::{self, Credential, NewAccount, Provider, StopCause};
use huliho_server::discovery::Address;
use huliho_server::events::{self, Actor};
use huliho_server::ids::AccountId;
use huliho_server::oauth::{self, RefreshError};
use huliho_server::presets;
use huliho_server::providers::OauthProvider;
use huliho_server::scope::{self, Scope};
use log_capture::Capture;
use oauth2::{PkceCodeChallenge, PkceCodeVerifier};
use serde_json::{Value, json};
use signin::{body_text, with_cookie};
use token_endpoint::{Answer, CLIENT_SECRET, CODE, REFRESH_TOKEN};
use tower::ServiceExt;
use url::Url;

const ROTATED_REFRESH_TOKEN: &str = "1//rotated-refresh";
/// Far enough ahead that no test runs into the margin.
const AN_HOUR_MS: i64 = 3_600_000;

/// The clock as the rows stamp it.
fn now_ms() -> i64 {
    let since_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    i64::try_from(since_epoch.as_millis()).unwrap()
}

async fn rig_with(refresh: Option<&'static str>) -> Rig {
    Rig::start(
        Answer::Tokens { refresh },
        imap::Script::tls(),
        smtp::Script::tls(),
    )
    .await
}

fn account_scope(rig: &Rig, id: &str) -> Scope {
    scope::resolve(
        &rig.store,
        &rig.user_id(),
        Some(&AccountId::from(id.to_owned())),
    )
    .unwrap()
}

/// The credential the router sealed on the row, opened with its keys.
fn credential(rig: &Rig, id: &str) -> Credential {
    accounts::credential(&rig.store, &rig.api.keys, &account_scope(rig, id)).unwrap()
}

/// A Gmail account with stored tokens, as an earlier consent left it.
fn gmail_account(expires_at: i64) -> NewAccount {
    let address = Address::parse(GMAIL_ADDRESS).unwrap();
    NewAccount {
        address: GMAIL_ADDRESS.to_owned(),
        name: "Gmail".to_owned(),
        provider: Provider::Gmail,
        settings: presets::fixed_target(Provider::Gmail, &address).unwrap(),
        credential: Credential::Oauth2 {
            provider: OauthProvider::Google,
            refresh_token: REFRESH_TOKEN.to_owned(),
            access_token: "ya29.stale".to_owned(),
            expires_at,
        },
    }
}

fn add_gmail_account(rig: &Rig, expires_at: i64) -> String {
    let scope = scope::resolve(&rig.store, &rig.user_id(), None).unwrap();
    accounts::add(
        &rig.store,
        &rig.api.keys,
        &scope,
        &gmail_account(expires_at),
    )
    .unwrap()
    .id
    .as_str()
    .to_owned()
}

fn event_types(rig: &Rig) -> Vec<String> {
    let scope = scope::resolve(&rig.store, &rig.user_id(), None).unwrap();
    events::for_organization(&rig.store, &scope)
        .unwrap()
        .into_iter()
        .map(|record| record.event_type)
        .collect()
}

async fn access_token(rig: &Rig, id: &str) -> Result<String, RefreshError> {
    oauth::access_token(
        Arc::clone(&rig.store),
        Arc::clone(&rig.api.keys),
        &rig.api.upstream,
        account_scope(rig, id),
    )
    .await
}

/// Starts and completes a consent; the state, the callback status and
/// the page.
async fn consent(rig: &Rig, cookie: &str, body: &Value) -> (String, StatusCode, String) {
    let (status, started) = rig.start_consent(cookie, body).await;
    assert_eq!(status, StatusCode::OK, "{started}");
    let state = started["state"].as_str().unwrap().to_owned();
    let query = google_query(&state);
    let (status, page) = rig
        .callback(Visit {
            cookie: Some(cookie),
            provider: "google",
            query: &query,
            language: Some("en-US,en;q=0.9"),
        })
        .await;
    (state, status, page)
}

#[tokio::test]
async fn a_gmail_consent_adds_the_account_with_xoauth2_and_seals_its_tokens() {
    let rig = rig_with(Some(REFRESH_TOKEN)).await;
    let cookie = rig.sign_in().await;
    let (status, started) = rig.start_consent(&cookie, &gmail_start_body()).await;
    assert_eq!(status, StatusCode::OK, "{started}");
    let state = started["state"].as_str().unwrap().to_owned();
    let challenge = Url::parse(started["url"].as_str().unwrap())
        .unwrap()
        .query_pairs()
        .find(|(key, _)| key == "code_challenge")
        .map(|(_, value)| value.into_owned())
        .unwrap();
    let query = google_query(&state);
    let (status, page) = rig
        .callback(Visit {
            cookie: Some(&cookie),
            provider: "google",
            query: &query,
            language: Some("en-US,en;q=0.9"),
        })
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("You can close this window."));
    assert!(page.contains("lang=\"en\""));
    let (_, pending) = rig.pending(&cookie, &state).await;
    assert_eq!(pending["status"], "done");
    let id = pending["accountId"].as_str().unwrap();
    let rows = rig.accounts(&cookie).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], id);
    assert_eq!(rows[0]["address"], GMAIL_ADDRESS);
    assert_eq!(rows[0]["name"], "Gmail");
    assert_eq!(rows[0]["provider"], "gmail");
    assert_eq!(rows[0]["kind"], "imap");
    assert_eq!(rows[0]["authMethod"], "oauth2");
    assert!(rows[0]["stoppedCause"].is_null());
    let Credential::Oauth2 {
        provider,
        refresh_token,
        access_token,
        expires_at,
    } = credential(&rig, id)
    else {
        panic!("an oauth credential");
    };
    assert_eq!(provider, OauthProvider::Google);
    assert_eq!(refresh_token, REFRESH_TOKEN);
    assert_eq!(access_token, TOKEN);
    assert!(expires_at > now_ms());
    let forms = rig.forms.lock().unwrap().clone();
    assert_eq!(forms.len(), 1);
    assert_eq!(rig.token_requests().len(), 1);
    assert_eq!(forms[0]["grant_type"], "authorization_code");
    assert_eq!(forms[0]["code"], CODE);
    assert_eq!(
        forms[0]["redirect_uri"],
        "https://mail.example.test/auth/google/callback"
    );
    assert_eq!(forms[0]["client_secret"], CLIENT_SECRET);
    let verifier = PkceCodeVerifier::new(forms[0]["code_verifier"].clone());
    assert_eq!(
        PkceCodeChallenge::from_code_verifier_sha256(&verifier).as_str(),
        challenge
    );
    assert!(
        rig.imap
            .lines()
            .iter()
            .any(|line| line.contains("AUTHENTICATE XOAUTH2"))
    );
    assert!(
        rig.smtp
            .lines()
            .iter()
            .any(|line| line.contains("AUTH XOAUTH2"))
    );
    assert!(event_types(&rig).contains(&"account.linked".to_owned()));
    let (status, _) = rig
        .callback(Visit {
            cookie: Some(&cookie),
            provider: "google",
            query: &query,
            language: None,
        })
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(rig.forms.lock().unwrap().len(), 1);
    assert_eq!(rig.accounts(&cookie).await.len(), 1);
    let plain = router_on(Arc::clone(&rig.store));
    let other = rig.sign_in_other().await;
    let response = plain
        .oneshot(with_cookie(Method::GET, "/api/accounts", &other))
        .await
        .unwrap();
    let listed: Value = serde_json::from_str(&body_text(response).await).unwrap();
    assert!(listed["accounts"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn a_reconnect_replaces_the_tokens_and_clears_the_credentials_stop() {
    let rig = rig_with(Some(ROTATED_REFRESH_TOKEN)).await;
    let cookie = rig.sign_in().await;
    let id = add_gmail_account(&rig, 0);
    accounts::stop(
        &rig.store,
        &account_scope(&rig, &id),
        StopCause::Credentials,
        &Actor::System,
    )
    .unwrap();
    assert_eq!(
        rig.accounts(&cookie).await[0]["stoppedCause"],
        "credentials"
    );
    let mut body = gmail_start_body();
    body["accountId"] = json!(id);
    let (state, status, _) = consent(&rig, &cookie, &body).await;
    assert_eq!(status, StatusCode::OK);
    let (_, pending) = rig.pending(&cookie, &state).await;
    assert_eq!(pending, json!({ "status": "done", "accountId": id }));
    let rows = rig.accounts(&cookie).await;
    assert_eq!(rows.len(), 1);
    assert!(rows[0]["stoppedCause"].is_null());
    assert_eq!(rows[0]["authMethod"], "oauth2");
    let Credential::Oauth2 {
        refresh_token,
        access_token,
        ..
    } = credential(&rig, &id)
    else {
        panic!("an oauth credential");
    };
    assert_eq!(refresh_token, ROTATED_REFRESH_TOKEN);
    assert_eq!(access_token, TOKEN);
    assert!(event_types(&rig).contains(&"account.credentials_updated".to_owned()));
    let theirs = accounts::add(
        &rig.store,
        &rig.api.keys,
        &scope::resolve(&rig.store, &rig.other_user_id(), None).unwrap(),
        &gmail_account(0),
    )
    .unwrap();
    body["accountId"] = json!(theirs.id.as_str());
    let (status, _) = rig.start_consent(&cookie, &body).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_fresh_token_is_handed_out_without_a_request_and_a_stale_one_refreshes_and_rotates() {
    let rig = rig_with(Some(ROTATED_REFRESH_TOKEN)).await;
    let fresh = add_gmail_account(&rig, now_ms() + AN_HOUR_MS);
    assert_eq!(access_token(&rig, &fresh).await.unwrap(), "ya29.stale");
    assert!(rig.forms.lock().unwrap().is_empty());
    let stale = add_gmail_account(&rig, 0);
    assert_eq!(access_token(&rig, &stale).await.unwrap(), TOKEN);
    let forms = rig.forms.lock().unwrap().clone();
    assert_eq!(forms.len(), 1);
    assert_eq!(forms[0]["grant_type"], "refresh_token");
    assert_eq!(forms[0]["refresh_token"], REFRESH_TOKEN);
    assert_eq!(forms[0]["client_secret"], CLIENT_SECRET);
    let Credential::Oauth2 {
        refresh_token,
        access_token: stored,
        expires_at,
        ..
    } = credential(&rig, &stale)
    else {
        panic!("an oauth credential");
    };
    assert_eq!(refresh_token, ROTATED_REFRESH_TOKEN);
    assert_eq!(stored, TOKEN);
    assert!(expires_at > now_ms());
    let kept = rig_with(None).await;
    let stale = add_gmail_account(&kept, 0);
    access_token(&kept, &stale).await.unwrap();
    let Credential::Oauth2 { refresh_token, .. } = credential(&kept, &stale) else {
        panic!("an oauth credential");
    };
    assert_eq!(refresh_token, REFRESH_TOKEN);
}

#[tokio::test]
async fn a_refused_refresh_stops_the_account_and_a_broken_endpoint_does_not() {
    let refused = Rig::start(
        Answer::InvalidGrant,
        imap::Script::tls(),
        smtp::Script::tls(),
    )
    .await;
    let cookie = refused.sign_in().await;
    let id = add_gmail_account(&refused, 0);
    let result = access_token(&refused, &id).await;
    assert!(matches!(result, Err(RefreshError::Revoked)), "{result:?}");
    let rows = refused.accounts(&cookie).await;
    assert_eq!(rows[0]["stoppedCause"], "credentials");
    assert!(rows[0]["stoppedAt"].as_i64().is_some());
    let stopped = events::for_organization(
        &refused.store,
        &scope::resolve(&refused.store, &refused.user_id(), None).unwrap(),
    )
    .unwrap()
    .into_iter()
    .find(|record| record.event_type == "account.stopped")
    .unwrap();
    assert_eq!(stopped.actor, "system");
    assert!(stopped.payload.contains("\"cause\":\"credentials\""));
    let broken = Rig::start(Answer::Broken, imap::Script::tls(), smtp::Script::tls()).await;
    let cookie = broken.sign_in().await;
    let id = add_gmail_account(&broken, 0);
    let result = access_token(&broken, &id).await;
    assert!(
        matches!(result, Err(RefreshError::Unavailable(_))),
        "{result:?}"
    );
    assert!(broken.accounts(&cookie).await[0]["stoppedCause"].is_null());
    let Credential::Oauth2 {
        access_token: stored,
        ..
    } = credential(&broken, &id)
    else {
        panic!("an oauth credential");
    };
    assert_eq!(stored, "ya29.stale");
    let password = accounts::add(
        &broken.store,
        &broken.api.keys,
        &scope::resolve(&broken.store, &broken.user_id(), None).unwrap(),
        &NewAccount {
            credential: Credential::Password {
                password: "app password".to_owned(),
            },
            ..gmail_account(0)
        },
    )
    .unwrap();
    let result = access_token(&broken, password.id.as_str()).await;
    assert!(matches!(result, Err(RefreshError::NotOauth)), "{result:?}");
}

#[tokio::test]
async fn no_log_line_carries_a_code_a_token_the_secret_the_state_or_the_address() {
    let capture = Capture::install();
    let rig = rig_with(Some(ROTATED_REFRESH_TOKEN)).await;
    let cookie = rig.sign_in().await;
    let (state, status, _) = consent(&rig, &cookie, &gmail_start_body()).await;
    assert_eq!(status, StatusCode::OK);
    let (_, pending) = rig.pending(&cookie, &state).await;
    assert_eq!(pending["status"], "done");
    let expired = add_gmail_account(&rig, 0);
    access_token(&rig, &expired).await.unwrap();
    let (status, started) = rig.start_consent(&cookie, &gmail_start_body()).await;
    assert_eq!(status, StatusCode::OK, "{started}");
    let refused = started["state"].as_str().unwrap().to_owned();
    let denied = format!("error=access_denied&state={refused}");
    rig.callback(Visit {
        cookie: Some(&cookie),
        provider: "google",
        query: &denied,
        language: None,
    })
    .await;
    let text = capture.text();
    assert!(text.contains("/auth/{provider}/callback"), "{text}");
    assert!(text.contains("consent denied"), "{text}");
    for secret in [
        CODE,
        TOKEN,
        REFRESH_TOKEN,
        ROTATED_REFRESH_TOKEN,
        CLIENT_SECRET,
        GMAIL_ADDRESS,
        state.as_str(),
        refused.as_str(),
        "sanne",
    ] {
        assert!(!text.contains(secret), "{secret} in {text}");
    }
}
