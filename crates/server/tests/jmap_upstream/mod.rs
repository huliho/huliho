// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A JMAP server for the proxy tests: a session object shaped by a
//! script, an API endpoint that echoes the request or misbehaves on
//! demand, every request on record with the credential it carried.

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
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

const SESSION_PATH: &str = "/jmap/session";
const API_PATH: &str = "/jmap/api";
const CORE: &str = "urn:ietf:params:jmap:core";
const MAIL: &str = "urn:ietf:params:jmap:mail";
const WEBSOCKET: &str = "urn:ietf:params:jmap:websocket";
const STATE: &str = "75128aab4b1b";

/// What the server says: the session object's shape and the API
/// endpoint's answer, switchable between requests.
pub struct Script {
    /// The `apiUrl` the session object names.
    pub api_url: String,
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
}

impl Script {
    /// The endpoint echoing the method calls it receives, named
    /// absolutely on `port`.
    pub fn echo(port: u16) -> Self {
        Self {
            api_url: format!("https://{HOST}:{port}{API_PATH}"),
            websocket_url: None,
            status: StatusCode::OK.as_u16(),
            json: true,
            oversized: false,
            hold: false,
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
    let (api_url, websocket_url) = {
        let script = shared.script.lock().unwrap();
        (script.api_url.clone(), script.websocket_url.clone())
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
        "downloadUrl": format!("https://{HOST}/jmap/download/{{accountId}}/{{blobId}}/{{name}}?accept={{type}}"),
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

/// The API endpoint: parked while the script holds, then the scripted
/// status with an echo, a page or an oversized document.
async fn api(State(shared): State<Shared>, headers: HeaderMap, body: Bytes) -> Response {
    if !authorized(&headers) {
        return challenge();
    }
    let (status, json, oversized, parked) = {
        let script = shared.script.lock().unwrap();
        // Enabled under the lock, so a release between reading `hold`
        // and the await cannot be missed.
        let mut notified = Box::pin(shared.release.notified());
        let parked = script.hold.then(|| {
            notified.as_mut().enable();
            notified
        });
        (script.status, script.json, script.oversized, parked)
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
    let answer = if oversized {
        oversized_document()
    } else if json {
        echo(&body)
    } else {
        b"<html>welcome</html>".to_vec()
    };
    (status, [(header::CONTENT_TYPE, content_type)], answer).into_response()
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
