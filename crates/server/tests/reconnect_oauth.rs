// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A retry on an account that signed in through a provider: the consent
//! first, then the stale token refreshed on the way to XOAUTH2.

mod common;
mod consent;
mod fake_dns;
mod signin;
mod tls_server;
mod token_endpoint;

use std::sync::Arc;

use axum::http::{Method, StatusCode};
use common::router_on;
use consent::{GMAIL_ADDRESS, Rig, Visit, gmail_start_body, google_query};
use huliho_imap_bridge::testing::{imap, smtp};
use huliho_server::accounts::{self, Credential, NewAccount, Provider, StopCause};
use huliho_server::discovery::Address;
use huliho_server::events::{self, Actor};
use huliho_server::ids::AccountId;
use huliho_server::presets;
use huliho_server::providers::OauthProvider;
use huliho_server::scope::{self, Scope};
use huliho_server::store::StoreError;
use serde_json::{Value, json};
use signin::{body_text, with_cookie};
use token_endpoint::{Answer, REFRESH_TOKEN};
use tower::ServiceExt;

async fn rig_answering(answer: Answer) -> Rig {
    Rig::start(answer, imap::Script::tls(), smtp::Script::tls()).await
}

async fn rig() -> Rig {
    rig_answering(Answer::Tokens {
        refresh: Some(REFRESH_TOKEN),
    })
    .await
}

/// A Gmail row with a stale token, as an earlier consent left it; no
/// consent runs, so the endpoint's answer reaches the refresh alone.
fn stale_gmail_account(rig: &Rig) -> String {
    let address = Address::parse(GMAIL_ADDRESS).unwrap();
    let new = NewAccount {
        address: GMAIL_ADDRESS.to_owned(),
        name: "Gmail".to_owned(),
        provider: Provider::Gmail,
        settings: presets::fixed_target(Provider::Gmail, &address).unwrap(),
        credential: Credential::Oauth2 {
            provider: OauthProvider::Google,
            refresh_token: REFRESH_TOKEN.to_owned(),
            access_token: "ya29.stale".to_owned(),
            expires_at: 0,
        },
    };
    let scope = scope::resolve(&rig.store, &rig.user_id(), None).unwrap();
    accounts::add(&rig.store, &rig.api.keys, &scope, &new)
        .unwrap()
        .id
        .as_str()
        .to_owned()
}

/// A consent through the window, completed by the callback; the row's
/// id.
async fn connected_account(rig: &Rig, cookie: &str) -> String {
    let (status, started) = rig.start_consent(cookie, &gmail_start_body()).await;
    assert_eq!(status, StatusCode::OK, "{started}");
    let state = started["state"].as_str().unwrap().to_owned();
    let query = google_query(&state);
    let (status, _page) = rig
        .callback(Visit {
            cookie: Some(cookie),
            provider: "google",
            query: &query,
            language: None,
        })
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, outcome) = rig.pending(cookie, &state).await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["status"], "done", "{outcome}");
    outcome["accountId"].as_str().unwrap().to_owned()
}

fn account_scope(rig: &Rig, id: &str) -> Scope {
    scope::resolve(
        &rig.store,
        &rig.user_id(),
        Some(&AccountId::from(id.to_owned())),
    )
    .unwrap()
}

fn stored_tokens(rig: &Rig, id: &str) -> Credential {
    accounts::credential(&rig.store, &rig.api.keys, &account_scope(rig, id)).unwrap()
}

/// Ages the stored access token, so the next attempt has to refresh.
fn expire_token(rig: &Rig, id: &str) {
    let Credential::Oauth2 {
        provider,
        refresh_token,
        access_token,
        ..
    } = stored_tokens(rig, id)
    else {
        panic!("the consent stored tokens")
    };
    let stale = Credential::Oauth2 {
        provider,
        refresh_token,
        access_token,
        expires_at: 0,
    };
    accounts::update_credential(&rig.store, &rig.api.keys, &account_scope(rig, id), &stale)
        .unwrap();
}

async fn retry(rig: &Rig, cookie: &str, id: &str) -> (StatusCode, Value) {
    let request = with_cookie(Method::POST, &format!("/api/accounts/{id}/retry"), cookie);
    let response = rig.router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let text = body_text(response).await;
    (
        status,
        serde_json::from_str(&text).unwrap_or(Value::String(text)),
    )
}

fn xoauth2_lines(lines: &[String]) -> usize {
    lines.iter().filter(|line| line.contains("XOAUTH2")).count()
}

