// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What the router answers a request with, read as JSON.

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::Value;
use tower::ServiceExt;

use crate::signin::body_text;

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
