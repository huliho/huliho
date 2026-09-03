// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The signed-in user's session list and revocations.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;

use super::{ApiError, ApiState, ClientInfo, Full, internal};
use crate::ids::SessionId;
use crate::scope;
use crate::session::{self, SessionRow};

pub(super) async fn list_sessions(
    State(state): State<ApiState>,
    auth: Full,
) -> Result<Json<Vec<SessionRow>>, ApiError> {
    let store = Arc::clone(&state.store);
    let timeouts = state.timeouts;
    let rows = tokio::task::spawn_blocking(move || -> Result<Vec<SessionRow>, ApiError> {
        let scope = scope::resolve(&store, &auth.session.user_id, None)?;
        Ok(session::list(&store, &scope, &auth.session, timeouts)?)
    })
    .await
    .map_err(internal)??;
    Ok(Json(rows))
}

pub(super) async fn revoke_session(
    State(state): State<ApiState>,
    client: ClientInfo,
    auth: Full,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let store = Arc::clone(&state.store);
    tokio::task::spawn_blocking(move || -> Result<(), ApiError> {
        let scope = scope::resolve(&store, &auth.session.user_id, None)?;
        session::touch(&store, &scope, &auth.session, client.address)?;
        session::revoke_other(&store, &scope, &auth.session, &SessionId::from(id))?;
        Ok(())
    })
    .await
    .map_err(internal)??;
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn revoke_other_sessions(
    State(state): State<ApiState>,
    client: ClientInfo,
    auth: Full,
) -> Result<StatusCode, ApiError> {
    let store = Arc::clone(&state.store);
    tokio::task::spawn_blocking(move || -> Result<(), ApiError> {
        let scope = scope::resolve(&store, &auth.session.user_id, None)?;
        session::touch(&store, &scope, &auth.session, client.address)?;
        session::revoke_others(&store, &scope, &auth.session)?;
        Ok(())
    })
    .await
    .map_err(internal)??;
    Ok(StatusCode::NO_CONTENT)
}
