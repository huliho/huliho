// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The download route behind the rewritten `downloadUrl`: an account's
//! blob streamed to the browser with the type read from its first
//! bytes, the disposition toward the download and the headers that
//! keep a blob from rendering anywhere but where the app puts it.

use std::sync::Arc;

use axum::extract::{FromRequestParts, Path, Query, State};
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, RANGE};
use axum::http::request::Parts;
use axum::http::{HeaderValue, StatusCode};
use axum::response::Response;
use serde::Deserialize;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use super::jmap::running;
use super::reconnect::scoped;
use super::{ApiError, ApiState, Caller, internal};
use crate::accounts::AccountKind;
use crate::ids::AccountId;
use crate::jmap::{BlobAsk, BlobError, Proxy};
use crate::mail::download::{
    DOWNLOAD_IDLE_TIMEOUT, blob_headers, clean_name, disposition, range_cut, range_end, served_type,
};
use crate::mail::stream::{self, BOUNDS, Blob};

/// The path of the template the session object names.
#[derive(Deserialize)]
pub(super) struct BlobPath {
    id: String,
    account_id: String,
    blob_id: String,
    name: String,
}

#[derive(Deserialize)]
struct TypeQuery {
    #[serde(rename = "type")]
    media_type: Option<String>,
}

/// What the request asks beside the path: the type the client expects
/// and, from the inspector, a range from byte zero.
pub(super) struct Wanted {
    media_type: Option<String>,
    range_end: Option<u64>,
}

impl FromRequestParts<ApiState> for Wanted {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &ApiState) -> Result<Self, ApiError> {
        let Query(query) = Query::<TypeQuery>::from_request_parts(parts, state)
            .await
            .map_err(|_| ApiError::InvalidRequest)?;
        Ok(Self {
            media_type: query.media_type,
            range_end: range_end(parts.headers.get(RANGE)),
        })
    }
}

/// What shapes the answer beside the bytes.
struct Shape<'a> {
    name: &'a str,
    media_type: Option<&'a str>,
    range_end: Option<u64>,
    strict: bool,
}

pub(super) async fn download(
    State(state): State<ApiState>,
    caller: Caller,
    Path(path): Path<BlobPath>,
    wanted: Wanted,
) -> Result<Response, ApiError> {
    let account_id = AccountId::from(path.id.clone());
    let scope = scoped(&state, caller, &account_id).await?;
    let account = running(&state, &scope).await?;
    let lane = lane(state.endpoints.downloads(&account_id)).await?;
    let ask = BlobAsk {
        account_id: &path.account_id,
        blob_id: &path.blob_id,
        name: &path.name,
        media_type: wanted.media_type.as_deref().unwrap_or_default(),
    };
    let blob = match account.kind {
        AccountKind::Jmap => Proxy::from(&state).blob(account, &scope, &ask).await?,
        AccountKind::Imap => return Err(ApiError::UpstreamUnsupported),
    };
    let shape = Shape {
        name: &path.name,
        media_type: wanted.media_type.as_deref(),
        range_end: wanted.range_end,
        strict: state.privacy_strict,
    };
    respond(blob, &shape, lane)
}

/// A place in the account's download lane, waited for as long as a
/// stalled stream is waited for; past that the browser asks again.
async fn lane(downloads: Arc<Semaphore>) -> Result<OwnedSemaphorePermit, ApiError> {
    match tokio::time::timeout(DOWNLOAD_IDLE_TIMEOUT, downloads.acquire_owned()).await {
        Ok(Ok(permit)) => Ok(permit),
        Ok(Err(closed)) => Err(internal(closed)),
        Err(_elapsed) => Err(ApiError::RateLimited {
            retry_after_ms: i64::try_from(DOWNLOAD_IDLE_TIMEOUT.as_millis()).unwrap_or(i64::MAX),
        }),
    }
}

/// The blob as the browser gets it: typed by its first bytes, cut
/// where a range from zero ends, streamed under the bounds with the
/// lane held to the last byte.
fn respond(
    blob: Blob,
    shape: &Shape<'_>,
    lane: OwnedSemaphorePermit,
) -> Result<Response, ApiError> {
    let (content_type, inline) = served_type(&blob.head, shape.media_type);
    let cut = range_cut(blob.declared, shape.range_end);
    let mut response = Response::builder()
        .status(if cut.is_some() {
            StatusCode::PARTIAL_CONTENT
        } else {
            StatusCode::OK
        })
        .header(CONTENT_TYPE, content_type)
        .header(
            CONTENT_DISPOSITION,
            disposition(inline, &clean_name(shape.name)),
        );
    for (header, value) in blob_headers(shape.strict) {
        response = response.header(header, value);
    }
    if let Some(length) = cut.or(blob.declared) {
        response = response.header(CONTENT_LENGTH, length);
    }
    if let (Some(cut), Some(total)) = (cut, blob.declared) {
        let range = format!("bytes 0-{}/{total}", cut - 1);
        response = response.header(
            CONTENT_RANGE,
            HeaderValue::from_str(&range).map_err(internal)?,
        );
    }
    response
        .body(stream::body(blob, BOUNDS, cut, lane))
        .map_err(internal)
}

impl From<BlobError> for ApiError {
    fn from(error: BlobError) -> Self {
        match error {
            BlobError::NotFound => Self::NotFound,
            BlobError::TooLarge => Self::TooLarge,
            BlobError::Attempt(inner) => Self::from(inner),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gate::AttemptError;
    use crate::probe::ProbeError;

    #[test]
    fn every_blob_error_has_one_api_word() {
        assert!(matches!(
            ApiError::from(BlobError::NotFound),
            ApiError::NotFound
        ));
        assert!(matches!(
            ApiError::from(BlobError::TooLarge),
            ApiError::TooLarge
        ));
        let refused = BlobError::from(AttemptError::from(ProbeError::CredentialRejected));
        assert!(matches!(
            ApiError::from(refused),
            ApiError::UpstreamCredentials
        ));
        let failed = BlobError::from(AttemptError::Failed(StatusCode::BAD_GATEWAY));
        assert!(matches!(ApiError::from(failed), ApiError::UpstreamFailed));
        let cut = BlobError::from(AttemptError::from(ProbeError::Unreachable(String::new())));
        assert!(matches!(ApiError::from(cut), ApiError::UpstreamUnreachable));
    }

    #[tokio::test(start_paused = true)]
    async fn a_lane_is_taken_at_once_or_refused_after_the_wait_with_a_retry_after() {
        let free = Arc::new(Semaphore::new(1));
        let permit = lane(Arc::clone(&free)).await.unwrap();
        assert_eq!(free.available_permits(), 0);
        drop(permit);
        assert_eq!(free.available_permits(), 1);
        let taken = Arc::new(Semaphore::new(0));
        let refused = lane(taken).await.unwrap_err();
        let expected = i64::try_from(DOWNLOAD_IDLE_TIMEOUT.as_millis()).unwrap();
        assert!(matches!(
            refused,
            ApiError::RateLimited { retry_after_ms } if retry_after_ms == expected
        ));
    }
}
