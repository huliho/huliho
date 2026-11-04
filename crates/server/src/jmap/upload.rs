// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The native side of the upload route: the template the upstream
//! session object named, expanded with the account id and checked like
//! every upstream URL; the client's body streamed to it with the
//! account's credential, no byte of it kept here; the answer judged (RFC
//! 8620 section 6.1).

use std::pin::Pin;
use std::sync::{Arc, OnceLock};
use std::task::{Context, Poll};

use axum::body::Bytes;
use http_body::Frame;
use reqwest::header::{CONTENT_LENGTH, CONTENT_TYPE};
use reqwest::{Response, StatusCode};
use serde_json::{Map, Value};
use thiserror::Error;
use tokio::sync::mpsc;

use super::download::fill;
use super::{Proxy, Stored, is_json, unsupported};
use crate::accounts::Account;
use crate::gate::{AttemptError, Fault};
use crate::mail::upload::{Cut, Incoming, UPLOAD_TOTAL_TIMEOUT};
use crate::probe::{self, MAX_SESSION_BYTES, ProbeError};
use crate::scope::Scope;
use crate::upstream::{BodyError, read_bounded};

/// The one variable an upload template carries (RFC 8620 section 2).
const ACCOUNT_VARIABLE: &str = "accountId";

/// An upload answer of this many octets at most: four properties.
const UPLOAD_ANSWER_BYTES: usize = MAX_SESSION_BYTES;

/// The chunks on their way from the client to the upstream at once.
const FORWARD_CHUNKS: usize = 4;

/// The upload the route forwards, in the upstream's terms, with the
/// client's body.
pub struct UploadAsk<'a> {
    /// The upstream account id.
    pub account_id: &'a str,
    pub media_type: &'a str,
    /// The length the client declared, passed on as declared.
    pub length: Option<u64>,
    pub body: Incoming,
}

/// Why an upload did not come back.
#[derive(Debug, Error)]
pub enum ForwardError {
    /// The client's body stopped short; it says nothing of the upstream.
    #[error("the body was cut: {0}")]
    Cut(Cut),
    /// The upstream refused the size.
    #[error("the upstream refused the size")]
    TooLarge,
    /// The upstream holds no more uploads at once.
    #[error("the upstream asks for fewer uploads at once")]
    RateLimited,
    #[error(transparent)]
    Attempt(#[from] AttemptError),
}

impl ForwardError {
    /// The rule the failure falls under; only the attempt says anything
    /// about the account.
    fn fault(&self) -> Fault {
        match self {
            Self::Attempt(error) => error.fault(),
            Self::Cut(_) | Self::TooLarge | Self::RateLimited => Fault::Undecided,
        }
    }
}

impl Proxy {
    /// The client's body forwarded to the upstream's upload endpoint;
    /// the upstream's answer as it came. The gate sees the outcome. The
    /// caller read the row as running.
    ///
    /// # Errors
    ///
    /// Returns [`ForwardError::Cut`] for a body the client cut, the
    /// upstream's refusal of the size or of one more upload and
    /// otherwise as [`Proxy::session`], plus [`ProbeError::Unsupported`]
    /// for a template the proxy cannot expand or reach.
    pub async fn upload(
        &self,
        account: Account,
        scope: &Scope,
        ask: UploadAsk<'_>,
    ) -> Result<Vec<u8>, ForwardError> {
        let stored = self.read(scope, account).await?;
        let outcome = self.forward_upload(&stored, scope, ask).await;
        self.observed(scope, outcome.as_ref().err().map(ForwardError::fault))
            .await?;
        outcome
    }

