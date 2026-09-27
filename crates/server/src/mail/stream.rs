// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The bytes of a blob on their way to the browser: the head that
//! decides the type, then the rest under a patience per chunk and a
//! bound on the whole, cut where a range ends and broken off where the
//! bound is passed. The body holds the account's download lane until
//! its last byte.

use std::io;
use std::time::Duration;

use axum::body::{Body, Bytes};
use futures_util::stream;
use reqwest::Response;
use thiserror::Error;
use tokio::sync::OwnedSemaphorePermit;

use super::detect::DETECT_BYTES;
use super::download::{BLOB_DOWNLOAD_LIMIT, DOWNLOAD_IDLE_TIMEOUT};

/// What the stream holds itself to.
#[derive(Debug, Clone, Copy)]
pub struct Bounds {
    /// How long one chunk may take.
    pub idle: Duration,
    /// How many bytes the whole blob may hold.
    pub limit: u64,
}

/// The bounds every blob streams under.
pub const BOUNDS: Bounds = Bounds {
    idle: DOWNLOAD_IDLE_TIMEOUT,
    limit: BLOB_DOWNLOAD_LIMIT,
};

/// A blob with its first bytes in hand and the rest still to stream.
pub struct Blob {
    /// The first bytes, enough for the type.
    pub head: Vec<u8>,
    pub response: Response,
    /// The length the upstream declared.
    pub declared: Option<u64>,
}

/// Why the bytes stopped short.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum StreamError {
    #[error("the blob stalled")]
    Stalled,
    #[error("the blob could not be read")]
    Failed,
    #[error("the blob runs past the limit")]
    TooLarge,
}

impl From<StreamError> for io::Error {
    fn from(error: StreamError) -> Self {
        Self::other(error)
    }
}

/// The blob with its head read: until [`DETECT_BYTES`] are in hand or
/// the body ends.
///
/// # Errors
///
/// Returns the reason a chunk did not arrive.
pub async fn open(mut response: Response, bounds: &Bounds) -> Result<Blob, StreamError> {
    let declared = response.content_length();
    let mut head = Vec::new();
    while head.len() < DETECT_BYTES {
        match chunk(&mut response, bounds).await? {
            Some(bytes) => head.extend_from_slice(&bytes),
            None => break,
        }
    }
    Ok(Blob {
        head,
        response,
        declared,
    })
}

/// The whole body: the head read before, then the rest. `cut` ends the
/// stream after that many bytes; the bound passed breaks it off with
/// an error, so the browser sees a failed download and never a short
/// file that looks whole. The lane is given back when the body ends.
pub fn body(blob: Blob, bounds: Bounds, cut: Option<u64>, lane: OwnedSemaphorePermit) -> Body {
    let flow = Flow {
        pending: Some(Bytes::from(blob.head)),
        response: blob.response,
        bounds,
        cut,
        sent: 0,
        ended: false,
        _lane: lane,
    };
    Body::from_stream(stream::unfold(flow, step))
}

struct Flow {
    pending: Option<Bytes>,
    response: Response,
    bounds: Bounds,
    cut: Option<u64>,
    sent: u64,
    ended: bool,
    _lane: OwnedSemaphorePermit,
}

async fn step(mut flow: Flow) -> Option<(Result<Bytes, io::Error>, Flow)> {
    if flow.ended || flow.cut.is_some_and(|cut| flow.sent >= cut) {
        return None;
    }
    let next = match flow.pending.take() {
        Some(head) => Some(head),
        None => match chunk(&mut flow.response, &flow.bounds).await {
            Ok(next) => next,
            Err(error) => return Some(broken(flow, error)),
        },
    };
    let mut bytes = next?;
    if let Some(cut) = flow.cut {
        let room = usize::try_from(cut - flow.sent).unwrap_or(usize::MAX);
        bytes.truncate(room);
    } else if flow.sent.saturating_add(len(&bytes)) > flow.bounds.limit {
        return Some(broken(flow, StreamError::TooLarge));
    }
    flow.sent += len(&bytes);
    Some((Ok(bytes), flow))
}

fn broken(mut flow: Flow, error: StreamError) -> (Result<Bytes, io::Error>, Flow) {
    flow.ended = true;
    (Err(error.into()), flow)
}

fn len(bytes: &Bytes) -> u64 {
    u64::try_from(bytes.len()).unwrap_or(u64::MAX)
}

