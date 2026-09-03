// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Fixtures for tests that issue a one-time password over HTTP.

use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::{Method, Request, Response, header};
use serde::Deserialize;

use crate::signin::body_text;

/// The one-time password a create or reset answer carries.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Issued {
    pub one_time_password: String,
    pub expires_at: i64,
}

/// A JSON mutation carrying the session cookie.
pub fn json_with_cookie(method: Method, uri: &str, cookie: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::COOKIE, cookie)
        .header("x-requested-with", "huliho")
        .body(Body::from(body.to_owned()))
        .unwrap()
}

/// Reads the issued one-time password out of a create or reset answer
/// and checks that its expiry lies ahead.
pub async fn issued(response: Response<Body>) -> Issued {
    let issued: Issued = serde_json::from_str(&body_text(response).await).unwrap();
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since_epoch| i64::try_from(since_epoch.as_millis()).unwrap())
        .unwrap();
    assert!(issued.expires_at > now_ms);
    issued
}
