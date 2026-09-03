// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The admin users endpoints over HTTP.

mod common;
mod one_time;
mod signin;

use std::io::Write;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::http::{Method, StatusCode};
use serde::Deserialize;
use tower::ServiceExt;

use common::router_on;
use huliho_server::auth::{self, LoginOutcome};
use huliho_server::identity::{self, NewUser};
use huliho_server::ids::Role;
use huliho_server::scope;
use huliho_server::store::Store;
use one_time::{issued, json_with_cookie};
use signin::{
    LOGIN, PASSWORD, body_text, cookie_of, login_request, sign_in, store_with_account, with_cookie,
};

const MEMBER_LOGIN: &str = "jonas";
const NEW_PASSWORD: &str = "a brand new passphrase";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListedUser {
    id: String,
    name: String,
    login: String,
    role: String,
    last_active_at: Option<i64>,
}

/// The fixture owner plus a member with a chosen password.
fn store_with_member() -> Arc<Store> {
    let store = store_with_account();
    let LoginOutcome::Verified(owner_id) = auth::verify_login(&store, LOGIN, PASSWORD).unwrap()
    else {
        panic!("the fixture owner signs in");
    };
    let owner_scope = scope::resolve(&store, &owner_id, None).unwrap();
    let member = identity::create_organization_user(
        &store,
        &owner_scope,
        &NewUser {
            login: MEMBER_LOGIN.to_owned(),
            name: "Jonas Verhulst".to_owned(),
            role: Role::Member,
        },
    )
    .unwrap();
    auth::set_password(&store, &member.id, PASSWORD).unwrap();
    store
}

async fn listed(router: &Router, cookie: &str) -> Vec<ListedUser> {
    let response = router
        .clone()
        .oneshot(with_cookie(Method::GET, "/api/users", cookie))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    serde_json::from_str(&body_text(response).await).unwrap()
}

async fn id_of(router: &Router, cookie: &str, login: &str) -> String {
    listed(router, cookie)
        .await
        .into_iter()
        .find(|row| row.login == login)
        .unwrap()
        .id
}

async fn sign_in_with(router: &Router, login: &str, password: &str) -> String {
    let response = router
        .clone()
        .oneshot(login_request(login, password))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    cookie_of(&response)
}

async fn reset_status(router: &Router, cookie: &str, id: &str) -> StatusCode {
    router
        .clone()
        .oneshot(with_cookie(
            Method::POST,
            &format!("/api/users/{id}/password-reset"),
            cookie,
        ))
        .await
        .unwrap()
        .status()
}

async fn create(
    router: &Router,
    cookie: &str,
    body: &str,
) -> axum::http::Response<axum::body::Body> {
    router
        .clone()
        .oneshot(json_with_cookie(Method::POST, "/api/users", cookie, body))
        .await
        .unwrap()
}

#[tokio::test]
async fn the_owner_lists_both_users_with_names_and_activity() {
    let router = router_on(store_with_member());
    let owner = sign_in(&router).await;
    let rows = listed(&router, &owner).await;
    assert_eq!(rows.len(), 2);
    let member = rows.iter().find(|row| row.login == MEMBER_LOGIN).unwrap();
    assert_eq!(member.name, "Jonas Verhulst");
    assert_eq!(member.role, "member");
    assert_eq!(member.last_active_at, None);
    let owner_row = rows.iter().find(|row| row.login == LOGIN).unwrap();
    assert_eq!(owner_row.role, "owner");
    assert!(owner_row.last_active_at.is_some());
    assert!(!owner_row.id.is_empty());
}

#[tokio::test]
async fn a_member_sees_no_users_and_resets_nobody() {
    let router = router_on(store_with_member());
    let member = sign_in_with(&router, MEMBER_LOGIN, PASSWORD).await;
    let owner = sign_in(&router).await;
    let owner_id = id_of(&router, &owner, LOGIN).await;
    let response = router
        .clone()
        .oneshot(with_cookie(Method::GET, "/api/users", &member))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(body_text(response).await.contains("\"forbidden\""));
    let status = reset_status(&router, &member, &owner_id).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let body = "{\"name\":\"Eve\",\"login\":\"eve\",\"role\":\"member\"}";
    assert_eq!(
        create(&router, &member, body).await.status(),
        StatusCode::FORBIDDEN
    );
    let still = router
        .oneshot(with_cookie(Method::GET, "/api/session", &owner))
        .await
        .unwrap();
    assert_eq!(still.status(), StatusCode::OK);
}

