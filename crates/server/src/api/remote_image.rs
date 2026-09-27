// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The remote-image route: an image a message links, fetched on the
//! reader's behalf from a session's allowance and in one of the
//! process's slots, handed on with the headers every blob answer
//! carries.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::http::header::{CONTENT_LENGTH, CONTENT_TYPE};
use axum::response::Response;
use serde::Deserialize;

use super::{ApiError, ApiState, Full, internal, permit};
use crate::mail::download::blob_headers;
use crate::mail::remote::{self, REMOTE_ATTEMPT_TIMEOUT, RemoteError};
use crate::store::now_ms;

#[derive(Deserialize)]
pub(super) struct ImageQuery {
    url: String,
}

pub(super) async fn fetch(
    State(state): State<ApiState>,
    Full { session }: Full,
    query: Result<Query<ImageQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let Query(query) = query.map_err(|_| ApiError::InvalidRequest)?;
    let url = remote::checked(&query.url)?;
    state
        .remote_images
        .take(session.id.as_str(), now_ms())
        .map_err(|retry_after_ms| ApiError::RateLimited { retry_after_ms })?;
    // The slot is held while the image is fetched and buffered; the
    // wait for one is as long as one attempt.
    let _slot = permit(Arc::clone(&state.remote_fetches), REMOTE_ATTEMPT_TIMEOUT).await?;
    let image = remote::fetch(&state.upstream, url).await?;
    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, image.media_type)
        .header(CONTENT_LENGTH, image.bytes.len());
    for (header, value) in blob_headers(state.privacy_strict) {
        response = response.header(header, value);
    }
    response.body(Body::from(image.bytes)).map_err(internal)
}

impl From<RemoteError> for ApiError {
    fn from(error: RemoteError) -> Self {
        match error {
            RemoteError::Refused => Self::InvalidRequest,
            RemoteError::TooLarge => Self::TooLarge,
            RemoteError::NotAnImage => Self::NotAnImage,
            RemoteError::Unreachable => Self::UpstreamUnreachable,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_remote_error_has_one_api_word() {
        assert!(matches!(
            ApiError::from(RemoteError::Refused),
            ApiError::InvalidRequest
        ));
        assert!(matches!(
            ApiError::from(RemoteError::TooLarge),
            ApiError::TooLarge
        ));
        assert!(matches!(
            ApiError::from(RemoteError::NotAnImage),
            ApiError::NotAnImage
        ));
        assert!(matches!(
            ApiError::from(RemoteError::Unreachable),
            ApiError::UpstreamUnreachable
        ));
    }
}