    async fn forward_upload(
        &self,
        stored: &Stored,
        scope: &Scope,
        ask: UploadAsk<'_>,
    ) -> Result<Vec<u8>, ForwardError> {
        let credential = self
            .live_credential(scope, stored.credential.clone())
            .await?;
        let template = self
            .upstream_urls(stored, &credential)
            .await?
            .upload
            .ok_or_else(|| unsupported("the session object names no upload template"))?;
        let expanded = fill(&template, &[(ACCOUNT_VARIABLE, ask.account_id)])?;
        let url = self
            .checked_endpoint(&stored.session_url, &expanded)
            .await?;
        let (sender, receiver) = mpsc::channel(FORWARD_CHUNKS);
        let cut = Arc::new(OnceLock::new());
        let pump = tokio::spawn(pump(ask.body, sender, Arc::clone(&cut)));
        // A streamed body cannot be sent twice, so a redirect is answered
        // rather than followed.
        let mut request = self
            .wiring
            .upstream
            .http_no_redirect()
            .post(url)
            .header(CONTENT_TYPE, ask.media_type)
            .timeout(UPLOAD_TOTAL_TIMEOUT)
            .body(reqwest::Body::wrap(Forwarded(receiver)));
        if let Some(length) = ask.length {
            request = request.header(CONTENT_LENGTH, length);
        }
        let sent = probe::send(request, &stored.address, &credential).await;
        pump.abort();
        if let Some(cut) = cut.get() {
            return Err(ForwardError::Cut(*cut));
        }
        judged(stored, ask.account_id, sent.map_err(AttemptError::from)?).await
    }
}

/// Hands the client's body on, chunk by chunk; where it stops short the
/// cause is kept before the forwarded body fails with it.
async fn pump(
    mut incoming: Incoming,
    sender: mpsc::Sender<Result<Bytes, Cut>>,
    cut: Arc<OnceLock<Cut>>,
) {
    loop {
        match incoming.next().await {
            Ok(Some(chunk)) => {
                if sender.send(Ok(chunk)).await.is_err() {
                    return;
                }
            }
            Ok(None) => return,
            Err(cause) => {
                let _kept = cut.set(cause);
                let _unread = sender.send(Err(cause)).await;
                return;
            }
        }
    }
}

/// The client's body as the upstream reads it.
struct Forwarded(mpsc::Receiver<Result<Bytes, Cut>>);

impl http_body::Body for Forwarded {
    type Data = Bytes;
    type Error = Cut;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Cut>>> {
        self.0
            .poll_recv(context)
            .map(|step| step.map(|chunk| chunk.map(Frame::data)))
    }
}

/// The upstream's answer: on a success one JSON object with the four
/// properties of RFC 8620 section 6.1 for the account asked, passed back
/// as it came; the credential's verdict on a 401; the size or the count
/// refused on a 413 or a 429; a server error of the upstream's own on a
/// 5xx; anything else, a redirect included, is no upload answer.
async fn judged(
    stored: &Stored,
    account_id: &str,
    response: Response,
) -> Result<Vec<u8>, ForwardError> {
    let status = response.status();
    if !status.is_success() {
        tracing::debug!(
            account = stored.account_id.as_str(),
            status = status.as_u16(),
            "the upstream answered an upload with an error"
        );
    }
    match status {
        status if status.is_success() => {}
        StatusCode::UNAUTHORIZED => {
            return Err(AttemptError::from(ProbeError::CredentialRejected).into());
        }
        StatusCode::PAYLOAD_TOO_LARGE => return Err(ForwardError::TooLarge),
        StatusCode::TOO_MANY_REQUESTS => return Err(ForwardError::RateLimited),
        status if status.is_server_error() => return Err(AttemptError::Failed(status).into()),
        status => return Err(unsupported(format!("the upload endpoint answered {status}")).into()),
    }
    if !is_json(response.headers()) {
        return Err(unsupported("the upload answer is not JSON").into());
    }
    let body = read_bounded(response, UPLOAD_ANSWER_BYTES)
        .await
        .map_err(|error| match error {
            BodyError::TooLarge => unsupported("the upload answer is larger than it can be"),
            BodyError::ReadFailed => {
                ProbeError::Unreachable("the upload answer ended early".to_owned()).into()
            }
        })?;
    if !is_upload(&body, account_id) {
        return Err(unsupported("the answer is no upload of the account").into());
    }
    Ok(body)
}

/// Whether the body is the object RFC 8620 section 6.1 names: the
/// account asked for, a blob id, a type and a size.
fn is_upload(body: &[u8], account_id: &str) -> bool {
    let Ok(object) = serde_json::from_slice::<Map<String, Value>>(body) else {
        return false;
    };
    let text = |name: &str| object.get(name).and_then(Value::as_str);
    text("accountId") == Some(account_id)
        && text("blobId").is_some_and(|id| !id.is_empty())
        && text("type").is_some()
        && object.get("size").and_then(Value::as_u64).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_upload_answer_names_the_account_a_blob_a_type_and_a_size_rfc8620_6_1() {
        let answer = br#"{"accountId":"u1","blobId":"G6","type":"image/png","size":12}"#;
        assert!(is_upload(answer, "u1"));
        assert!(!is_upload(answer, "u2"));
        for broken in [
            br#"{"accountId":"u1","blobId":"","type":"image/png","size":12}"#.as_slice(),
            br#"{"accountId":"u1","type":"image/png","size":12}"#,
            br#"{"accountId":"u1","blobId":"G6","size":12}"#,
            br#"{"accountId":"u1","blobId":"G6","type":"image/png","size":-1}"#,
            br#"[{"accountId":"u1"}]"#,
            b"not json",
        ] {
            assert!(
                !is_upload(broken, "u1"),
                "{}",
                String::from_utf8_lossy(broken)
            );
        }
    }

    #[test]
    fn only_the_attempt_decides_anything_at_the_gate() {
        for undecided in [
            ForwardError::Cut(Cut::Stalled),
            ForwardError::TooLarge,
            ForwardError::RateLimited,
        ] {
            assert_eq!(undecided.fault(), Fault::Undecided);
        }
        let refused = ForwardError::from(AttemptError::from(ProbeError::CredentialRejected));
        assert_eq!(refused.fault(), Fault::Credential);
    }
}