/// One chunk within the patience; `None` at the end of the body.
async fn chunk(response: &mut Response, bounds: &Bounds) -> Result<Option<Bytes>, StreamError> {
    tokio::time::timeout(bounds.idle, response.chunk())
        .await
        .map_err(|_elapsed| StreamError::Stalled)?
        .map_err(|_failed| StreamError::Failed)
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;
    use std::sync::Arc;

    use http_body::Frame;
    use http_body_util::combinators::BoxBody;
    use http_body_util::{BodyExt, Full, Limited, StreamBody};
    use tokio::sync::Semaphore;

    use super::*;

    const PATIENCE: Duration = Duration::from_secs(5);

    fn bounds(limit: u64) -> Bounds {
        Bounds {
            idle: PATIENCE,
            limit,
        }
    }

    fn answer(body: reqwest::Body) -> Response {
        Response::from(axum::http::Response::new(body))
    }

    /// A body arriving in the given chunks.
    fn chunked(chunks: &[&'static [u8]]) -> Response {
        let frames: Vec<Result<Frame<Bytes>, Infallible>> = chunks
            .iter()
            .map(|chunk| Ok(Frame::data(Bytes::from_static(chunk))))
            .collect();
        let body: BoxBody<Bytes, Infallible> = BoxBody::new(StreamBody::new(stream::iter(frames)));
        answer(reqwest::Body::wrap(body))
    }

    /// A lane of one, so a test can see it given back.
    fn lane() -> (Arc<Semaphore>, OwnedSemaphorePermit) {
        let lane = Arc::new(Semaphore::new(1));
        let permit = Arc::clone(&lane).try_acquire_owned().unwrap();
        (lane, permit)
    }

    async fn collected(body: Body) -> Result<Vec<u8>, String> {
        body.collect()
            .await
            .map(|collected| collected.to_bytes().to_vec())
            .map_err(|error| error.to_string())
    }

    #[tokio::test]
    async fn the_head_gathers_the_detect_bytes_or_the_whole_body_and_the_rest_follows() {
        let long = vec![b'x'; DETECT_BYTES + 10];
        let blob = open(answer(long.clone().into()), &bounds(u64::MAX))
            .await
            .unwrap();
        assert_eq!(blob.head.len(), DETECT_BYTES + 10);
        assert_eq!(blob.declared, Some(len(&Bytes::from(long))));
        let blob = open(chunked(&[b"ab", b"cd", b"ef"]), &bounds(u64::MAX))
            .await
            .unwrap();
        assert_eq!(blob.head, b"abcdef");
        assert_eq!(blob.declared, None);
        let blob = open(
            chunked(&[&[b'a'; 300], &[b'b'; 300], &[b'c'; 300]]),
            &bounds(u64::MAX),
        )
        .await
        .unwrap();
        assert_eq!(blob.head.len(), 600);
        let (_, permit) = lane();
        let whole = collected(body(blob, bounds(u64::MAX), None, permit))
            .await
            .unwrap();
        assert_eq!(whole.len(), 900);
        assert!(whole[600..].iter().all(|byte| *byte == b'c'));
    }

    #[tokio::test]
    async fn a_range_cuts_the_body_where_it_ends() {
        let blob = open(chunked(&[b"abc", b"def", b"ghi"]), &bounds(u64::MAX))
            .await
            .unwrap();
        let (_, permit) = lane();
        let cut = collected(body(blob, bounds(u64::MAX), Some(4), permit))
            .await
            .unwrap();
        assert_eq!(cut, b"abcd");
        let blob = open(chunked(&[&[b'a'; 600], &[b'b'; 600]]), &bounds(u64::MAX))
            .await
            .unwrap();
        let (_, permit) = lane();
        let cut = collected(body(blob, bounds(u64::MAX), Some(700), permit))
            .await
            .unwrap();
        assert_eq!(cut.len(), 700);
        assert_eq!(cut[699], b'b');
    }

    #[tokio::test]
    async fn a_body_past_the_bound_breaks_off_with_an_error() {
        let blob = open(chunked(&[&[b'a'; 600], &[b'b'; 600]]), &bounds(1000))
            .await
            .unwrap();
        let (_, permit) = lane();
        let outcome = collected(body(blob, bounds(1000), None, permit)).await;
        assert!(outcome.unwrap_err().contains("past the limit"));
        let blob = open(chunked(&[&[b'a'; 600], &[b'b'; 400]]), &bounds(1000))
            .await
            .unwrap();
        let (_, permit) = lane();
        let whole = collected(body(blob, bounds(1000), None, permit))
            .await
            .unwrap();
        assert_eq!(whole.len(), 1000);
    }

    #[tokio::test]
    async fn the_lane_is_held_until_the_body_ends_or_is_dropped() {
        let blob = open(chunked(&[&[b'a'; 600], &[b'b'; 600]]), &bounds(u64::MAX))
            .await
            .unwrap();
        let (lane, permit) = lane();
        let streamed = body(blob, bounds(u64::MAX), None, permit);
        assert_eq!(lane.available_permits(), 0);
        collected(streamed).await.unwrap();
        assert_eq!(lane.available_permits(), 1);
        let blob = open(chunked(&[&[b'a'; 600]]), &bounds(u64::MAX))
            .await
            .unwrap();
        let permit = Arc::clone(&lane).try_acquire_owned().unwrap();
        let dropped = body(blob, bounds(u64::MAX), None, permit);
        assert_eq!(lane.available_permits(), 0);
        drop(dropped);
        assert_eq!(lane.available_permits(), 1);
    }

    #[tokio::test]
    async fn a_body_that_fails_to_read_breaks_off() {
        let failing = Limited::new(Full::new(Bytes::from_static(&[b'a'; 700])), 100);
        let outcome = open(answer(reqwest::Body::wrap(failing)), &bounds(u64::MAX)).await;
        assert!(matches!(outcome, Err(StreamError::Failed)));
    }

    #[tokio::test(start_paused = true)]
    async fn a_body_that_stalls_breaks_off_after_the_patience() {
        let stalled: BoxBody<Bytes, Infallible> = BoxBody::new(StreamBody::new(stream::pending::<
            Result<Frame<Bytes>, Infallible>,
        >()));
        let outcome = open(answer(reqwest::Body::wrap(stalled)), &bounds(u64::MAX)).await;
        assert!(matches!(outcome, Err(StreamError::Stalled)));
    }

    #[test]
    fn the_bounds_are_the_documented_ones() {
        assert_eq!(BOUNDS.idle, Duration::from_secs(20));
        assert_eq!(BOUNDS.limit, 64 * 1024 * 1024);
    }
}
