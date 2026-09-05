// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The signed-in user's connected accounts: the list and the removal.

use std::num::NonZeroU32;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
use serde::Serialize;

use super::{ApiError, ApiState, ClientInfo, Full, internal};
use crate::accounts::{self, Account, AccountKind, AuthMethod, Provider, StopCause};
use crate::ids::AccountId;
use crate::scope;
use crate::session;

/// One account as the list shows it; never a credential, never the
/// connection settings.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AccountView {
    id: AccountId,
    address: String,
    name: String,
    provider: Provider,
    kind: AccountKind,
    auth_method: AuthMethod,
    stopped_cause: Option<StopCause>,
    stopped_at: Option<i64>,
    created_at: i64,
}

impl From<Account> for AccountView {
    fn from(account: Account) -> Self {
        Self {
            id: account.id,
            address: account.address,
            name: account.name,
            provider: account.provider,
            kind: account.kind,
            auth_method: account.auth_method,
            stopped_cause: account.stopped_cause,
            stopped_at: account.stopped_at,
            created_at: account.created_at,
        }
    }
}

/// The wire shape of `GET /accounts`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AccountList {
    accounts: Vec<AccountView>,
    /// How often a stopped account is checked, so the page can say so.
    probe_interval_minutes: NonZeroU32,
}

pub(super) async fn list_accounts(
    State(state): State<ApiState>,
    auth: Full,
) -> Result<Json<AccountList>, ApiError> {
    let store = Arc::clone(&state.store);
    let views = tokio::task::spawn_blocking(move || -> Result<Vec<AccountView>, ApiError> {
        let scope = scope::resolve(&store, &auth.session.user_id, None)?;
        let rows = accounts::list(&store, &scope)?;
        Ok(rows.into_iter().map(AccountView::from).collect())
    })
    .await
    .map_err(internal)??;
    Ok(Json(AccountList {
        accounts: views,
        probe_interval_minutes: state.probe_interval_minutes,
    }))
}

pub(super) async fn remove_account(
    State(state): State<ApiState>,
    client: ClientInfo,
    auth: Full,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let store = Arc::clone(&state.store);
    tokio::task::spawn_blocking(move || -> Result<(), ApiError> {
        let account_id = AccountId::from(id);
        let scope = scope::resolve(&store, &auth.session.user_id, Some(&account_id))?;
        session::touch(&store, &scope, &auth.session, client.address)?;
        accounts::remove(&store, &scope)?;
        Ok(())
    })
    .await
    .map_err(internal)??;
    Ok(StatusCode::NO_CONTENT)
}
