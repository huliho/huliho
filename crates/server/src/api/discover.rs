// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Finding the mail server for an address before anything is stored.

use std::sync::Arc;

use axum::extract::State;
use axum::response::Json;
use serde::{Deserialize, Serialize};

use super::{ApiError, ApiState, ClientInfo, Full, internal, upstream_keys};
use crate::accounts::{AccountKind, AccountSettings, Provider};
use crate::discovery::{self, Address, Budget, Discovered};
use crate::presets::{self, CredentialKind};
use crate::providers;
use crate::store::now_ms;

#[derive(Deserialize)]
pub(super) struct DiscoverRequest {
    address: String,
}

/// What discovery answers: the server it found or that it found none.
#[derive(Serialize)]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(super) enum DiscoveryView {
    Found {
        provider: Provider,
        kind: AccountKind,
        target: AccountSettings,
        credential_kind: CredentialKind,
        host: String,
        oauth_available: bool,
    },
    NotFound,
}

impl DiscoveryView {
    fn found(found: Discovered, oauth_available: bool) -> Self {
        let host = found.host().to_owned();
        Self::Found {
            provider: found.provider,
            kind: found.target.kind(),
            credential_kind: presets::for_provider(found.provider).credential_kind,
            target: found.target,
            host,
            oauth_available,
        }
    }
}

pub(super) async fn discover(
    State(state): State<ApiState>,
    client: ClientInfo,
    auth: Full,
    Json(request): Json<DiscoverRequest>,
) -> Result<Json<DiscoveryView>, ApiError> {
    let address = Address::parse(&request.address).map_err(|_| ApiError::InvalidRequest)?;
    let limiter_keys = upstream_keys(&auth.session.user_id, &client);
    let keys: Vec<&str> = limiter_keys.iter().map(String::as_str).collect();
    let now = now_ms();
    if let Some(retry_after_ms) = state.limiter.blocked_for(&keys, now) {
        return Err(ApiError::RateLimited { retry_after_ms });
    }
    // Every discovery counts, found or not, so nobody scans hosts through
    // the instance.
    state.limiter.record_failure(&keys, now);
    let Some(found) = discovery::discover(&state.upstream, &address, Budget::default()).await
    else {
        return Ok(Json(DiscoveryView::NotFound));
    };
    let oauth_available = oauth_available(&state, found.provider).await?;
    Ok(Json(DiscoveryView::found(found, oauth_available)))
}

/// True when the instance holds a client for the preset's sign-in
/// provider and knows its public URL, which the consent needs.
async fn oauth_available(state: &ApiState, provider: Provider) -> Result<bool, ApiError> {
    let Some(oauth) = presets::for_provider(provider).oauth else {
        return Ok(false);
    };
    if state.public_url.is_none() {
        return Ok(false);
    }
    let store = Arc::clone(&state.store);
    let registered = tokio::task::spawn_blocking(move || providers::is_registered(&store, oauth))
        .await
        .map_err(internal)??;
    Ok(registered)
}
