// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What an upload is held to on its way in: the bounds of the route, the
//! type it declares and its body as it arrives, cut where it stalls or
//! runs past the bound. Nothing of a file is kept here.

use std::time::Duration;

use axum::body::{Body, BodyDataStream, Bytes};
use axum::http::HeaderMap;
use axum::http::header::{CONTENT_LENGTH, CONTENT_TYPE};
use futures_util::StreamExt as _;
use thiserror::Error;
use tokio::time::Instant;

use super::download::{BLOB_DOWNLOAD_LIMIT, DOWNLOAD_IDLE_TIMEOUT, DOWNLOAD_TOTAL_TIMEOUT};

/// One upload at most through the route, the download route's figure; a
/// native upstream judges its own bound under it.
pub const UPLOAD_LIMIT: u64 = BLOB_DOWNLOAD_LIMIT;

/// Uploads in flight per account, beside the request cap and the
/// download lanes; the figure the bridge advertises.
pub const MAX_CONCURRENT_UPLOADS: usize = huliho_imap_bridge::upload::MAX_CONCURRENT_UPLOADS;

/// How long a lane is waited for and how long the body may go silent.
pub const UPLOAD_IDLE_TIMEOUT: Duration = DOWNLOAD_IDLE_TIMEOUT;

/// One upload in total: the largest file on the slowest link the route
/// serves.
pub const UPLOAD_TOTAL_TIMEOUT: Duration = DOWNLOAD_TOTAL_TIMEOUT;

/// A declared type longer than this is no media type a file carries.
const MAX_UPLOAD_TYPE_BYTES: usize = 255;

/// The type of an upload that declares none (RFC 9110 section 8.3).
const OCTET_STREAM: &str = "application/octet-stream";

/// Why the body of an upload stopped short.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum Cut {
    #[error("the body went silent")]
    Stalled,
    #[error("the body runs past the bound")]
    TooLarge,
    #[error("the body broke off")]
    Broken,
}

/// The type an upload declares, as RFC 8620 section 6.1 keeps it: the
/// `Content-Type` as given where it parses as a media type within the
/// bound, `application/octet-stream` where the request names none.
/// `None` for any other value.
#[must_use]
pub fn declared_type(headers: &HeaderMap) -> Option<String> {
    let Some(value) = headers.get(CONTENT_TYPE) else {
        return Some(OCTET_STREAM.to_owned());
    };
    let text = value.to_str().ok()?.trim();
    if text.len() > MAX_UPLOAD_TYPE_BYTES {
        return None;
    }
    // The parser lets an empty subtype through; RFC 9110 section 8.3.1
    // makes it a token.
    let parsed = text.parse::<mime::Mime>().ok()?;
    (!parsed.subtype().as_str().is_empty()).then(|| text.to_owned())
}

/// The length the request declares, when it declares one that reads.
#[must_use]
pub fn declared_length(headers: &HeaderMap) -> Option<u64> {
    headers
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
}

/// The body of an upload as it arrives: chunk by chunk, each within the
/// patience, all of it within the total and the bound.
pub struct Incoming {
    body: BodyDataStream,
    until: Instant,
    bound: u64,
    read: u64,
}

impl Incoming {
    /// The body under `bound` octets; the total starts now.
    #[must_use]
    pub fn new(body: Body, bound: u64) -> Self {
        Self {
            body: body.into_data_stream(),
            until: Instant::now() + UPLOAD_TOTAL_TIMEOUT,
            bound,
            read: 0,
        }
    }

    /// The next chunk; `None` at the end of the body.
    ///
    /// # Errors
    ///
    /// Returns why the body stopped short.
    pub async fn next(&mut self) -> Result<Option<Bytes>, Cut> {
        let left = self.until.saturating_duration_since(Instant::now());
        let patience = left.min(UPLOAD_IDLE_TIMEOUT);
        if patience.is_zero() {
            return Err(Cut::Stalled);
        }
        let chunk = match tokio::time::timeout(patience, self.body.next()).await {
            Err(_elapsed) => return Err(Cut::Stalled),
            Ok(None) => return Ok(None),
            Ok(Some(Err(_broken))) => return Err(Cut::Broken),
            Ok(Some(Ok(chunk))) => chunk,
        };
        let more = u64::try_from(chunk.len()).unwrap_or(u64::MAX);
        self.read = self.read.saturating_add(more);
        if self.read > self.bound {
            return Err(Cut::TooLarge);
        }
        Ok(Some(chunk))
    }
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;
    use futures_util::stream;

