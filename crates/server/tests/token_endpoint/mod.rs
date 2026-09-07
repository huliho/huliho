// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A token endpoint for tests: the fixture tokens for the fixture code
//! or refresh token, every form it receives on record.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::{Form, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use axum::routing::post;
use huliho_imap_bridge::testing::TOKEN;
use serde_json::json;

pub const CODE: &str = "4/fixture-code";
pub const REFRESH_TOKEN: &str = "1//fixture-refresh";
pub const CLIENT_ID: &str = "fixture-client-id.apps.googleusercontent.com";
pub const CLIENT_SECRET: &str = "GOCSPX-fixture-secret";
/// An hour, as Google grants it.
const EXPIRES_IN_SECONDS: u64 = 3600;

/// What the endpoint answers a well-formed request.
#[derive(Clone, Copy)]
pub enum Answer {
    /// Tokens, with the refresh token named or none at all.
    Tokens { refresh: Option<&'static str> },
    /// `invalid_grant`, as for a withdrawn consent.
    InvalidGrant,
    /// A broken endpoint: 500 with a text body.
    Broken,
}

/// Every form the endpoint received, oldest first.
pub type Forms = Arc<Mutex<Vec<HashMap<String, String>>>>;

#[derive(Clone)]
struct Endpoint {
    answer: Answer,
    forms: Forms,
}

/// `POST /token` on the given behavior; every form lands in `forms`.
pub fn routes(answer: Answer, forms: Forms) -> Router {
    Router::new()
        .route("/token", post(token))
        .with_state(Endpoint { answer, forms })
}

async fn token(
    State(endpoint): State<Endpoint>,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    let client_ok = form.get("client_id").is_some_and(|id| id == CLIENT_ID)
        && form
            .get("client_secret")
            .is_some_and(|secret| secret == CLIENT_SECRET);
    let granted = client_ok
        && match form.get("grant_type").map(String::as_str) {
            Some("authorization_code") => {
                form.get("code").is_some_and(|code| code == CODE)
                    && form.contains_key("code_verifier")
                    && form.contains_key("redirect_uri")
            }
            Some("refresh_token") => form
                .get("refresh_token")
                .is_some_and(|token| token == REFRESH_TOKEN),
            _ => false,
        };
    endpoint.forms.lock().unwrap().push(form);
    match (endpoint.answer, granted) {
        (Answer::Broken, _) => (StatusCode::INTERNAL_SERVER_ERROR, "no").into_response(),
        (Answer::InvalidGrant, _) | (Answer::Tokens { .. }, false) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid_grant", "error_description": "Bad Request" })),
        )
            .into_response(),
        (Answer::Tokens { refresh }, true) => {
            let mut body = json!({
                "access_token": TOKEN,
                "token_type": "Bearer",
                "expires_in": EXPIRES_IN_SECONDS,
                "scope": "https://mail.google.com/",
            });
            if let Some(refresh) = refresh {
                body["refresh_token"] = json!(refresh);
            }
            Json(body).into_response()
        }
    }
}
