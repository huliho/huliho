// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What a rig reads back after a request: the row's stop cause, the
//! account events and a JSON answer.

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use huliho_server::accounts;
use huliho_server::events;
use huliho_server::scope::Scope;
use huliho_server::store::Store;
use serde_json::Value;
use tower::ServiceExt;

use crate::signin::body_text;

/// The row's stop cause word; `None` while it runs.
pub fn stopped_cause(store: &Store, scope: &Scope) -> Option<String> {
    accounts::get(store, scope)
        .unwrap()
        .stopped_cause
        .map(|cause| cause.as_str().to_owned())
}

/// The account events as `(type, actor)`, oldest first.
pub fn account_events(store: &Store, scope: &Scope) -> Vec<(String, String)> {
    events::for_organization(store, scope)
        .unwrap()
        .into_iter()
        .filter(|record| record.event_type.starts_with("account."))
        .map(|record| (record.event_type, record.actor))
        .collect()
}

/// One request through the router: its status and its body as JSON
/// where the body parses, as a string otherwise.
pub async fn answer(router: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let text = body_text(response).await;
    (
        status,
        serde_json::from_str(&text).unwrap_or(Value::String(text)),
    )
}
