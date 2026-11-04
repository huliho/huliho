// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The upload route behind the rewritten `uploadUrl` (RFC 8620 section
//! 6.1): a file the browser sends before a draft names it, streamed to
//! the upstream of a native account or into the bridge's sealed store
//! for an IMAP account, never held whole here.

use axum::extract::{Path, Request, State};
use axum::http::header::CONTENT_TYPE;
use axum::response::{IntoResponse, Response};
use huliho_imap_bridge::upload::{MAX_ATTACHMENTS_BYTES, UploadError};
use serde::Deserialize;
use serde_json::json;

use super::jmap::running;
use super::reconnect::scoped;
use super::{ApiError, ApiState, Caller, internal, permit};
use crate::accounts::AccountKind;
use crate::bridge;
use crate::ids::AccountId;
use crate::jmap::{ForwardError, JSON, Proxy, UploadAsk};
use crate::mail::upload::{
    Cut, Incoming, UPLOAD_IDLE_TIMEOUT, UPLOAD_LIMIT, declared_length, declared_type,
};

/// The path of the template the session object names.
#[derive(Deserialize)]
pub(super) struct UploadPath {
    id: String,
    account_id: String,
}

pub(super) async fn upload(
    State(state): State<ApiState>,
    caller: Caller,
    Path(path): Path<UploadPath>,
    request: Request,
) -> Result<Response, ApiError> {
    let account_id = AccountId::from(path.id.clone());
    let scope = scoped(&state, caller, &account_id).await?;
    let account = running(&state, &scope).await?;
    let media_type = declared_type(request.headers()).ok_or(ApiError::InvalidRequest)?;
    // The bridge's store holds less than the route carries.
    let bound = match account.kind {
        AccountKind::Jmap => UPLOAD_LIMIT,
        AccountKind::Imap => MAX_ATTACHMENTS_BYTES,
    };
    let length = declared_length(request.headers());
    if length.is_some_and(|length| length > bound) {
        return Err(ApiError::TooLarge);
    }
    // A place in the account's upload lane, waited for as long as a
    // stalled body is waited for.
    let _lane = permit(state.endpoints.uploads(&account_id), UPLOAD_IDLE_TIMEOUT).await?;
    let incoming = Incoming::new(request.into_body(), bound);
    let answer = match account.kind {
        AccountKind::Jmap => {
            let ask = UploadAsk {
                account_id: &path.account_id,
                media_type: &media_type,
                length,
                body: incoming,
            };
            Proxy::from(&state).upload(account, &scope, ask).await?
        }
        AccountKind::Imap => stored(&state, &path, &media_type, incoming).await?,
    };
    Ok(([(CONTENT_TYPE, JSON)], answer).into_response())
}

/// A bridge account's upload, written into its store as it arrives. The
/// session object of such an account names one account, the row's own
/// id, so any other account id on the path finds nothing.
async fn stored(
    state: &ApiState,
    path: &UploadPath,
    media_type: &str,
    mut incoming: Incoming,
) -> Result<Vec<u8>, ApiError> {
    if path.account_id != path.id {
        return Err(ApiError::NotFound);
    }
    let key = bridge::key_of(&AccountId::from(path.id.clone()));
    let mut writing = state.bridge().upload(&key, media_type).await?;
    loop {
        match incoming.next().await {
            Ok(Some(chunk)) => writing.push(&chunk).await?,
            Ok(None) => break,
            Err(cut) => {
                writing.abandon().await;
                return Err(cut.into());
            }
        }
    }
    let upload = writing.finish().await?;
    let answer = json!({
        "accountId": path.account_id,
        "blobId": upload.id.as_str(),
        "type": upload.media_type,
        "size": upload.size,
    });
    serde_json::to_vec(&answer).map_err(internal)
}

/// A body the client cut answers for the client: silence past the
/// patience 408, past the bound 413, a body that broke off 400.
impl From<Cut> for ApiError {
    fn from(cut: Cut) -> Self {
        match cut {
            Cut::Stalled => Self::RequestTimeout,
            Cut::TooLarge => Self::TooLarge,
            Cut::Broken => Self::InvalidRequest,
        }
    }
}

impl From<ForwardError> for ApiError {
    fn from(error: ForwardError) -> Self {
        match error {
            ForwardError::Cut(cut) => cut.into(),
            ForwardError::TooLarge => Self::TooLarge,
            ForwardError::RateLimited => Self::RateLimited {
                retry_after_ms: i64::try_from(UPLOAD_IDLE_TIMEOUT.as_millis()).unwrap_or(i64::MAX),
            },
            ForwardError::Attempt(inner) => Self::from(inner),
        }
    }
}

/// The store's refusals in fixed words; a fault of the store is the
/// instance's own.
impl From<UploadError> for ApiError {
    fn from(error: UploadError) -> Self {
        match error {
            UploadError::TooLarge => Self::TooLarge,
            UploadError::OverQuota => Self::OverQuota,
            UploadError::Unnamed
            | UploadError::Sealing
            | UploadError::Io(_)
            | UploadError::Task => internal(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;

    use super::*;
    use crate::gate::AttemptError;
    use crate::probe::ProbeError;

    fn status(error: ApiError) -> StatusCode {
        error.into_response().status()
    }

    #[test]
    fn every_upload_failure_has_one_api_word() {
        assert_eq!(status(Cut::Stalled.into()), StatusCode::REQUEST_TIMEOUT);
        assert_eq!(status(Cut::TooLarge.into()), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(status(Cut::Broken.into()), StatusCode::BAD_REQUEST);
        assert_eq!(
            status(ForwardError::TooLarge.into()),
            StatusCode::PAYLOAD_TOO_LARGE
        );
        assert_eq!(
            status(ForwardError::RateLimited.into()),
            StatusCode::TOO_MANY_REQUESTS
        );
        let refused = ForwardError::from(AttemptError::from(ProbeError::CredentialRejected));
        assert_eq!(status(refused.into()), StatusCode::UNAUTHORIZED);
        assert_eq!(
            status(UploadError::TooLarge.into()),
            StatusCode::PAYLOAD_TOO_LARGE
        );
        assert_eq!(
            status(UploadError::OverQuota.into()),
            StatusCode::INSUFFICIENT_STORAGE
        );
        assert_eq!(
            status(UploadError::Sealing.into()),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }
}
