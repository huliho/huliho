// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The JMAP proxy routes: an account's session object and its API
//! endpoint, both answered from the upstream with the account's
//! credential added here.

use axum::body::Bytes;
use axum::extract::{FromRequest, Path, Request, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;

use super::reconnect::scoped;
use super::{ApiError, ApiState, Caller};
use crate::events::Actor;
use crate::ids::AccountId;
use crate::jmap::{JSON, Proxy, is_json};

/// The media type of a problem details object (RFC 7807), the shape
/// RFC 8620 section 3.6.1 gives a request-level error.
const PROBLEM_JSON: &str = "application/problem+json";

/// The problem type of a request past one of the server's limits.
const LIMIT_PROBLEM: &str = "urn:ietf:params:jmap:error:limit";

/// The limit the per-account cap enforces, by its core capability name.
const CONCURRENCY_LIMIT: &str = "maxConcurrentRequests";

/// A Request object as bytes: JSON by content type and within the
/// route's body limit.
pub(super) struct JmapBody(Bytes);

impl FromRequest<ApiState> for JmapBody {
    type Rejection = Response;

    async fn from_request(request: Request, state: &ApiState) -> Result<Self, Response> {
        if !is_json(request.headers()) {
            return Err(ApiError::InvalidRequest.into_response());
        }
        Bytes::from_request(request, state)
            .await
            .map(Self)
            .map_err(IntoResponse::into_response)
    }
}

/// A problem details object for the one request-level error the proxy
/// raises itself.
#[derive(Serialize)]
struct Problem {
    #[serde(rename = "type")]
    kind: &'static str,
    status: u16,
    limit: &'static str,
    detail: &'static str,
}

pub(super) async fn session(
    State(state): State<ApiState>,
    caller: Caller,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let account_id = AccountId::from(id);
    let actor = Actor::User(caller.session.user_id.clone());
    let scope = scoped(&state, caller, &account_id).await?;
    let body = Proxy::from(&state).session(&scope, &actor).await?;
    Ok(json(body))
}

pub(super) async fn request(
    State(state): State<ApiState>,
    caller: Caller,
    Path(id): Path<String>,
    JmapBody(body): JmapBody,
) -> Result<Response, ApiError> {
    let account_id = AccountId::from(id);
    let actor = Actor::User(caller.session.user_id.clone());
    let scope = scoped(&state, caller, &account_id).await?;
    let Some(_permit) = state.endpoints.enter(&account_id) else {
        return Ok(over_the_cap());
    };
    let answer = Proxy::from(&state).forward(&scope, &actor, body).await?;
    Ok(json(answer))
}

fn json(body: Vec<u8>) -> Response {
    ([(header::CONTENT_TYPE, JSON)], body).into_response()
}

/// The `limit` error of RFC 8620 section 3.6.1: the request was not
/// run, the cap is named and nothing queues.
fn over_the_cap() -> Response {
    let problem = Problem {
        kind: LIMIT_PROBLEM,
        status: StatusCode::BAD_REQUEST.as_u16(),
        limit: CONCURRENCY_LIMIT,
        detail: "Too many requests are in flight on this account; send this one again once one of them answered.",
    };
    match serde_json::to_vec(&problem) {
        Ok(body) => (
            StatusCode::BAD_REQUEST,
            [(header::CONTENT_TYPE, PROBLEM_JSON)],
            body,
        )
            .into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cap_answers_the_limit_problem_rfc8620_3_6_1() {
        let response = over_the_cap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            PROBLEM_JSON
        );
    }
}
