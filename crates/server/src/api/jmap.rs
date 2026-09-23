// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The JMAP routes: an account's session object and its API endpoint.
//! A native account is answered from the upstream with the account's
//! credential added here; an IMAP account by the in-process bridge, so
//! the browser sees one surface.

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{FromRequest, Path, Request, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use huliho_imap_bridge::jmap::{RequestError, session_object};
use serde::Serialize;

use super::reconnect::scoped;
use super::{ApiError, ApiState, Caller, internal};
use crate::accounts::{self, Account, AccountKind};
use crate::bridge;
use crate::ids::AccountId;
use crate::jmap::{JSON, Proxy, is_json};
use crate::scope::Scope;

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

/// A problem details object for a request that was not run (RFC 8620
/// section 3.6.1).
#[derive(Serialize)]
struct Problem {
    #[serde(rename = "type")]
    kind: &'static str,
    status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    limit: Option<&'static str>,
    detail: String,
}

pub(super) async fn session(
    State(state): State<ApiState>,
    caller: Caller,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let account_id = AccountId::from(id);
    let scope = scoped(&state, caller, &account_id).await?;
    let account = running(&state, &scope).await?;
    let body = match account.kind {
        AccountKind::Jmap => Proxy::from(&state).session(account, &scope).await?,
        AccountKind::Imap => {
            let registration = bridge::registration(&account);
            state.bridge().start(&registration);
            session_object(&registration, &account.address, &bridge::urls(&account_id))
                .map_err(internal)?
        }
    };
    Ok(json(body))
}

pub(super) async fn request(
    State(state): State<ApiState>,
    caller: Caller,
    Path(id): Path<String>,
    JmapBody(body): JmapBody,
) -> Result<Response, ApiError> {
    let account_id = AccountId::from(id);
    let scope = scoped(&state, caller, &account_id).await?;
    let Some(_permit) = state.endpoints.enter(&account_id) else {
        return Ok(over_the_cap());
    };
    let account = running(&state, &scope).await?;
    let answer = match account.kind {
        AccountKind::Jmap => Proxy::from(&state).forward(account, &scope, body).await?,
        AccountKind::Imap => {
            let registration = bridge::registration(&account);
            match state.bridge().handle(&registration, &body).await {
                Ok(answer) => answer,
                Err(error) => return Ok(not_run(error)),
            }
        }
    };
    Ok(json(answer))
}

/// The row as it stands; a stopped account answers 409 with its cause
/// before anything connects, whatever its kind.
async fn running(state: &ApiState, scope: &Scope) -> Result<Account, ApiError> {
    let (store, scope) = (Arc::clone(&state.store), scope.clone());
    let account = tokio::task::spawn_blocking(move || accounts::get(&store, &scope))
        .await
        .map_err(internal)??;
    match account.stopped_cause {
        Some(cause) => Err(ApiError::StillStopped { cause }),
        None => Ok(account),
    }
}

fn json(body: Vec<u8>) -> Response {
    ([(header::CONTENT_TYPE, JSON)], body).into_response()
}

/// A problem details object with status 400.
fn problem(kind: &'static str, limit: Option<&'static str>, detail: String) -> Response {
    let problem = Problem {
        kind,
        status: StatusCode::BAD_REQUEST.as_u16(),
        limit,
        detail,
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

/// The `limit` error of RFC 8620 section 3.6.1: the request was not
/// run, the cap is named and nothing queues.
fn over_the_cap() -> Response {
    problem(
        LIMIT_PROBLEM,
        Some(CONCURRENCY_LIMIT),
        "Too many requests are in flight on this account; send this one again once one of them answered.".to_owned(),
    )
}

/// A request the bridge did not run: the problem it names, or 500 when
/// the store or its task failed.
fn not_run(error: RequestError) -> Response {
    match error.problem_type() {
        Some(kind) => problem(kind, error.limit(), error.to_string()),
        None => internal(error).into_response(),
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

    #[test]
    fn a_request_the_bridge_did_not_run_answers_its_problem_or_500() {
        let named = not_run(RequestError::Limit("maxCallsInRequest"));
        assert_eq!(named.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            named.headers().get(header::CONTENT_TYPE).unwrap(),
            PROBLEM_JSON
        );
        let failed = not_run(RequestError::Task);
        assert_eq!(failed.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
