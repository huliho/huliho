// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The admin's users: listing, creating and resetting a password.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
use serde::{Deserialize, Serialize};

use super::login::MAX_LOGIN_BYTES;
use super::{ApiError, ApiState, ClientInfo, Full, internal};
use crate::auth::{self, OneTimePassword};
use crate::identity::{self, NewUser, User};
use crate::ids::{Role, UserId};
use crate::scope;
use crate::session;

/// A display name longer than this is a paragraph, not a name.
const MAX_NAME_CHARS: usize = 100;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct UserView {
    id: UserId,
    name: String,
    login: String,
    role: Role,
    last_active_at: Option<i64>,
}

impl From<User> for UserView {
    fn from(user: User) -> Self {
        Self {
            id: user.id,
            name: user.name,
            login: user.login,
            role: user.role,
            last_active_at: user.last_active_at,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct IssuedPassword {
    one_time_password: String,
    expires_at: i64,
}

impl From<OneTimePassword> for IssuedPassword {
    fn from(issued: OneTimePassword) -> Self {
        Self {
            one_time_password: issued.secret,
            expires_at: issued.expires_at,
        }
    }
}

#[derive(Serialize)]
pub(super) struct CreatedUser {
    user: UserView,
    #[serde(flatten)]
    password: IssuedPassword,
}

#[derive(Deserialize)]
pub(super) struct CreateRequest {
    name: String,
    login: String,
    role: Role,
}

pub(super) async fn list_users(
    State(state): State<ApiState>,
    auth: Full,
) -> Result<Json<Vec<UserView>>, ApiError> {
    let store = Arc::clone(&state.store);
    let views = tokio::task::spawn_blocking(move || -> Result<Vec<UserView>, ApiError> {
        let scope = scope::resolve(&store, &auth.session.user_id, None)?;
        let users = identity::users(&store, &scope)?;
        Ok(users.into_iter().map(UserView::from).collect())
    })
    .await
    .map_err(internal)??;
    Ok(Json(views))
}

pub(super) async fn create_user(
    State(state): State<ApiState>,
    client: ClientInfo,
    auth: Full,
    Json(request): Json<CreateRequest>,
) -> Result<(StatusCode, Json<CreatedUser>), ApiError> {
    let new = new_user(request)?;
    let permit = state.verify_gate.acquire().await.map_err(internal)?;
    let store = Arc::clone(&state.store);
    let created = tokio::task::spawn_blocking(move || -> Result<CreatedUser, ApiError> {
        let scope = scope::resolve(&store, &auth.session.user_id, None)?;
        session::touch(&store, &scope, &auth.session, client.address)?;
        let (user, issued) = auth::create_user(&store, &scope, &new)?;
        Ok(CreatedUser {
            user: UserView::from(user),
            password: IssuedPassword::from(issued),
        })
    })
    .await
    .map_err(internal)??;
    drop(permit);
    Ok((StatusCode::CREATED, Json(created)))
}

pub(super) async fn reset_password(
    State(state): State<ApiState>,
    client: ClientInfo,
    auth: Full,
    Path(id): Path<String>,
) -> Result<Json<IssuedPassword>, ApiError> {
    let permit = state.verify_gate.acquire().await.map_err(internal)?;
    let store = Arc::clone(&state.store);
    let issued = tokio::task::spawn_blocking(move || -> Result<IssuedPassword, ApiError> {
        let scope = scope::resolve(&store, &auth.session.user_id, None)?;
        session::touch(&store, &scope, &auth.session, client.address)?;
        let issued = auth::reset_password(&store, &scope, &UserId::from(id))?;
        Ok(IssuedPassword::from(issued))
    })
    .await
    .map_err(internal)??;
    drop(permit);
    Ok(Json(issued))
}

/// A name is trimmed and bounded; a sign-in name is bounded and carries
/// no whitespace.
fn new_user(request: CreateRequest) -> Result<NewUser, ApiError> {
    let name = request.name.trim();
    let name_fits = !name.is_empty() && name.chars().count() <= MAX_NAME_CHARS;
    let login_fits = !request.login.is_empty()
        && request.login.len() <= MAX_LOGIN_BYTES
        && !request.login.contains(char::is_whitespace);
    if !name_fits || !login_fits {
        return Err(ApiError::InvalidRequest);
    }
    Ok(NewUser {
        login: request.login,
        name: name.to_owned(),
        role: request.role,
    })
}
