// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A JMAP session resource for tests: a challenge without the fixture
//! credential, a session object with it.

use axum::Router;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use huliho_imap_bridge::testing::{PASSWORD, TOKEN};

/// The name the session resource lives under; the test certificate
/// carries it.
pub const HOST: &str = "example.test";
pub const ADDRESS: &str = "sanne@example.test";

/// The session URL on the test server's port.
pub fn session_url(port: u16) -> String {
    format!("https://{HOST}:{port}/jmap/session")
}

/// `/jmap/session` answers the fixture password or token with a session
/// object, with or without the mail capability; `/page` answers a web
/// page.
pub fn routes(mail: bool) -> Router {
    Router::new()
        .route(
            "/jmap/session",
            get(move |headers: HeaderMap| async move { session(&headers, mail) }),
        )
        .route("/page", get(|| async { "<html>welcome</html>" }))
}

fn session(headers: &HeaderMap, mail: bool) -> Response {
    let authorized = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == basic() || value == format!("Bearer {TOKEN}"));
    if !authorized {
        return (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, "Basic realm=\"test\"")],
            "",
        )
            .into_response();
    }
    let capabilities = if mail {
        r#"{"urn:ietf:params:jmap:core":{},"urn:ietf:params:jmap:mail":{}}"#
    } else {
        r#"{"urn:ietf:params:jmap:core":{}}"#
    };
    (
        [(header::CONTENT_TYPE, "application/json")],
        format!(r#"{{"capabilities":{capabilities},"username":"{ADDRESS}"}}"#),
    )
        .into_response()
}

/// The Basic value for the fixture address and password (RFC 7617
/// section 2).
fn basic() -> String {
    format!("Basic {}", BASE64.encode(format!("{ADDRESS}:{PASSWORD}")))
}
