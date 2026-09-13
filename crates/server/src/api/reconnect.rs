// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What a user does with a stopped account: retry the stored credential
//! now or replace it after a passing check.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::response::Json;
use serde::Deserialize;

use super::accounts::{AccountView, credential_fits};
use super::{ApiError, ApiState, Caller, internal, upstream_keys};
use crate::accounts::{self, Account, AccountSettings, Credential};
use crate::discovery::Address;
use crate::events::Actor;
use crate::gate::{AttemptError, Reconnect};
use crate::ids::AccountId;
use crate::probe::{Probe, ProbeError};
use crate::scope::{self, Scope};
use crate::session;
use crate::store::now_ms;

/// The candidate credential the user typed.
#[derive(Deserialize)]
pub(super) struct CredentialRequest {
    credential: Credential,
}

pub(super) async fn retry_account(
    State(state): State<ApiState>,
    caller: Caller,
    Path(id): Path<String>,
) -> Result<Json<AccountView>, ApiError> {
    let account_id = AccountId::from(id);
    let actor = Actor::User(caller.session.user_id.clone());
    let scope = scoped(&state, caller, &account_id).await?;
    match Reconnect::from(&state).retry(&scope, &actor).await {
        Ok(account) => Ok(Json(AccountView::from(account))),
        Err(AttemptError::Upstream(error)) => Err(refused(&state, &scope, error).await),
        Err(AttemptError::Store(error)) => Err(error.into()),
        Err(AttemptError::Task) => Err(internal(AttemptError::Task)),
    }
}

pub(super) async fn replace_credentials(
    State(state): State<ApiState>,
    caller: Caller,
    Path(id): Path<String>,
    Json(request): Json<CredentialRequest>,
) -> Result<Json<AccountView>, ApiError> {
    let account_id = AccountId::from(id);
    let limiter_keys = upstream_keys(&caller.session.user_id, &caller.client);
    let keys: Vec<&str> = limiter_keys.iter().map(String::as_str).collect();
    let now = now_ms();
    if let Some(retry_after_ms) = state.limiter.blocked_for(&keys, now) {
        return Err(ApiError::RateLimited { retry_after_ms });
    }
    let actor = Actor::User(caller.session.user_id.clone());
    let scope = scoped(&state, caller, &account_id).await?;
    let _held = state.gate.hold(&account_id).await;
    let (address, settings) = target_of(&state, &scope).await?;
    if !credential_fits(&request.credential, &settings) {
        return Err(ApiError::InvalidRequest);
    }
    // A candidate is checked like a new one; the row's own credential is
    // not judged here, so the gate stays out until the pass is written.
    state.limiter.record_failure(&keys, now);
    Probe::new(Arc::clone(&state.upstream))
        .check(&address, &settings, &request.credential)
        .await?;
    state.limiter.record_success(&keys);
    let (store, sealing, gate) = (
        Arc::clone(&state.store),
        Arc::clone(&state.keys),
        state.gate.clone(),
    );
    let account = tokio::task::spawn_blocking(move || -> Result<Account, ApiError> {
        accounts::replace_credential(&store, &sealing, &scope, &request.credential)?;
        Ok(gate.passed(&scope, &actor)?)
    })
    .await
    .map_err(internal)??;
    Ok(Json(AccountView::from(account)))
}

/// The account within the caller's scope, the session touched; another
/// user's id is not found.
async fn scoped(
    state: &ApiState,
    caller: Caller,
    account_id: &AccountId,
) -> Result<Scope, ApiError> {
    let store = Arc::clone(&state.store);
    let account_id = account_id.clone();
    tokio::task::spawn_blocking(move || -> Result<Scope, ApiError> {
        let scope = scope::resolve(&store, &caller.session.user_id, Some(&account_id))?;
        session::touch(&store, &scope, &caller.session, caller.client.address)?;
        Ok(scope)
    })
    .await
    .map_err(internal)?
}

/// The address and the target the row connects with; a row whose address
/// does not parse is not usable.
async fn target_of(
    state: &ApiState,
    scope: &Scope,
) -> Result<(Address, AccountSettings), ApiError> {
    let (store, scope) = (Arc::clone(&state.store), scope.clone());
    tokio::task::spawn_blocking(move || -> Result<(Address, AccountSettings), ApiError> {
        let account = accounts::get(&store, &scope)?;
        let address =
            Address::parse(&account.address).map_err(|_| ApiError::UpstreamUnsupported)?;
        Ok((address, accounts::settings(&store, &scope)?))
    })
    .await
    .map_err(internal)?
}

/// A failed retry: a stopped row answers 409 with its cause, whatever
/// landed the stop; a row still running answers the upstream's own word.
async fn refused(state: &ApiState, scope: &Scope, error: ProbeError) -> ApiError {
    let (store, scope) = (Arc::clone(&state.store), scope.clone());
    match tokio::task::spawn_blocking(move || accounts::get(&store, &scope)).await {
        Ok(Ok(Account {
            stopped_cause: Some(cause),
            ..
        })) => ApiError::StillStopped { cause },
        Ok(Ok(_)) => ApiError::from(error),
        Ok(Err(store_error)) => ApiError::from(store_error),
        Err(join) => internal(join),
    }
}
