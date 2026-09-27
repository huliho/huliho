// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The signed-in user's per-sender policies: one list, one row written
//! at a time and one row removed at a time. The sender is lowercased
//! here, so two spellings of one address share a row.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
use serde::Deserialize;

use super::{ApiError, ApiState, Caller, Full, internal};
use crate::prefs::{
    self, MAX_AUTHSERV_BYTES, MAX_SENDER_BYTES, PolicyKey, PolicyName, RemoteContentPolicy,
    SenderPolicy,
};
use crate::scope;
use crate::session;

/// What the client sends: the policy and its value for the sender in
/// the path.
#[derive(Deserialize)]
pub(super) struct PolicyRequest {
    key: PolicyName,
    value: RemoteContentPolicy,
}

pub(super) async fn list_policies(
    State(state): State<ApiState>,
    auth: Full,
) -> Result<Json<Vec<SenderPolicy>>, ApiError> {
    let store = Arc::clone(&state.store);
    let rows = tokio::task::spawn_blocking(move || -> Result<Vec<SenderPolicy>, ApiError> {
        let scope = scope::resolve(&store, &auth.session.user_id, None)?;
        Ok(prefs::sender_policies(&store, &scope)?)
    })
    .await
    .map_err(internal)??;
    Ok(Json(rows))
}

pub(super) async fn set_policy(
    State(state): State<ApiState>,
    caller: Caller,
    Path(sender): Path<String>,
    Json(request): Json<PolicyRequest>,
) -> Result<StatusCode, ApiError> {
    let sender = sender_key(&sender)?;
    if !fits(&request.value) {
        return Err(ApiError::InvalidRequest);
    }
    let store = Arc::clone(&state.store);
    tokio::task::spawn_blocking(move || -> Result<(), ApiError> {
        let scope = scope::resolve(&store, &caller.session.user_id, None)?;
        session::touch(&store, &scope, &caller.session, caller.client.address)?;
        let key = PolicyKey {
            sender: &sender,
            name: request.key.as_str(),
        };
        prefs::set_sender_policy(&store, &scope, key, &request.value)?;
        Ok(())
    })
    .await
    .map_err(internal)??;
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn remove_policy(
    State(state): State<ApiState>,
    caller: Caller,
    Path((sender, key)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let sender = sender_key(&sender)?;
    let name = PolicyName::from_word(&key).ok_or(ApiError::NotFound)?;
    let store = Arc::clone(&state.store);
    tokio::task::spawn_blocking(move || -> Result<(), ApiError> {
        let scope = scope::resolve(&store, &caller.session.user_id, None)?;
        session::touch(&store, &scope, &caller.session, caller.client.address)?;
        let key = PolicyKey {
            sender: &sender,
            name: name.as_str(),
        };
        prefs::remove_sender_policy(&store, &scope, key)?;
        Ok(())
    })
    .await
    .map_err(internal)??;
    Ok(StatusCode::NO_CONTENT)
}

/// The sender as the rows key it: lowercased, then bounded and
/// printable, since lowercasing can lengthen a string.
fn sender_key(sender: &str) -> Result<String, ApiError> {
    let key = sender.to_lowercase();
    if key.len() > MAX_SENDER_BYTES || key.chars().any(char::is_control) {
        return Err(ApiError::InvalidRequest);
    }
    Ok(key)
}

/// The one value a grant takes: an allow with a bounded authserv-id.
fn fits(value: &RemoteContentPolicy) -> bool {
    value.allow
        && value.authserv.as_ref().is_none_or(|authserv| {
            authserv.len() <= MAX_AUTHSERV_BYTES && !authserv.chars().any(char::is_control)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sender_is_lowercased_and_then_bounded() {
        assert_eq!(sender_key("News@Example.COM").unwrap(), "news@example.com");
        assert!(sender_key(&"a".repeat(MAX_SENDER_BYTES + 1)).is_err());
        assert!(sender_key("news@example.com\n").is_err());
        // A dotted capital I is two bytes and lowercases to three.
        let grows = |count: usize| format!("{}@x", "İ".repeat(count));
        assert!(grows(106).len() < MAX_SENDER_BYTES);
        assert_eq!(sender_key(&grows(106)).unwrap().len(), MAX_SENDER_BYTES);
        assert!(grows(107).len() < MAX_SENDER_BYTES);
        assert!(sender_key(&grows(107)).is_err());
    }

    #[test]
    fn a_grant_is_an_allow_with_a_bounded_authserv() {
        let grant = |allow: bool, authserv: Option<&str>| RemoteContentPolicy {
            allow,
            authserv: authserv.map(str::to_owned),
        };
        assert!(fits(&grant(true, None)));
        assert!(fits(&grant(true, Some("mx.google.com"))));
        assert!(!fits(&grant(false, None)));
        assert!(!fits(&grant(
            true,
            Some(&"a".repeat(MAX_AUTHSERV_BYTES + 1))
        )));
        assert!(!fits(&grant(true, Some("mx\r\n"))));
    }
}
