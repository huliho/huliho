// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The upload endpoint of the fixture server (RFC 8620 section 6.1):
//! parked, refused or answered as the script says. Its own answer names
//! what it took: the blob id is a digest of the bytes, the type the one
//! the request declared and `declared` the length it declared.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::json;
use sha2::{Digest, Sha256};

use super::{Shared, authorized, challenge, parked};

/// Where the endpoint listens, as the template names it.
pub const UPLOAD_PATH: &str = "/jmap/upload/{account}/";

/// What the upload endpoint answers.
#[derive(Clone)]
pub struct Upload {
    /// The status the endpoint answers.
    pub status: u16,
    /// The bytes it answers in place of the object it builds.
    pub answer: Option<Vec<u8>>,
    /// Whether the endpoint parks the upload until the next `set`.
    pub hold: bool,
}

impl Upload {
    /// Every upload taken with its object, as Cyrus answers it.
    pub fn taken() -> Self {
        Self {
            status: StatusCode::CREATED.as_u16(),
            answer: None,
            hold: false,
        }
    }
}

/// The blob id the endpoint gives `bytes`: their digest, so a test sees
/// every byte arrived.
pub fn blob_id(bytes: &[u8]) -> String {
    format!(
        "G{}",
        base16ct::lower::encode_string(&Sha256::digest(bytes))
    )
}

/// The endpoint: parked while the script holds, then the scripted status
/// with the fixed answer or the object of what it took.
pub(super) async fn upload(
    State(shared): State<Shared>,
    Path(account): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !authorized(&headers) {
        return challenge();
    }
    let (script, parked) = {
        let script = shared.script.lock().unwrap();
        (script.upload.clone(), parked(&shared, script.upload.hold))
    };
    if let Some(notified) = parked {
        notified.await;
    }
    let header = |name| headers.get(name).and_then(|value| value.to_str().ok());
    let status = StatusCode::from_u16(script.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let answer = script.answer.unwrap_or_else(|| {
        let object = json!({
            "accountId": account,
            "blobId": blob_id(&body),
            "type": header(header::CONTENT_TYPE),
            "size": body.len(),
            "declared": header(header::CONTENT_LENGTH),
        });
        object.to_string().into_bytes()
    });
    (status, [(header::CONTENT_TYPE, "application/json")], answer).into_response()
}