    use super::*;

    fn typed(value: Option<&str>) -> Option<String> {
        let mut headers = HeaderMap::new();
        if let Some(value) = value {
            headers.insert(CONTENT_TYPE, HeaderValue::from_str(value).unwrap());
        }
        declared_type(&headers)
    }

    #[test]
    fn a_declared_type_is_kept_as_given_when_it_parses_rfc6838_4_2() {
        assert_eq!(typed(None).as_deref(), Some(OCTET_STREAM));
        for kept in [
            "image/png",
            "application/pdf",
            "text/plain; charset=utf-8",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        ] {
            assert_eq!(typed(Some(kept)).as_deref(), Some(kept), "{kept}");
        }
        let long = format!("application/{}", "x".repeat(MAX_UPLOAD_TYPE_BYTES));
        for refused in [
            "",
            "image",
            "image/",
            "/png",
            "ima ge/png",
            "a/b/c",
            long.as_str(),
        ] {
            assert_eq!(typed(Some(refused)), None, "{refused:?}");
        }
    }

    #[test]
    fn a_declared_length_reads_only_as_a_number() {
        let mut headers = HeaderMap::new();
        assert_eq!(declared_length(&headers), None);
        headers.insert(CONTENT_LENGTH, HeaderValue::from_static("1048576"));
        assert_eq!(declared_length(&headers), Some(1_048_576));
        headers.insert(CONTENT_LENGTH, HeaderValue::from_static("-1"));
        assert_eq!(declared_length(&headers), None);
    }

    fn chunks(sizes: &[usize]) -> Body {
        let chunks: Vec<Result<Bytes, std::io::Error>> = sizes
            .iter()
            .map(|size| Ok(Bytes::from(vec![0; *size])))
            .collect();
        Body::from_stream(stream::iter(chunks))
    }

    #[tokio::test]
    async fn a_body_runs_to_its_end_within_the_bound_and_is_cut_past_it() {
        let mut whole = Incoming::new(chunks(&[3, 4]), 7);
        assert_eq!(whole.next().await.unwrap().unwrap().len(), 3);
        assert_eq!(whole.next().await.unwrap().unwrap().len(), 4);
        assert_eq!(whole.next().await, Ok(None));
        let mut long = Incoming::new(chunks(&[3, 5]), 7);
        assert!(long.next().await.is_ok());
        assert_eq!(long.next().await, Err(Cut::TooLarge));
        let failing = stream::iter([Err::<Bytes, _>(std::io::Error::other("reset"))]);
        let mut broken = Incoming::new(Body::from_stream(failing), 7);
        assert_eq!(broken.next().await, Err(Cut::Broken));
    }

    #[tokio::test(start_paused = true)]
    async fn a_silent_body_is_cut_at_the_patience_and_at_the_total() {
        let silent = Body::from_stream(stream::pending::<Result<Bytes, std::io::Error>>());
        let mut stalled = Incoming::new(silent, 7);
        assert_eq!(stalled.next().await, Err(Cut::Stalled));
        let dripping = stream::repeat_with(|| Ok::<_, std::io::Error>(Bytes::from_static(b"x")))
            .then(|chunk| async move {
                tokio::time::sleep(UPLOAD_IDLE_TIMEOUT / 2).await;
                chunk
            });
        let mut slow = Incoming::new(Body::from_stream(dripping), u64::MAX);
        let mut chunks = 0u32;
        let ended = loop {
            match slow.next().await {
                Ok(Some(_)) => chunks += 1,
                other => break other,
            }
        };
        assert_eq!(ended, Err(Cut::Stalled));
        let most = UPLOAD_TOTAL_TIMEOUT.as_secs() / (UPLOAD_IDLE_TIMEOUT / 2).as_secs();
        assert!(u64::from(chunks) <= most, "{chunks}");
    }
}
