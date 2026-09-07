// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The consent routes over HTTP: what is refused before, at and after
//! the provider; nothing lands when it is.

mod common;
mod consent;
mod fake_dns;
mod signin;
mod tls_server;
mod token_endpoint;

use std::collections::HashMap;

use axum::body::Body;
use axum::http::{Method, StatusCode, header};
use common::{api_state, router_on, router_with};
use consent::{GMAIL_ADDRESS, Rig, Visit, gmail_start_body, google_query};
use huliho_imap_bridge::testing::{imap, smtp};
use huliho_server::accounts::{self, AccountSettings, Credential, NewAccount, Provider};
use huliho_server::api::ApiState;
use huliho_server::oauth::PKCE_MIN_LENGTH;
use huliho_server::scope;
use serde_json::{Value, json};
use signin::{body_text, sign_in, store_with_account, with_cookie};
use token_endpoint::{Answer, REFRESH_TOKEN};
use tower::ServiceExt;
use url::Url;

const START: &str = "/api/accounts/oauth/start";

fn tokens() -> Answer {
    Answer::Tokens {
        refresh: Some(REFRESH_TOKEN),
    }
}

async fn rig() -> Rig {
    Rig::start(tokens(), imap::Script::tls(), smtp::Script::tls()).await
}

/// A JMAP account with a token, the kind a consent never touches.
fn password_account() -> NewAccount {
    NewAccount {
        address: "mira@fastmail.com".to_owned(),
        name: "Fastmail".to_owned(),
        provider: Provider::Fastmail,
        settings: AccountSettings::Jmap {
            session_url: "https://api.fastmail.com/jmap/session".parse().unwrap(),
        },
        credential: Credential::Bearer {
            token: "fmu1-token".to_owned(),
        },
    }
}

/// A start request on a router of the caller's choosing.
fn start_request(cookie: &str, body: &Value) -> axum::http::Request<Body> {
    let mut request = with_cookie(Method::POST, START, cookie);
    request
        .headers_mut()
        .insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
    *request.body_mut() = Body::from(body.to_string());
    request
}

fn query_pairs(url: &str) -> HashMap<String, String> {
    Url::parse(url)
        .unwrap()
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect()
}

