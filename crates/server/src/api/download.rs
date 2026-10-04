// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The download route behind the rewritten `downloadUrl`: an account's
//! blob, from the upstream of a native account or through the bridge
//! of an IMAP account, streamed to the browser with the type read from
//! its first bytes, the disposition toward the download and the headers
//! that keep a blob from rendering anywhere but where the app puts it.

use std::time::Duration;

use axum::extract::{FromRequestParts, Path, Query, State};
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, RANGE};
use axum::http::request::Parts;
use axum::http::{HeaderValue, StatusCode};
use axum::response::Response;
use huliho_imap_bridge::blob::{Blob as BridgeBlob, BlobError as BridgeBlobError};
use serde::Deserialize;
use tokio::sync::OwnedSemaphorePermit;
use tokio::time::Instant;

use super::jmap::running;
use super::reconnect::scoped;
use super::{ApiError, ApiState, Caller, internal, permit};
use crate::accounts::{Account, AccountKind};
use crate::bridge;
use crate::ids::AccountId;
use crate::jmap::{BlobAsk, BlobError, Proxy};
use crate::mail::download::{
    BLOB_DOWNLOAD_LIMIT, DOWNLOAD_IDLE_TIMEOUT, DOWNLOAD_TOTAL_TIMEOUT, blob_headers, clean_name,
    disposition, range_cut, range_end, served_type,
};
use crate::mail::stream::{self, BOUNDS, Blob, Source};

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
    // A place in the account's download lane, waited for as long as a
    // stalled stream is waited for.
    let lane = permit(
        state.endpoints.downloads(&account_id),
        DOWNLOAD_IDLE_TIMEOUT,
    )
    .await?;
    let ask = BlobAsk {
        account_id: &path.account_id,
        blob_id: &path.blob_id,
        name: &path.name,
        media_type: wanted.media_type.as_deref().unwrap_or_default(),
    };
    let blob = match account.kind {
        AccountKind::Jmap => Proxy::from(&state).blob(account, &scope, &ask).await?,
        AccountKind::Imap => bridged(&state, &account, &path).await?,
    };
    let shape = Shape {
        name: &path.name,
        media_type: wanted.media_type.as_deref(),
        range_end: wanted.range_end,
        strict: state.privacy_strict,
    };
    respond(blob, &shape, lane)
}

/// A bridge account's blob with its first bytes in hand, refused before
/// a window is read where the server states more than the route
/// carries. The session object of such an account names one account,
/// the row's own id, so any other account id on the path finds nothing.
async fn bridged(state: &ApiState, account: &Account, path: &BlobPath) -> Result<Blob, ApiError> {
    if path.account_id != path.id {
        return Err(ApiError::NotFound);
    }
    let registration = bridge::registration(account);
    let bridge = state.bridge();
    let opened = Box::pin(bridge.blob(&registration, &path.blob_id, BLOB_DOWNLOAD_LIMIT));
    let source = within(DOWNLOAD_TOTAL_TIMEOUT, opened).await?;
    stream::open(source, &BOUNDS)
        .await
        .map_err(|_stopped| ApiError::UpstreamFailed)
}

/// The source of a bridge blob once the bridge opened it. The time a
/// download gets in all starts before the bridge is asked, so the wait
/// for the account's conversation counts.
async fn within<F>(total: Duration, opened: F) -> Result<Source, ApiError>
where
    F: Future<Output = Result<BridgeBlob, BridgeBlobError>>,
{
    let until = Instant::now() + total;
    let blob = tokio::time::timeout_at(until, opened)
        .await
        .map_err(|_elapsed| ApiError::UpstreamFailed)??;
    Ok(Source::Bridge { blob, until })
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

/// A server the bridge could not read answers in fixed words, never in
/// the server's own.
impl From<BridgeBlobError> for ApiError {
    fn from(error: BridgeBlobError) -> Self {
        match error {
            BridgeBlobError::NotFound => Self::NotFound,
            BridgeBlobError::TooLarge => Self::TooLarge,
            BridgeBlobError::Unavailable => Self::UpstreamFailed,
            BridgeBlobError::Undecodable | BridgeBlobError::Store(_) | BridgeBlobError::Task => {
                internal(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::mpsc;

    use super::*;
    use crate::gate::AttemptError;
    use crate::probe::ProbeError;

    /// What a scripted bridge download gets in all.
    const TOTAL: Duration = Duration::from_secs(10);

    /// How long the scripted bridge takes to open its blob.
    const WAIT: Duration = Duration::from_secs(3);

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

    #[test]
    fn every_bridge_blob_error_has_one_api_word() {
        assert!(matches!(
            ApiError::from(BridgeBlobError::NotFound),
            ApiError::NotFound
        ));
        assert!(matches!(
            ApiError::from(BridgeBlobError::TooLarge),
            ApiError::TooLarge
        ));
        assert!(matches!(
            ApiError::from(BridgeBlobError::Unavailable),
            ApiError::UpstreamFailed
        ));
        for fault in [BridgeBlobError::Undecodable, BridgeBlobError::Task] {
            assert!(matches!(ApiError::from(fault), ApiError::Internal));
        }
    }

    #[tokio::test(start_paused = true)]
    async fn the_total_of_a_bridge_download_starts_before_the_bridge_is_asked() {
        let started = Instant::now();
        let slow = |wait: Duration| async move {
            tokio::time::sleep(wait).await;
            Ok(BridgeBlob::scripted(mpsc::channel(1).1))
        };
        let source = within(TOTAL, slow(WAIT)).await;
        let Ok(Source::Bridge { until, .. }) = source else {
            panic!("the bridge opened no blob");
        };
        assert_eq!(until, started + TOTAL);
        // A bridge that answers past the total opens no download.
        let late = within(TOTAL, slow(TOTAL + WAIT)).await;
        assert!(matches!(late, Err(ApiError::UpstreamFailed)));
    }
}
