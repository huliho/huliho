// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The instance's sign-in providers: the clients registered and
//! registering one. Instance admins only.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
use serde::{Deserialize, Serialize};

use super::{ApiError, ApiState, Caller, Full, internal, secret_fits};
use crate::providers::{self, OauthClient, OauthProvider, RegisteredClient};
use crate::scope;
use crate::session;

/// A registered client as the list shows it: never the secret.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ProviderView {
    provider: OauthProvider,
    client_id: String,
}

impl From<RegisteredClient> for ProviderView {
    fn from(client: RegisteredClient) -> Self {
        Self {
            provider: client.provider,
            client_id: client.id,
        }
    }
}

/// What the admin pastes from the provider's console.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RegisterRequest {
    client_id: String,
    client_secret: String,
}

pub(super) async fn list_providers(
    State(state): State<ApiState>,
    auth: Full,
) -> Result<Json<Vec<ProviderView>>, ApiError> {
    let store = Arc::clone(&state.store);
    let views = tokio::task::spawn_blocking(move || -> Result<Vec<ProviderView>, ApiError> {
        let scope = scope::resolve(&store, &auth.session.user_id, None)?;
        let clients = providers::list(&store, &scope)?;
        Ok(clients.into_iter().map(ProviderView::from).collect())
    })
    .await
    .map_err(internal)??;
    Ok(Json(views))
}

pub(super) async fn set_provider(
    State(state): State<ApiState>,
    caller: Caller,
    Path(provider): Path<String>,
    Json(request): Json<RegisterRequest>,
) -> Result<StatusCode, ApiError> {
    let provider = OauthProvider::from_word(&provider).ok_or(ApiError::NotFound)?;
    if !secret_fits(&request.client_id) || !secret_fits(&request.client_secret) {
        return Err(ApiError::InvalidRequest);
    }
    let client = OauthClient {
        provider,
        id: request.client_id,
        secret: request.client_secret,
    };
    let store = Arc::clone(&state.store);
    let keys = Arc::clone(&state.keys);
    tokio::task::spawn_blocking(move || -> Result<(), ApiError> {
        let scope = scope::resolve(&store, &caller.session.user_id, None)?;
        session::touch(&store, &scope, &caller.session, caller.client.address)?;
        providers::set_client(&store, &keys, &scope, &client)?;
        Ok(())
    })
    .await
    .map_err(internal)??;
    Ok(StatusCode::NO_CONTENT)
}