#[tokio::test]
async fn starting_needs_a_session_the_header_and_a_configured_instance() {
    let router = router_on(store_with_account());
    let response = router
        .clone()
        .oneshot(start_request("huliho_session=stale", &gmail_start_body()))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let cookie = sign_in(&router).await;
    let mut without_header = start_request(&cookie, &gmail_start_body());
    without_header.headers_mut().remove("x-requested-with");
    let response = router.clone().oneshot(without_header).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = router
        .clone()
        .oneshot(start_request(&cookie, &gmail_start_body()))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert!(
        body_text(response)
            .await
            .contains("provider_not_configured")
    );
    let with_url = router_with(ApiState {
        public_url: Some(Url::parse("https://mail.example.test").unwrap()),
        ..api_state(store_with_account())
    });
    let cookie = sign_in(&with_url).await;
    let response = with_url
        .oneshot(start_request(&cookie, &gmail_start_body()))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn a_start_is_refused_for_a_preset_without_a_provider_a_bad_address_or_a_foreign_account() {
    let rig = rig().await;
    let cookie = rig.sign_in().await;
    for (label, body) in [
        (
            "fastmail",
            json!({ "provider": "fastmail", "address": "mira@fastmail.com" }),
        ),
        (
            "generic",
            json!({ "provider": "generic", "address": "sanne@example.test" }),
        ),
        (
            "a bad address",
            json!({ "provider": "gmail", "address": "sanne" }),
        ),
    ] {
        let (status, body) = rig.start_consent(&cookie, &body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{label}: {body}");
        assert_eq!(body["error"], "invalid_request", "{label}");
    }
    let theirs = accounts::add(
        &rig.store,
        &rig.api.keys,
        &scope::resolve(&rig.store, &rig.other_user_id(), None).unwrap(),
        &password_account(),
    )
    .unwrap();
    let mut body = gmail_start_body();
    body["accountId"] = json!(theirs.id.as_str());
    let (status, _) = rig.start_consent(&cookie, &body).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let mine = accounts::add(
        &rig.store,
        &rig.api.keys,
        &scope::resolve(&rig.store, &rig.user_id(), None).unwrap(),
        &password_account(),
    )
    .unwrap();
    body["accountId"] = json!(mine.id.as_str());
    let (status, body) = rig.start_consent(&cookie, &body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let (status, _) = rig
        .start_consent(
            &cookie,
            &json!({ "provider": "yahoo", "address": GMAIL_ADDRESS }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = rig
        .start_consent(
            &cookie,
            &json!({ "provider": "aol", "address": GMAIL_ADDRESS }),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn the_consent_url_carries_pkce_the_state_the_scopes_and_the_hint() {
    let rig = rig().await;
    let cookie = rig.sign_in().await;
    let (status, started) = rig.start_consent(&cookie, &gmail_start_body()).await;
    assert_eq!(status, StatusCode::OK, "{started}");
    let state = started["state"].as_str().unwrap();
    assert!(!state.is_empty());
    let url = started["url"].as_str().unwrap();
    assert!(url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"));
    let pairs = query_pairs(url);
    assert_eq!(pairs["state"], state);
    assert_eq!(pairs["code_challenge_method"], "S256");
    assert!(pairs["code_challenge"].len() >= PKCE_MIN_LENGTH);
    assert_eq!(
        pairs["redirect_uri"],
        "https://mail.example.test/auth/google/callback"
    );
    assert_eq!(pairs["scope"], "https://mail.google.com/");
    assert_eq!(pairs["access_type"], "offline");
    assert_eq!(pairs["prompt"], "consent");
    assert_eq!(pairs["login_hint"], GMAIL_ADDRESS);
    assert!(!pairs.contains_key("client_secret"));
    let (status, started) = rig
        .start_consent(
            &cookie,
            &json!({ "provider": "microsoft", "address": "noor@outlook.com" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{started}");
    let pairs = query_pairs(started["url"].as_str().unwrap());
    assert!(
        pairs["scope"]
            .split(' ')
            .any(|scope| scope == "offline_access")
    );
    assert!(!pairs.contains_key("access_type"));
    let (status, pending) = rig.pending(&cookie, state).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(pending, json!({ "status": "pending" }));
}

#[tokio::test]
async fn the_pending_route_answers_the_owner_only() {
    let rig = rig().await;
    let cookie = rig.sign_in().await;
    let (_, started) = rig.start_consent(&cookie, &gmail_start_body()).await;
    let state = started["state"].as_str().unwrap();
    let other = rig.sign_in_other().await;
    let (status, _) = rig.pending(&other, state).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = rig.pending(&cookie, "no-such-state").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = rig.pending("huliho_session=stale", state).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_callback_from_another_session_or_without_one_is_refused_and_the_consent_stays() {
    let rig = rig().await;
    let cookie = rig.sign_in().await;
    let (_, started) = rig.start_consent(&cookie, &gmail_start_body()).await;
    let state = started["state"].as_str().unwrap();
    let query = google_query(state);
    let other = rig.sign_in_other().await;
    for (label, visit) in [
        (
            "another session",
            Visit {
                cookie: Some(&other),
                provider: "google",
                query: &query,
                language: None,
            },
        ),
        (
            "no session",
            Visit {
                cookie: None,
                provider: "google",
                query: &query,
                language: None,
            },
        ),
        (
            "an unknown provider word",
            Visit {
                cookie: Some(&cookie),
                provider: "yahoo",
                query: &query,
                language: None,
            },
        ),
        (
            "the other provider",
            Visit {
                cookie: Some(&cookie),
                provider: "microsoft",
                query: &query,
                language: None,
            },
        ),
        (
            "no state",
            Visit {
                cookie: Some(&cookie),
                provider: "google",
                query: "code=4%2Ffixture-code",
                language: None,
            },
        ),
    ] {
        let (status, body) = rig.callback(visit).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{label}");
        assert!(body.contains("You can close this window."), "{label}");
    }
    assert!(rig.forms.lock().unwrap().is_empty());
    assert!(rig.token_requests().is_empty());
    assert!(rig.imap.lines().is_empty());
    let (_, pending) = rig.pending(&cookie, state).await;
    assert_eq!(pending, json!({ "status": "pending" }));
    let denied = format!("error=access_denied&state={state}");
    let (status, body) = rig
        .callback(Visit {
            cookie: Some(&cookie),
            provider: "google",
            query: &denied,
            language: Some("nl-NL,nl;q=0.9"),
        })
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Je kunt dit venster sluiten."));
    assert!(body.contains("lang=\"nl\""));
    assert!(!body.contains("<script"));
    assert!(!body.contains("style="));
    let (_, pending) = rig.pending(&cookie, state).await;
    assert_eq!(
        pending,
        json!({ "status": "denied", "cause": "accessDenied" })
    );
    let (status, _) = rig
        .callback(Visit {
            cookie: Some(&cookie),
            provider: "google",
            query: &query,
            language: None,
        })
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(rig.forms.lock().unwrap().is_empty());
    assert!(rig.accounts(&cookie).await.is_empty());
}

#[tokio::test]
async fn a_failing_exchange_or_a_missing_refresh_token_denies_and_stores_nothing() {
    for (answer, cause) in [
        (Answer::Broken, "exchangeFailed"),
        (Answer::InvalidGrant, "exchangeFailed"),
        (Answer::Tokens { refresh: None }, "noRefreshToken"),
    ] {
        let rig = Rig::start(answer, imap::Script::tls(), smtp::Script::tls()).await;
        let cookie = rig.sign_in().await;
        let (_, started) = rig.start_consent(&cookie, &gmail_start_body()).await;
        let state = started["state"].as_str().unwrap();
        let query = google_query(state);
        let (status, _) = rig
            .callback(Visit {
                cookie: Some(&cookie),
                provider: "google",
                query: &query,
                language: None,
            })
            .await;
        assert_eq!(status, StatusCode::OK, "{cause}");
        let (_, pending) = rig.pending(&cookie, state).await;
        assert_eq!(
            pending,
            json!({ "status": "denied", "cause": cause }),
            "{cause}"
        );
        assert_eq!(rig.forms.lock().unwrap().len(), 1, "{cause}");
        assert!(rig.imap.lines().is_empty(), "{cause}");
        assert!(rig.smtp.lines().is_empty(), "{cause}");
        assert!(rig.accounts(&cookie).await.is_empty(), "{cause}");
    }
}

#[tokio::test]
async fn a_submission_server_without_auth_denies_with_the_outlook_word() {
    let rig = Rig::start(
        tokens(),
        imap::Script::tls(),
        smtp::Script {
            mechanisms: "",
            ..smtp::Script::tls()
        },
    )
    .await;
    let cookie = rig.sign_in().await;
    let (_, started) = rig.start_consent(&cookie, &gmail_start_body()).await;
    let state = started["state"].as_str().unwrap();
    let query = google_query(state);
    let (status, _) = rig
        .callback(Visit {
            cookie: Some(&cookie),
            provider: "google",
            query: &query,
            language: None,
        })
        .await;
    assert_eq!(status, StatusCode::OK);
    let (_, pending) = rig.pending(&cookie, state).await;
    assert_eq!(
        pending,
        json!({ "status": "denied", "cause": "smtpAuthUnavailable" })
    );
    assert!(
        rig.imap
            .lines()
            .iter()
            .any(|line| line.contains("AUTHENTICATE XOAUTH2"))
    );
    assert!(rig.smtp.lines().iter().any(|line| line.contains("EHLO")));
    assert!(rig.accounts(&cookie).await.is_empty());
}
