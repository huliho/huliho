// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The signed-in user's preferences: the listed keys as one object and
//! one word at a time.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
use serde::{Deserialize, Serialize};

use super::{ApiError, ApiState, Caller, Full, internal};
use crate::prefs::{self, PreferenceKey};
use crate::scope;
use crate::session;

/// The listed preferences; a key never written is left out.
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PreferencesView {
    #[serde(skip_serializing_if = "Option::is_none")]
    reading_pane: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    theme: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    density: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    locale: Option<String>,
}

impl PreferencesView {
    fn set(&mut self, key: PreferenceKey, word: String) {
        let slot = match key {
            PreferenceKey::ReadingPane => &mut self.reading_pane,
            PreferenceKey::Theme => &mut self.theme,
            PreferenceKey::Density => &mut self.density,
            PreferenceKey::Locale => &mut self.locale,
        };
        *slot = Some(word);
    }
}

/// What the client sends: the word for the key in the path.
#[derive(Deserialize)]
pub(super) struct PreferenceRequest {
    value: String,
}

pub(super) async fn list_preferences(
    State(state): State<ApiState>,
    auth: Full,
) -> Result<Json<PreferencesView>, ApiError> {
    let store = Arc::clone(&state.store);
    let view = tokio::task::spawn_blocking(move || -> Result<PreferencesView, ApiError> {
        let scope = scope::resolve(&store, &auth.session.user_id, None)?;
        let mut view = PreferencesView::default();
        for (key, word) in prefs::preferences(&store, &scope)? {
            view.set(key, word);
        }
        Ok(view)
    })
    .await
    .map_err(internal)??;
    Ok(Json(view))
}

pub(super) async fn set_preference(
    State(state): State<ApiState>,
    caller: Caller,
    Path(key): Path<String>,
    Json(request): Json<PreferenceRequest>,
) -> Result<StatusCode, ApiError> {
    let key = PreferenceKey::from_word(&key).ok_or(ApiError::NotFound)?;
    if !key.accepts(&request.value) {
        return Err(ApiError::InvalidRequest);
    }
    let store = Arc::clone(&state.store);
    tokio::task::spawn_blocking(move || -> Result<(), ApiError> {
        let scope = scope::resolve(&store, &caller.session.user_id, None)?;
        session::touch(&store, &scope, &caller.session, caller.client.address)?;
        prefs::set_preference(&store, &scope, key.as_str(), &request.value)?;
        Ok(())
    })
    .await
    .map_err(internal)??;
    Ok(StatusCode::NO_CONTENT)
}
