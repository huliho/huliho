// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What the download tests of both account kinds share: one download
//! through the router and the headers every blob answer carries.

use axum::Router;
use axum::http::{HeaderMap, Method, StatusCode, header};
use http_body_util::BodyExt;
use tower::ServiceExt;

use crate::signin::with_cookie;

/// The headers every blob answer carries, with their values.
pub const BLOB_HEADERS: [(&str, &str); 5] = [
    ("x-content-type-options", "nosniff"),
    ("content-security-policy", "sandbox; default-src 'none'"),
    ("cross-origin-resource-policy", "same-origin"),
    ("referrer-policy", "no-referrer"),
    ("cache-control", "private, max-age=86400"),
];

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

pub fn assert_blob_headers(headers: &HeaderMap) {
    for (name, value) in BLOB_HEADERS {
        assert_eq!(headers.get(name).unwrap(), value, "{name}");
    }
}