#[tokio::test]
async fn a_retry_refreshes_a_stale_token_and_signs_in_with_it() {
    let rig = rig().await;
    let cookie = rig.sign_in().await;
    let id = connected_account(&rig, &cookie).await;
    assert_eq!(rig.accounts(&cookie).await.len(), 1);
    expire_token(&rig, &id);
    let requests_before = rig.token_requests().len();
    let (status, row) = retry(&rig, &cookie, &id).await;
    assert_eq!(status, StatusCode::OK, "{row}");
    assert_eq!(row["address"], GMAIL_ADDRESS);
    assert_eq!(row["authMethod"], "oauth2");
    assert!(row["stoppedCause"].is_null());
    assert_eq!(rig.token_requests().len(), requests_before + 1);
    let forms = rig.forms.lock().unwrap();
    let last = forms.last().unwrap();
    assert_eq!(
        last.get("grant_type").map(String::as_str),
        Some("refresh_token")
    );
    assert_eq!(
        last.get("refresh_token").map(String::as_str),
        Some(REFRESH_TOKEN)
    );
    drop(forms);
    assert_eq!(xoauth2_lines(&rig.imap.lines()), 2);
    assert_eq!(xoauth2_lines(&rig.smtp.lines()), 2);
    let Credential::Oauth2 { expires_at, .. } = stored_tokens(&rig, &id) else {
        panic!("still tokens")
    };
    assert!(expires_at > 0);
}

#[tokio::test]
async fn a_refused_refresh_stops_the_account_and_a_broken_endpoint_counts_as_unreachable() {
    let refused = rig_answering(Answer::InvalidGrant).await;
    let cookie = refused.sign_in().await;
    let id = stale_gmail_account(&refused);
    let (status, body) = retry(&refused, &cookie, &id).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["cause"], "credentials");
    assert_eq!(xoauth2_lines(&refused.imap.lines()), 0);
    let broken = rig_answering(Answer::Broken).await;
    let cookie = broken.sign_in().await;
    let id = stale_gmail_account(&broken);
    let (status, body) = retry(&broken, &cookie, &id).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert_eq!(body["error"], "upstream_unreachable");
    assert_eq!(xoauth2_lines(&broken.imap.lines()), 0);
    let Credential::Oauth2 { access_token, .. } = stored_tokens(&broken, &id) else {
        panic!("still tokens")
    };
    assert_eq!(access_token, "ya29.stale");
}

#[tokio::test]
async fn a_consent_reconnect_resumes_a_connection_stop_too() {
    let rig = rig().await;
    let cookie = rig.sign_in().await;
    let id = connected_account(&rig, &cookie).await;
    let scope = account_scope(&rig, &id);
    accounts::stop(&rig.store, &scope, StopCause::Connection, &Actor::System).unwrap();
    let body = json!({ "provider": "gmail", "address": GMAIL_ADDRESS, "accountId": id });
    let (status, started) = rig.start_consent(&cookie, &body).await;
    assert_eq!(status, StatusCode::OK, "{started}");
    let state = started["state"].as_str().unwrap().to_owned();
    let query = google_query(&state);
    let (status, _page) = rig
        .callback(Visit {
            cookie: Some(&cookie),
            provider: "google",
            query: &query,
            language: None,
        })
        .await;
    assert_eq!(status, StatusCode::OK);
    let (_, outcome) = rig.pending(&cookie, &state).await;
    assert_eq!(outcome["status"], "done", "{outcome}");
    assert_eq!(outcome["accountId"], id);
    let rows = rig.accounts(&cookie).await;
    let row = rows.iter().find(|row| row["id"] == id).unwrap();
    assert!(row["stoppedCause"].is_null(), "{row}");
    let plain = scope::resolve(&rig.store, &rig.user_id(), None).unwrap();
    let tail: Vec<(String, String)> = events::for_organization(&rig.store, &plain)
        .unwrap()
        .into_iter()
        .rev()
        .take(2)
        .map(|record| (record.event_type, record.actor))
        .collect();
    let user = rig.user_id();
    assert_eq!(
        tail,
        [
            ("account.resumed".to_owned(), user.as_str().to_owned()),
            (
                "account.credentials_updated".to_owned(),
                user.as_str().to_owned()
            )
        ]
    );
}

#[tokio::test]
async fn another_user_reaches_neither_the_row_nor_its_tokens() {
    let rig = rig().await;
    let cookie = rig.sign_in().await;
    let id = connected_account(&rig, &cookie).await;
    let other = rig.sign_in_other().await;
    let (status, _) = retry(&rig, &other, &id).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let resolved = scope::resolve(
        &rig.store,
        &rig.other_user_id(),
        Some(&AccountId::from(id.clone())),
    );
    assert!(matches!(resolved, Err(StoreError::NotFound)));
    assert!(rig.accounts(&other).await.is_empty());
    // A plain router over the same store answers the same; the fakes see
    // nothing of it.
    let plain = router_on(Arc::clone(&rig.store));
    let request = with_cookie(Method::POST, &format!("/api/accounts/{id}/retry"), &other);
    let response = plain.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(xoauth2_lines(&rig.imap.lines()), 1);
}
