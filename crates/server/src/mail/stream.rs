// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The bytes of a blob on their way to the browser, from a native
//! upstream's answer or from the bridge's windows: the head that
//! decides the type, then the rest under a patience per chunk and a
//! bound on the whole, cut where a range ends and broken off where the
//! bound is passed. The body holds the account's download lane until
//! its last byte.

use std::io;
use std::time::Duration;

use axum::body::{Body, Bytes};
use futures_util::stream;
use huliho_imap_bridge::blob::Blob as BridgeBlob;
use reqwest::Response;
use thiserror::Error;
use tokio::sync::OwnedSemaphorePermit;
use tokio::time::Instant;

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

/// Where the bytes of a blob come from.
pub enum Source {
    /// The answer of a native account's upstream; its request carries
    /// the patience with the whole download.
    Upstream(Response),
    /// The windows the bridge reads of an IMAP account's message, until
    /// the patience with the whole download ends; the route starts that
    /// clock before it asks for the blob.
    Bridge { blob: BridgeBlob, until: Instant },
}

impl Source {
    /// The length the source declares. A bridge blob declares none: an
    /// IMAP server states the size of a message as it counts it, at
    /// times as an estimate, while the size of a part is that of its
    /// transfer encoding.
    fn declared(&self) -> Option<u64> {
        match self {
            Self::Upstream(response) => response.content_length(),
            Self::Bridge { .. } => None,
        }
    }

    /// The next bytes; `None` at the end of the blob.
    async fn next(&mut self) -> Result<Option<Bytes>, StreamError> {
        match self {
            Self::Upstream(response) => response
                .chunk()
                .await
                .map_err(|_failed| StreamError::Failed),
            Self::Bridge { blob, until } => {
                // The clock comes first: a window that waits ready would
                // otherwise go out however long ago the time ran out.
                if Instant::now() >= *until {
                    return Err(StreamError::Stalled);
                }
                match tokio::time::timeout_at(*until, blob.next()).await {
                    Err(_elapsed) => Err(StreamError::Stalled),
                    Ok(None) => Ok(None),
                    Ok(Some(Ok(bytes))) => Ok(Some(Bytes::from(bytes))),
                    Ok(Some(Err(_failed))) => Err(StreamError::Failed),
                }
            }
        }
    }
}

/// A blob with its first bytes in hand and the rest still to stream.
pub struct Blob {
    /// The first bytes, enough for the type.
    pub head: Vec<u8>,
    source: Source,
    /// The length the source declared.
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
pub async fn open(mut source: Source, bounds: &Bounds) -> Result<Blob, StreamError> {
    let declared = source.declared();
    let mut head = Vec::new();
    while head.len() < DETECT_BYTES {
        match chunk(&mut source, bounds).await? {
            Some(bytes) => head.extend_from_slice(&bytes),
            None => break,
        }
    }
    Ok(Blob {
        head,
        source,
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
        source: blob.source,
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
    source: Source,
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
        None => match chunk(&mut flow.source, &flow.bounds).await {
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
async fn chunk(source: &mut Source, bounds: &Bounds) -> Result<Option<Bytes>, StreamError> {
    tokio::time::timeout(bounds.idle, source.next())
        .await
        .map_err(|_elapsed| StreamError::Stalled)?
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;
    use std::sync::Arc;

    use http_body::Frame;
    use http_body_util::combinators::BoxBody;
    use http_body_util::{BodyExt, Full, Limited, StreamBody};
    use huliho_imap_bridge::blob::BlobError as BridgeBlobError;
    use tokio::sync::{Semaphore, mpsc};

    use super::*;

    const PATIENCE: Duration = Duration::from_secs(5);

    /// What a scripted bridge blob gets in all: three chunks that each
    /// arrive within the patience fit, a fourth does not.
    const TOTAL: Duration = Duration::from_secs(15);

    /// The pause between the chunks of a slow bridge blob.
    const SLOW: Duration = Duration::from_secs(4);

    fn bounds(limit: u64) -> Bounds {
        Bounds {
            idle: PATIENCE,
            limit,
        }
    }

    fn answer(body: reqwest::Body) -> Source {
        Source::Upstream(Response::from(axum::http::Response::new(body)))
    }

    /// A body arriving in the given chunks.
    fn chunked(chunks: &[&'static [u8]]) -> Source {
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

    /// A bridge blob of that many chunks, each `pause` after the one
    /// before; it then fails or ends.
    fn bridged(chunks: usize, fails: bool, pause: Duration) -> Source {
        let (sender, receiver) = mpsc::channel(1);
        tokio::spawn(async move {
            for _ in 0..chunks {
                tokio::time::sleep(pause).await;
                if sender.send(Ok(Some(vec![b'w'; 600]))).await.is_err() {
                    return;
                }
            }
            let end = if fails {
                Err(BridgeBlobError::Unavailable)
            } else {
                Ok(None)
            };
            let _unread = sender.send(end).await;
        });
        Source::Bridge {
            blob: BridgeBlob::scripted(receiver),
            until: Instant::now() + TOTAL,
        }
    }

    /// The body of a bridge blob, read to its end or to what broke it off.
    async fn streamed(source: Source) -> Result<Vec<u8>, String> {
        let blob = open(source, &bounds(u64::MAX)).await.unwrap();
        assert_eq!((blob.head.len(), blob.declared), (600, None));
        let (_, permit) = lane();
        collected(body(blob, bounds(u64::MAX), None, permit)).await
    }

    #[tokio::test(start_paused = true)]
    async fn a_bridge_blob_declares_no_length_and_breaks_off_on_a_failure_or_past_its_total() {
        let whole = streamed(bridged(2, false, Duration::ZERO)).await;
        assert_eq!(whole.unwrap().len(), 1200);
        let failed = streamed(bridged(1, true, Duration::ZERO)).await;
        assert!(failed.unwrap_err().contains("could not be read"));
        // Every chunk arrives within the patience, the fourth past the total.
        let slow = streamed(bridged(4, false, SLOW)).await;
        assert!(slow.unwrap_err().contains("stalled"));
        let fits = streamed(bridged(3, false, SLOW)).await;
        assert_eq!(fits.unwrap().len(), 1800);
    }

    #[tokio::test(start_paused = true)]
    async fn a_reader_that_comes_late_to_a_bridge_blob_finds_its_total_run_out() {
        // The second chunk waits ready while the reader stays away past
        // the total; only the head, read before that, still goes out.
        let source = bridged(3, false, Duration::ZERO);
        let blob = open(source, &bounds(u64::MAX)).await.unwrap();
        tokio::time::sleep(TOTAL).await;
        let (_, permit) = lane();
        let mut late = body(blob, bounds(u64::MAX), None, permit);
        let head = late.frame().await.unwrap().unwrap();
        assert_eq!(head.into_data().unwrap().len(), 600);
        let after = late.frame().await.unwrap();
        assert!(after.unwrap_err().to_string().contains("stalled"));
    }

    #[test]
    fn the_bounds_are_the_documented_ones() {
        assert_eq!(BOUNDS.idle, Duration::from_secs(20));
        assert_eq!(BOUNDS.limit, 64 * 1024 * 1024);
    }
}
