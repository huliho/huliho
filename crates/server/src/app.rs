// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The HTTP router: SPA serving, liveness and the response contract.

use std::path::Path;

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Request, header};
use axum::routing::get;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::{DefaultOnResponse, TraceLayer};
use tracing::Level;

/// Liveness body only; version and build info stay out on purpose.
const HEALTH_BODY: &str = "ok";

const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");

/// Same-origin only; the mail rendering pipeline adds its own sandbox
/// on top of this policy.
const CONTENT_SECURITY_POLICY: &str =
    "default-src 'self'; frame-ancestors 'none'; base-uri 'self'; form-action 'self'";

/// Two years with subdomains; the preload token is the operator's call.
const STRICT_TRANSPORT_SECURITY: &str = "max-age=63072000; includeSubDomains";

/// A mail client needs none of these browser features.
const PERMISSIONS_POLICY: &str = "camera=(), geolocation=(), microphone=()";

pub fn router(assets: &Path) -> Router {
    let spa = ServeDir::new(assets).fallback(ServeFile::new(assets.join("index.html")));
    Router::new()
        .route("/healthz", get(healthz))
        .fallback_service(spa)
        .layer(response_header(
            header::CONTENT_SECURITY_POLICY,
            CONTENT_SECURITY_POLICY,
        ))
        .layer(response_header(header::X_CONTENT_TYPE_OPTIONS, "nosniff"))
        .layer(response_header(header::REFERRER_POLICY, "no-referrer"))
        .layer(response_header(
            HeaderName::from_static("permissions-policy"),
            PERMISSIONS_POLICY,
        ))
        .layer(response_header(
            header::STRICT_TRANSPORT_SECURITY,
            STRICT_TRANSPORT_SECURITY,
        ))
        .layer(PropagateRequestIdLayer::new(REQUEST_ID_HEADER))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(request_span)
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .layer(SetRequestIdLayer::new(REQUEST_ID_HEADER, MakeRequestUuid))
}

async fn healthz() -> &'static str {
    HEALTH_BODY
}

fn request_span(request: &Request<Body>) -> tracing::Span {
    let request_id = request
        .headers()
        .get(REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    tracing::info_span!(
        "request",
        method = %request.method(),
        uri = %request.uri(),
        request_id
    )
}

fn response_header(name: HeaderName, value: &'static str) -> SetResponseHeaderLayer<HeaderValue> {
    SetResponseHeaderLayer::overriding(name, HeaderValue::from_static(value))
}
