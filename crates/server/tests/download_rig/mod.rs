// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Fixtures for the download route: the path the session object's
//! template expands to, one download through the router and what
//! every blob answer carries.

use axum::Router;
use axum::http::{HeaderMap, Method, StatusCode, header};
use http_body_util::BodyExt;
use huliho_imap_bridge::testing::TOKEN;
use huliho_server::accounts::Credential;
use huliho_server::gate::RUN_WINDOW;
use tower::ServiceExt;

use crate::jmap_upstream::UPSTREAM_ACCOUNT;
use crate::proxy_rig::{Instance, Setup};
use crate::signin::with_cookie;

/// The blob id and the name the tests ask for.
pub const BLOB: &str = "b1";
pub const NAME: &str = "photo.png";

/// The headers every blob answer carries, with their values.
pub const BLOB_HEADERS: [(&str, &str); 5] = [
    ("x-content-type-options", "nosniff"),
    ("content-security-policy", "sandbox; default-src 'none'"),
    ("cross-origin-resource-policy", "same-origin"),
    ("referrer-policy", "no-referrer"),
    ("cache-control", "private, max-age=86400"),
];

pub fn bearer() -> Credential {
    Credential::Bearer {
        token: TOKEN.to_owned(),
    }
}

pub async fn instance() -> Instance {
    Instance::start(Setup {
        window: RUN_WINDOW,
        servers: &[],
    })
    .await
}

/// The path of the route as the session object's template expands it,
/// the name already encoded as a browser sends it.
pub fn path(id: &str, name: &str, media_type: Option<&str>) -> String {
    let query = media_type.map_or(String::new(), |media_type| format!("?type={media_type}"));
    format!("/api/jmap/{id}/download/{UPSTREAM_ACCOUNT}/{BLOB}/{name}{query}")
}

/// One download through the router: the status, the headers and the
/// body; a body that broke off answers the reason instead.
pub async fn download(
    router: &Router,
    cookie: &str,
    uri: &str,
    range: Option<&str>,
) -> (StatusCode, HeaderMap, Result<Vec<u8>, String>) {
    let mut request = with_cookie(Method::GET, uri, cookie);
    if let Some(range) = range {
        request
            .headers_mut()
            .insert(header::RANGE, range.parse().unwrap());
    }
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = response
        .into_body()
        .collect()
        .await
        .map(|collected| collected.to_bytes().to_vec())
        .map_err(|error| error.to_string());
    (status, headers, body)
}

/// The blob requests the fixture upstream saw.
pub fn downloads(instance: &Instance) -> Vec<String> {
    instance
        .upstream
        .lines()
        .into_iter()
        .filter(|line| line.contains("/jmap/download/"))
        .collect()
}

pub fn assert_blob_headers(headers: &HeaderMap) {
    for (name, value) in BLOB_HEADERS {
        assert_eq!(headers.get(name).unwrap(), value, "{name}");
    }
}
