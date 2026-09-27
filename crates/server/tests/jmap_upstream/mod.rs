// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A JMAP server for the proxy tests: a session object shaped by a
//! script, an API endpoint that echoes the request or misbehaves on
//! demand, a download endpoint that serves a scripted blob, every
//! request on record with the credential it carried.

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use futures_util::{StreamExt as _, stream};
use huliho_imap_bridge::testing::{PASSWORD, TOKEN};
use huliho_server::config::UpstreamConfig;
use huliho_server::jmap::JMAP_RESPONSE_LIMIT;
use serde_json::{Value, json};
use tokio::sync::Notify;
use url::Url;

use crate::tls_server::TlsServer;

/// The name the server lives under; the test certificate carries it.
pub const HOST: &str = "example.test";
/// The address of the account the session object serves.
pub const ADDRESS: &str = "sanne@example.test";
/// The upstream account id the session object names.
pub const UPSTREAM_ACCOUNT: &str = "u1";
/// A name under the certificate that the resolver answers with a
/// private address.
pub const INWARD_HOST: &str = "inside.example.test";

/// The bytes of a small PNG: the signature and the start of its header.
pub const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0";

const SESSION_PATH: &str = "/jmap/session";
const API_PATH: &str = "/jmap/api";
const DOWNLOAD_PATH: &str = "/jmap/download/{account}/{blob}/{name}";
const CORE: &str = "urn:ietf:params:jmap:core";
const MAIL: &str = "urn:ietf:params:jmap:mail";
const WEBSOCKET: &str = "urn:ietf:params:jmap:websocket";
const STATE: &str = "75128aab4b1b";

/// The chunk an endless blob streams over and over.
const ENDLESS_CHUNK: usize = 64 * 1024;

/// What the server says: the session object's shape, the API
/// endpoint's answer and the blob the download endpoint serves,
/// switchable between requests.
pub struct Script {
    /// The `apiUrl` the session object names.
    pub api_url: String,
    /// The `downloadUrl` template the session object names.
    pub download_url: String,
    /// The URL of a websocket capability the session object advertises,
    /// none for a server without one.
    pub websocket_url: Option<String>,
    /// The status the API endpoint answers.
    pub status: u16,
    /// Whether that answer carries a JSON content type.
    pub json: bool,
    /// Whether the answer runs one byte past the proxy's limit.
    pub oversized: bool,
    /// Whether the endpoint parks the request until the next `set`.
    pub hold: bool,
    /// The bytes the endpoint answers in place of the echo.
    pub answer: Option<Vec<u8>>,
    /// What the download endpoint serves.
    pub blob: Blob,
}

/// A blob as the download endpoint serves it.
#[derive(Clone)]
pub struct Blob {
    pub bytes: Vec<u8>,
    /// The status the endpoint answers.
    pub status: u16,
    /// The length the endpoint declares; `None` streams without one.
    pub declared: Option<u64>,
    /// Whether the endpoint parks the request until the next `set`.
    pub hold: bool,
    /// Whether the endpoint streams zeros without end after `bytes`.
    pub endless: bool,
    /// Whether the endpoint stalls after `bytes` without ending.
    pub stalls: bool,
}

impl Blob {
    /// The given bytes, declared in full.
    pub fn of(bytes: &[u8]) -> Self {
        Self {
            bytes: bytes.to_vec(),
            status: StatusCode::OK.as_u16(),
            declared: Some(bytes.len() as u64),
            hold: false,
            endless: false,
            stalls: false,
        }
    }
}

impl Script {
    /// The endpoint echoing the method calls it receives, named
    /// absolutely on `port`, with a PNG behind the download endpoint.
    pub fn echo(port: u16) -> Self {
        Self {
            api_url: format!("https://{HOST}:{port}{API_PATH}"),
            download_url: format!(
                "https://{HOST}:{port}/jmap/download/{{accountId}}/{{blobId}}/{{name}}?accept={{type}}"
            ),
            websocket_url: None,
            status: StatusCode::OK.as_u16(),
            json: true,
            oversized: false,
            hold: false,
            answer: None,
            blob: Blob::of(PNG),
        }
    }
}

