// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The retry and the credential replacement over HTTP: every refusal,
//! then the ways through.

mod answers;
mod common;
mod log_capture;
mod readers;
mod reconnect;
mod signin;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use huliho_imap_bridge::testing::{PASSWORD, TOKEN, imap};
use huliho_server::accounts::Credential;
use huliho_server::gate::MAX_REFUSED_RUN;
use log_capture::Capture;
use reconnect::{ADDRESS, Instance, password};
use serde_json::json;
use signin::{body_text, with_cookie};
use tower::ServiceExt;

/// Wrong candidates the limiter lets through before it starts to wait:
/// three free failures, then the fourth sets the first delay.
const FREE_CANDIDATES: usize = 4;

fn logins(instance: &Instance) -> usize {
    instance
        .imap
        .lines()
        .iter()
        .filter(|line| line.contains(" LOGIN "))
        .count()
}

fn stored_password(instance: &Instance, id: &str) -> Credential {
    instance.credential(id)
}

fn event(kind: &str, actor: &str) -> (String, String) {
    (kind.to_owned(), actor.to_owned())
}

#[tokio::test]
async fn another_users_account_is_not_found_on_both_routes_and_nothing_connects() {
    let instance = Instance::start(imap::Script::tls()).await;
    let id = instance.add_account(PASSWORD);
    let other = instance.sign_in_other().await;
    let (status, body) = instance.retry(&other, &id).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (status, body) = instance
        .put_credentials(&other, &id, &password(PASSWORD))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(logins(&instance), 0);
    assert_eq!(instance.stopped_cause(&id), None);
}

#[tokio::test]
async fn both_routes_need_a_session_and_the_header() {
    let instance = Instance::start(imap::Script::tls()).await;
    let id = instance.add_account(PASSWORD);
    for (method, action) in [(Method::POST, "retry"), (Method::PUT, "credentials")] {
        let uri = format!("/api/accounts/{id}/{action}");
        let stale = with_cookie(method.clone(), &uri, "huliho_session=stale");
        let response = instance.router.clone().oneshot(stale).await.unwrap();
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{method} {action}"
        );
    }
    let cookie = instance.sign_in().await;
    for (method, action) in [(Method::POST, "retry"), (Method::PUT, "credentials")] {
        let bare = Request::builder()
            .method(method.clone())
            .uri(format!("/api/accounts/{id}/{action}"))
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap();
        let response = instance.router.clone().oneshot(bare).await.unwrap();
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "{method} {action}"
        );
        assert!(body_text(response).await.contains("missing_csrf_header"));
    }
    assert_eq!(logins(&instance), 0);
}

#[tokio::test]
async fn a_candidate_of_the_wrong_kind_is_refused_before_any_connection() {
    let instance = Instance::start(imap::Script::tls()).await;
    let id = instance.add_account(PASSWORD);
    let cookie = instance.sign_in().await;
    let tokens = json!({
        "kind": "oauth2",
        "provider": "google",
        "refreshToken": "1//refresh",
        "accessToken": TOKEN,
        "expiresAt": 0,
    });
    let bearer = json!({ "kind": "bearer", "token": TOKEN });
    for candidate in [tokens, bearer, password("")] {
        let (status, body) = instance.put_credentials(&cookie, &id, &candidate).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["error"], "invalid_request");
    }
    assert_eq!(logins(&instance), 0);
    assert_eq!(
        stored_password(&instance, &id),
        Credential::Password {
            password: PASSWORD.to_owned()
        }
    );
}

#[tokio::test]
async fn a_wrong_candidate_leaves_the_row_and_the_blob_alone() {
    let instance = Instance::start(imap::Script::tls()).await;
    let id = instance.add_account(PASSWORD);
    let cookie = instance.sign_in().await;
    let (status, body) = instance
        .put_credentials(&cookie, &id, &password("wrong horse"))
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(body["error"], "upstream_credentials");
    assert_eq!(instance.stopped_cause(&id), None);
    assert_eq!(
        stored_password(&instance, &id),
        Credential::Password {
            password: PASSWORD.to_owned()
        }
    );
    assert_eq!(
        instance.account_events(),
        [event("account.linked", instance.user_id().as_str())]
    );
}

#[tokio::test]
async fn a_rejected_stored_credential_stops_after_exactly_one_attempt() {
    let instance = Instance::start(imap::Script::tls()).await;
    let id = instance.add_account("wrong horse");
    let cookie = instance.sign_in().await;
    let (status, body) = instance.retry(&cookie, &id).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], "still_stopped");
    assert_eq!(body["cause"], "credentials");
    assert_eq!(logins(&instance), 1);
    assert_eq!(instance.stopped_cause(&id).as_deref(), Some("credentials"));
    assert_eq!(
        instance.account_events().last(),
        Some(&event("account.stopped", instance.user_id().as_str()))
    );
    // A refused credential is not sent again; the user replaces it.
    let (status, body) = instance.retry(&cookie, &id).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["cause"], "credentials");
    assert_eq!(logins(&instance), 1);
}