#[tokio::test]
async fn a_reset_is_shown_once_and_ends_the_targets_sessions() {
    let router = router_on(store_with_member());
    let member_phone = sign_in_with(&router, MEMBER_LOGIN, PASSWORD).await;
    let owner = sign_in(&router).await;
    let member_id = id_of(&router, &owner, MEMBER_LOGIN).await;
    let response = router
        .clone()
        .oneshot(with_cookie(
            Method::POST,
            &format!("/api/users/{member_id}/password-reset"),
            &owner,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let issued = issued(response).await;
    assert_eq!(issued.one_time_password.len(), "k7fq-2mzp-x4rt".len());
    let status = router
        .clone()
        .oneshot(with_cookie(Method::GET, "/api/session", &member_phone))
        .await
        .unwrap()
        .status();
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let old = router
        .clone()
        .oneshot(login_request(MEMBER_LOGIN, PASSWORD))
        .await
        .unwrap();
    assert_eq!(old.status(), StatusCode::UNAUTHORIZED);
    let fresh = router
        .clone()
        .oneshot(login_request(MEMBER_LOGIN, &issued.one_time_password))
        .await
        .unwrap();
    assert_eq!(fresh.status(), StatusCode::NO_CONTENT);
    let spent = router
        .oneshot(login_request(MEMBER_LOGIN, &issued.one_time_password))
        .await
        .unwrap();
    assert_eq!(spent.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_own_row_a_strangers_row_and_an_unknown_row_are_refused() {
    let store = store_with_member();
    let (_, stranger) = identity::create_personal_user(&store, "stranger@example.com").unwrap();
    let router = router_on(store);
    let owner = sign_in(&router).await;
    let owner_id = id_of(&router, &owner, LOGIN).await;
    let own = reset_status(&router, &owner, &owner_id).await;
    assert_eq!(own, StatusCode::BAD_REQUEST);
    let outside = reset_status(&router, &owner, stranger.id.as_str()).await;
    assert_eq!(outside, StatusCode::NOT_FOUND);
    let unknown = reset_status(&router, &owner, "nobody").await;
    assert_eq!(unknown, StatusCode::NOT_FOUND);
    let same = router
        .oneshot(login_request(LOGIN, PASSWORD))
        .await
        .unwrap();
    assert_eq!(same.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn creating_a_user_hands_out_a_one_time_password_once() {
    let router = router_on(store_with_member());
    let owner = sign_in(&router).await;
    let body = "{\"name\":\" Tomas Lindqvist \",\"login\":\"tomas\",\"role\":\"admin\"}";
    let response = create(&router, &owner, body).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let text = body_text(response).await;
    assert!(text.contains("\"name\":\"Tomas Lindqvist\""));
    assert!(text.contains("\"login\":\"tomas\""));
    assert!(text.contains("\"role\":\"admin\""));
    assert!(text.contains("\"lastActiveAt\":null"));
    assert!(text.contains("\"oneTimePassword\":\""));
    assert_eq!(listed(&router, &owner).await.len(), 3);
    let taken = create(&router, &owner, body).await;
    assert_eq!(taken.status(), StatusCode::CONFLICT);
    assert!(body_text(taken).await.contains("login_taken"));
    for body in [
        "{\"name\":\" \",\"login\":\"eve\",\"role\":\"member\"}",
        "{\"name\":\"Eve\",\"login\":\"x y\",\"role\":\"member\"}",
        "{\"name\":\"Eve\",\"login\":\"\",\"role\":\"member\"}",
    ] {
        let status = create(&router, &owner, body).await.status();
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    }
    let unknown_role = "{\"name\":\"Eve\",\"login\":\"eve\",\"role\":\"root\"}";
    let status = create(&router, &owner, unknown_role).await.status();
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(listed(&router, &owner).await.len(), 3);
}

#[tokio::test]
async fn an_admin_grants_no_owner_role_and_resets_no_owner() {
    let router = router_on(store_with_member());
    let owner = sign_in(&router).await;
    let body = "{\"name\":\"Mireille Dekker\",\"login\":\"mireille\",\"role\":\"admin\"}";
    let secret = issued(create(&router, &owner, body).await)
        .await
        .one_time_password;
    let forced = sign_in_with(&router, "mireille", &secret).await;
    let chosen = router
        .clone()
        .oneshot(json_with_cookie(
            Method::PUT,
            "/api/password",
            &forced,
            &format!("{{\"new\":\"{NEW_PASSWORD}\"}}"),
        ))
        .await
        .unwrap();
    assert_eq!(chosen.status(), StatusCode::NO_CONTENT);
    let admin = cookie_of(&chosen);
    let upward = "{\"name\":\"Boss\",\"login\":\"boss\",\"role\":\"owner\"}";
    assert_eq!(
        create(&router, &admin, upward).await.status(),
        StatusCode::FORBIDDEN
    );
    let owner_id = id_of(&router, &admin, LOGIN).await;
    assert_eq!(
        reset_status(&router, &admin, &owner_id).await,
        StatusCode::FORBIDDEN
    );
    let member_id = id_of(&router, &admin, MEMBER_LOGIN).await;
    assert_eq!(
        reset_status(&router, &admin, &member_id).await,
        StatusCode::OK
    );
}

/// Collects everything the process logs, so a test can read it back.
#[derive(Clone)]
struct LogCapture(Arc<Mutex<Vec<u8>>>);

impl Write for LogCapture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn the_one_time_password_reaches_no_log_line() {
    let capture = LogCapture(Arc::new(Mutex::new(Vec::new())));
    let writer = capture.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || writer.clone())
        .finish();
    tracing::subscriber::set_global_default(subscriber).unwrap();
    let router = router_on(store_with_member());
    let owner = sign_in(&router).await;
    let member_id = id_of(&router, &owner, MEMBER_LOGIN).await;
    let response = router
        .clone()
        .oneshot(with_cookie(
            Method::POST,
            &format!("/api/users/{member_id}/password-reset"),
            &owner,
        ))
        .await
        .unwrap();
    let secret = issued(response).await.one_time_password;
    let body = "{\"name\":\"Tomas Lindqvist\",\"login\":\"tomas\",\"role\":\"member\"}";
    let created = issued(create(&router, &owner, body).await)
        .await
        .one_time_password;
    sign_in_with(&router, MEMBER_LOGIN, &secret).await;
    sign_in_with(&router, "tomas", &created).await;
    let logged = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
    assert!(logged.contains("password-reset"));
    for shown in [&secret, &created] {
        assert!(!logged.contains(shown.as_str()));
        assert!(!logged.contains(&shown.replace('-', "")));
    }
}
