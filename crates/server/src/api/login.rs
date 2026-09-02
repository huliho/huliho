// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Sign-in, the current session and sign-out.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Json;
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use serde::{Deserialize, Serialize};
use time::Duration;

use super::{ApiError, ApiState, Authenticated, ClientInfo, internal, session_token};
use crate::auth::{self, LoginOutcome, MAX_PASSWORD_CHARS};
use crate::identity;
use crate::ids::{OrganizationId, Role, UserId};
use crate::scope;
use crate::secrets::SessionKeys;
use crate::session::{self, Client, SESSION_COOKIE, SessionTimeouts, device};
use crate::store::{Store, now_ms};

/// Longest login name: the 256-octet path of RFC 5321 section 4.5.3.1.3 minus its angle brackets.
const MAX_LOGIN_BYTES: usize = 254;

#[derive(Deserialize)]
pub(super) struct LoginRequest {
    login: String,
    password: String,
    /// Whether the client runs as an installed web app.
    #[serde(default)]
    installed: bool,
}

pub(super) async fn create_session(
    State(state): State<ApiState>,
    client: ClientInfo,
    jar: CookieJar,
    Json(request): Json<LoginRequest>,
) -> Result<(CookieJar, StatusCode), ApiError> {
    if request.login.is_empty()
        || request.login.len() > MAX_LOGIN_BYTES
        || request.password.chars().count() > MAX_PASSWORD_CHARS
    {
        return Err(ApiError::InvalidRequest);
    }
    let limiter_keys = [
        format!("login:{}", request.login),
        format!(
            "ip:{}",
            client
                .address
                .map_or("unknown".to_owned(), |address| address.to_string())
        ),
    ];
    let keys: Vec<&str> = limiter_keys.iter().map(String::as_str).collect();
    if let Some(retry_after_ms) = state.limiter.blocked_for(&keys, now_ms()) {
        return Err(ApiError::RateLimited { retry_after_ms });
    }
    let permit = state.verify_gate.acquire().await.map_err(internal)?;
    let store = Arc::clone(&state.store);
    let session_keys = Arc::clone(&state.keys);
    let session_client = Client {
        device: device::from_user_agent(&client.user_agent, request.installed),
        address: client.address,
    };
    let token = tokio::task::spawn_blocking(move || {
        attempt_login(&store, &session_keys, &request, &session_client)
    })
    .await
    .map_err(internal)??;
    drop(permit);
    if let Some(token) = token {
        state.limiter.record_success(&keys);
        let jar = jar.add(session_cookie(token, state.timeouts));
        Ok((jar, StatusCode::NO_CONTENT))
    } else {
        state.limiter.record_failure(&keys, now_ms());
        Err(ApiError::InvalidCredentials)
    }
}

fn attempt_login(
    store: &Store,
    keys: &SessionKeys,
    request: &LoginRequest,
    client: &Client,
) -> Result<Option<String>, ApiError> {
    match auth::verify_login(store, &request.login, &request.password)? {
        LoginOutcome::Verified(user_id) => {
            let token = session::create(store, keys, &user_id, client)?;
            Ok(Some(token))
        }
        LoginOutcome::Rejected(Some(user_id)) => {
            auth::record_login_failure(store, &user_id)?;
            Ok(None)
        }
        LoginOutcome::Rejected(None) => Ok(None),
    }
}

#[derive(Serialize)]
struct SessionUser {
    id: UserId,
    login: String,
    name: String,
    role: Role,
}

#[derive(Serialize)]
struct SessionOrganization {
    id: OrganizationId,
    name: String,
}

#[derive(Serialize)]
pub(super) struct SessionInfo {
    user: SessionUser,
    organization: SessionOrganization,
}

pub(super) async fn current_session(
    State(state): State<ApiState>,
    auth: Authenticated,
) -> Result<Json<SessionInfo>, ApiError> {
    let store = Arc::clone(&state.store);
    let info = tokio::task::spawn_blocking(move || -> Result<SessionInfo, ApiError> {
        let scope = scope::resolve(&store, &auth.session.user_id, None)?;
        let user = identity::user(&store, &scope)?;
        let organization = identity::organization(&store, &scope)?;
        Ok(SessionInfo {
            user: SessionUser {
                id: user.id,
                login: user.login,
                name: user.name,
                role: user.role,
            },
            organization: SessionOrganization {
                id: organization.id,
                name: organization.name,
            },
        })
    })
    .await
    .map_err(internal)??;
    Ok(Json(info))
}

pub(super) async fn delete_session(
    State(state): State<ApiState>,
    jar: CookieJar,
) -> Result<(CookieJar, StatusCode), ApiError> {
    if let Ok(token) = session_token(&jar) {
        let store = Arc::clone(&state.store);
        tokio::task::spawn_blocking(move || session::revoke(&store, &token))
            .await
            .map_err(internal)??;
    }
    let jar = jar.remove(Cookie::build(SESSION_COOKIE).path("/"));
    Ok((jar, StatusCode::NO_CONTENT))
}

fn session_cookie(token: String, timeouts: SessionTimeouts) -> Cookie<'static> {
    Cookie::build((SESSION_COOKIE, token))
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(Duration::milliseconds(timeouts.absolute_ms))
        .build()
}
