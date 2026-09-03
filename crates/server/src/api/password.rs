// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The signed-in user's password change, forced or chosen.

use std::net::IpAddr;
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Json;
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;

use super::login::session_cookie;
use super::{ApiError, ApiState, Authenticated, ClientInfo, internal};
use crate::auth::{self, MAX_PASSWORD_CHARS};
use crate::scope;
use crate::secrets::SessionKeys;
use crate::session::{self, PasswordChange, Session};
use crate::store::{Store, now_ms};

#[derive(Deserialize)]
pub(super) struct PasswordRequest {
    current: Option<String>,
    new: String,
}

/// What the blocking step needs: the required-or-skipped current
/// password, the chosen one and where the change comes from.
struct Change {
    current: Option<String>,
    new: String,
    address: Option<IpAddr>,
}

pub(super) async fn change_password(
    State(state): State<ApiState>,
    client: ClientInfo,
    auth: Authenticated,
    Json(request): Json<PasswordRequest>,
) -> Result<(CookieJar, StatusCode), ApiError> {
    auth::check_length(&request.new)?;
    let current = current_password(&auth.session, request.current)?;
    let limiter_keys = [
        format!("password:{}", auth.session.user_id.as_str()),
        client.address_key(),
    ];
    let keys: Vec<&str> = limiter_keys.iter().map(String::as_str).collect();
    if let Some(retry_after_ms) = state.limiter.blocked_for(&keys, now_ms()) {
        return Err(ApiError::RateLimited { retry_after_ms });
    }
    let permit = state.verify_gate.acquire().await.map_err(internal)?;
    let store = Arc::clone(&state.store);
    let session_keys = Arc::clone(&state.keys);
    let change = Change {
        current,
        new: request.new,
        address: client.address,
    };
    let token = tokio::task::spawn_blocking(move || {
        attempt_change(&store, &session_keys, &auth.session, &change)
    })
    .await
    .map_err(internal)??;
    drop(permit);
    let Some(token) = token else {
        state.limiter.record_failure(&keys, now_ms());
        return Err(ApiError::InvalidCredentials);
    };
    state.limiter.record_success(&keys);
    let jar = CookieJar::new().add(session_cookie(token, state.timeouts));
    Ok((jar, StatusCode::NO_CONTENT))
}

/// The current password the change verifies: none in the forced step,
/// required otherwise and bounded before it reaches the hasher.
fn current_password(
    session: &Session,
    current: Option<String>,
) -> Result<Option<String>, ApiError> {
    match (session.password_change_required, current) {
        (true, _) => Ok(None),
        (false, Some(current)) if current.chars().count() <= MAX_PASSWORD_CHARS => {
            Ok(Some(current))
        }
        (false, _) => Err(ApiError::InvalidRequest),
    }
}

fn attempt_change(
    store: &Store,
    keys: &SessionKeys,
    session: &Session,
    change: &Change,
) -> Result<Option<String>, ApiError> {
    let scope = scope::resolve(store, &session.user_id, None)?;
    if let Some(current) = &change.current
        && !auth::verify_password(store, &scope, current)?
    {
        return Ok(None);
    }
    let applied = PasswordChange {
        current: session,
        password_hash: auth::hash_password(&change.new)?,
        address: change.address,
    };
    let token = session::apply_password_change(store, keys, &scope, &applied)?;
    Ok(Some(token))
}