#[tokio::test]
async fn five_refused_connections_stop_and_four_do_not() {
    let instance = Instance::start(imap::Script::tls()).await;
    let id = instance.add_account(PASSWORD);
    let cookie = instance.sign_in().await;
    instance.server_down();
    for _ in 1..MAX_REFUSED_RUN {
        let (status, body) = instance.retry(&cookie, &id).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
        assert_eq!(body["error"], "upstream_unreachable");
        assert_eq!(instance.stopped_cause(&id), None);
    }
    let (status, body) = instance.retry(&cookie, &id).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["cause"], "connection");
    assert_eq!(instance.stopped_cause(&id).as_deref(), Some("connection"));
    assert_eq!(
        instance.account_events().last(),
        Some(&event("account.stopped", "system"))
    );
    assert_eq!(logins(&instance), 0);
}

#[tokio::test]
async fn an_unavailable_backend_counts_as_a_connection_failure_not_a_verdict() {
    let instance = Instance::start(imap::Script {
        unavailable: true,
        ..imap::Script::tls()
    })
    .await;
    let id = instance.add_account(PASSWORD);
    let cookie = instance.sign_in().await;
    for _ in 1..MAX_REFUSED_RUN {
        let (status, body) = instance.retry(&cookie, &id).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
        assert_eq!(instance.stopped_cause(&id), None);
    }
    let (_, body) = instance.retry(&cookie, &id).await;
    assert_eq!(body["cause"], "connection", "{body}");
    assert_eq!(logins(&instance), usize::try_from(MAX_REFUSED_RUN).unwrap());
}

#[tokio::test]
async fn a_retry_resumes_a_stopped_account_at_once() {
    let instance = Instance::start(imap::Script::tls()).await;
    let id = instance.add_account(PASSWORD);
    let cookie = instance.sign_in().await;
    instance.server_down();
    for _ in 0..MAX_REFUSED_RUN {
        instance.retry(&cookie, &id).await;
    }
    assert_eq!(instance.stopped_cause(&id).as_deref(), Some("connection"));
    instance.server_up();
    let (status, row) = instance.retry(&cookie, &id).await;
    assert_eq!(status, StatusCode::OK, "{row}");
    assert_eq!(row["id"], id);
    assert!(row["stoppedCause"].is_null());
    assert!(row["stoppedAt"].is_null());
    assert_eq!(instance.stopped_cause(&id), None);
    assert_eq!(
        instance.account_events().last(),
        Some(&event("account.resumed", instance.user_id().as_str()))
    );
}

#[tokio::test]
async fn a_passing_candidate_replaces_the_blob_and_clears_a_credentials_stop() {
    let instance = Instance::start(imap::Script::tls()).await;
    let id = instance.add_account("wrong horse");
    let cookie = instance.sign_in().await;
    instance.retry(&cookie, &id).await;
    assert_eq!(instance.stopped_cause(&id).as_deref(), Some("credentials"));
    let (status, row) = instance
        .put_credentials(&cookie, &id, &password(PASSWORD))
        .await;
    assert_eq!(status, StatusCode::OK, "{row}");
    assert!(row["stoppedCause"].is_null());
    assert_eq!(row["authMethod"], "password");
    assert!(!row.to_string().contains(PASSWORD));
    assert_eq!(
        stored_password(&instance, &id),
        Credential::Password {
            password: PASSWORD.to_owned()
        }
    );
    let kinds: Vec<String> = instance
        .account_events()
        .into_iter()
        .map(|(kind, _)| kind)
        .collect();
    assert_eq!(
        kinds,
        [
            "account.linked",
            "account.stopped",
            "account.credentials_updated"
        ]
    );
}

#[tokio::test]
async fn a_candidate_credential_counts_against_the_limiter() {
    let instance = Instance::start(imap::Script::tls()).await;
    let id = instance.add_account(PASSWORD);
    let cookie = instance.sign_in().await;
    for _ in 0..FREE_CANDIDATES {
        let (status, body) = instance
            .put_credentials(&cookie, &id, &password("wrong horse"))
            .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    }
    let (status, body) = instance
        .put_credentials(&cookie, &id, &password("wrong horse"))
        .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert_eq!(body["error"], "rate_limited");
    assert_eq!(logins(&instance), FREE_CANDIDATES);
    // A retry carries no candidate, so the limiter does not hold it.
    let (status, row) = instance.retry(&cookie, &id).await;
    assert_eq!(status, StatusCode::OK, "{row}");
}

#[tokio::test]
async fn no_log_line_carries_the_address_the_host_or_a_secret() {
    let capture = Capture::install();
    let instance = Instance::start(imap::Script::tls()).await;
    let id = instance.add_account("wrong horse");
    let cookie = instance.sign_in().await;
    instance.retry(&cookie, &id).await;
    instance
        .put_credentials(&cookie, &id, &password(PASSWORD))
        .await;
    instance.server_down();
    for _ in 0..MAX_REFUSED_RUN {
        instance.retry(&cookie, &id).await;
    }
    instance.server_up();
    assert_eq!(instance.reconnect().probe_once().await, 1);
    let text = capture.text();
    assert!(text.contains("account stopped"), "{text}");
    assert!(text.contains("account resumed"), "{text}");
    for secret in ["wrong horse", PASSWORD, ADDRESS, imap::HOST] {
        assert!(!text.contains(secret), "{secret} in {text}");
    }
}