#[derive(Clone)]
struct Shared {
    script: Arc<Mutex<Script>>,
    release: Arc<Notify>,
}

/// The server: its TLS listener and the script it plays.
pub struct JmapUpstream {
    server: TlsServer,
    shared: Shared,
}

impl JmapUpstream {
    /// Serves the echo script on a loopback port.
    pub async fn start() -> Self {
        let shared = Shared {
            script: Arc::new(Mutex::new(Script::echo(0))),
            release: Arc::new(Notify::new()),
        };
        let server = TlsServer::start(routes(shared.clone())).await;
        *shared.script.lock().unwrap() = Script::echo(server.address.port());
        Self { server, shared }
    }

    /// The session URL of the account on this server.
    pub fn session_url(&self) -> Url {
        Url::parse(&format!("https://{HOST}:{}{SESSION_PATH}", self.port())).unwrap()
    }

    pub fn port(&self) -> u16 {
        self.server.address.port()
    }

    /// The upstream rules that trust this server's CA and reach the
    /// loopback.
    pub fn config(&self) -> UpstreamConfig {
        self.server.config(true)
    }

    /// Replaces the script and releases every parked request.
    pub fn set(&self, script: Script) {
        *self.shared.script.lock().unwrap() = script;
        self.shared.release.notify_waiters();
    }

    /// Every request so far as `METHOD host path`, plus the credential
    /// it carried.
    pub fn lines(&self) -> Vec<String> {
        self.server.requests()
    }

    /// The Basic value for the fixture address and password (RFC 7617
    /// section 2).
    pub fn basic() -> String {
        format!("Basic {}", BASE64.encode(format!("{ADDRESS}:{PASSWORD}")))
    }
}

fn routes(shared: Shared) -> Router {
    Router::new()
        .route(SESSION_PATH, get(session))
        .route(API_PATH, post(api))
        .route(DOWNLOAD_PATH, get(download))
        .with_state(shared)
}

fn authorized(headers: &HeaderMap) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == JmapUpstream::basic() || value == format!("Bearer {TOKEN}"))
}

fn challenge() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, "Basic realm=\"test\"")],
        "",
    )
        .into_response()
}

/// The session object (RFC 8620 section 2) as a full-featured server
/// answers it, the websocket capability included when the script says
/// so.
async fn session(State(shared): State<Shared>, headers: HeaderMap) -> Response {
    if !authorized(&headers) {
        return challenge();
    }
    let (api_url, download_url, websocket_url) = {
        let script = shared.script.lock().unwrap();
        (
            script.api_url.clone(),
            script.download_url.clone(),
            script.websocket_url.clone(),
        )
    };
    let mut capabilities = json!({
        CORE: {
            "maxSizeUpload": 50_000_000,
            "maxConcurrentUpload": 4,
            "maxSizeRequest": 10_000_000,
            "maxConcurrentRequests": 8,
            "maxCallsInRequest": 16,
            "maxObjectsInGet": 500,
            "maxObjectsInSet": 500,
            "collationAlgorithms": ["i;unicode-casemap"]
        },
        MAIL: {}
    });
    let mut account_capabilities = json!({ CORE: {}, MAIL: { "maxMailboxDepth": null } });
    let mut primary = json!({ MAIL: UPSTREAM_ACCOUNT });
    if let Some(url) = websocket_url {
        capabilities[WEBSOCKET] = json!({ "url": url, "supportsPush": true });
        account_capabilities[WEBSOCKET] = json!({});
        primary[WEBSOCKET] = json!(UPSTREAM_ACCOUNT);
    }
    let body = json!({
        "capabilities": capabilities,
        "accounts": {
            UPSTREAM_ACCOUNT: {
                "name": ADDRESS,
                "isPersonal": true,
                "isReadOnly": false,
                "accountCapabilities": account_capabilities
            }
        },
        "primaryAccounts": primary,
        "username": ADDRESS,
        "apiUrl": api_url,
        "downloadUrl": download_url,
        "uploadUrl": format!("https://{HOST}/jmap/upload/{{accountId}}/"),
        "eventSourceUrl": format!("https://{HOST}/jmap/eventsource/?types={{types}}&closeafter={{closeafter}}&ping={{ping}}"),
        "state": STATE
    });
    (
        [(header::CONTENT_TYPE, "application/json")],
        body.to_string(),
    )
        .into_response()
}

/// A request parked until the next `set` when the script holds;
/// enabled under the lock, so a release between reading the flag and
/// the await cannot be missed.
fn parked(
    shared: &Shared,
    hold: bool,
) -> Option<std::pin::Pin<Box<tokio::sync::futures::Notified<'_>>>> {
    hold.then(|| {
        let mut notified = Box::pin(shared.release.notified());
        notified.as_mut().enable();
        notified
    })
}

/// The API endpoint: parked while the script holds, then the scripted
/// status with the fixed answer, an echo, a page or an oversized
/// document.
async fn api(State(shared): State<Shared>, headers: HeaderMap, body: Bytes) -> Response {
    if !authorized(&headers) {
        return challenge();
    }
    let (status, json, oversized, parked, fixed) = {
        let script = shared.script.lock().unwrap();
        (
            script.status,
            script.json,
            script.oversized,
            parked(&shared, script.hold),
            script.answer.clone(),
        )
    };
    if let Some(notified) = parked {
        notified.await;
    }
    let status = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let content_type = if json {
        "application/json"
    } else {
        "text/html"
    };
    let answer = if let Some(fixed) = fixed {
        fixed
    } else if oversized {
        oversized_document()
    } else if json {
        echo(&body)
    } else {
        b"<html>welcome</html>".to_vec()
    };
    (status, [(header::CONTENT_TYPE, content_type)], answer).into_response()
}

/// The download endpoint: parked while the blob holds, then the
/// scripted status with the blob's bytes, declared or not, followed by
/// zeros without end or by silence when the script says so.
async fn download(State(shared): State<Shared>, headers: HeaderMap) -> Response {
    if !authorized(&headers) {
        return challenge();
    }
    let (blob, parked) = {
        let script = shared.script.lock().unwrap();
        (script.blob.clone(), parked(&shared, script.blob.hold))
    };
    if let Some(notified) = parked {
        notified.await;
    }
    let status = StatusCode::from_u16(blob.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mut response = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/octet-stream");
    if let Some(declared) = blob.declared {
        response = response.header(header::CONTENT_LENGTH, declared);
    }
    let first = stream::iter([Ok::<Bytes, std::io::Error>(Bytes::from(blob.bytes))]);
    let body = if blob.endless {
        let zeros = Bytes::from(vec![0; ENDLESS_CHUNK]);
        Body::from_stream(first.chain(stream::repeat_with(move || Ok(zeros.clone()))))
    } else if blob.stalls {
        Body::from_stream(first.chain(stream::pending()))
    } else {
        Body::from_stream(first)
    };
    response.body(body).unwrap()
}

/// A Response object naming each method call back (RFC 8620 section
/// 3.4), the call's arguments under `echoed`.
fn echo(request: &[u8]) -> Vec<u8> {
    let parsed: Value = serde_json::from_slice(request).unwrap_or(Value::Null);
    let responses: Vec<Value> = parsed["methodCalls"]
        .as_array()
        .map(|calls| {
            calls
                .iter()
                .map(|call| json!([call[0], { "echoed": call[1] }, call[2]]))
                .collect()
        })
        .unwrap_or_default();
    json!({ "methodResponses": responses, "sessionState": STATE })
        .to_string()
        .into_bytes()
}

/// A JSON document one byte past the proxy's answer limit.
fn oversized_document() -> Vec<u8> {
    let mut document = Vec::with_capacity(JMAP_RESPONSE_LIMIT + 1);
    document.extend_from_slice(b"{\"pad\":\"");
    document.resize(JMAP_RESPONSE_LIMIT - 1, b'x');
    document.extend_from_slice(b"\"}");
    document
}
