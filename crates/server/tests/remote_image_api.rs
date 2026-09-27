// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The remote-image proxy: what it refuses before anything resolves or
//! connects, which redirects it follows, what it reads and what the
//! image host sees of the reader.

mod common;
mod fake_dns;
mod image_host;
mod signin;
mod tls_server;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, Response, StatusCode, header};
use fake_dns::FakeDns;
use http_body_util::BodyExt;
use huliho_server::api::ApiState;
use huliho_server::mail::download::ENCODED;
use huliho_server::mail::remote::{
    REMOTE_IMAGE_BURST, REMOTE_REDIRECTS, REMOTE_URL_BYTES, REMOTE_USER_AGENT,
};
use huliho_server::upstream::{Dns, Upstream};
use image_host::{HOST, INWARD_HOST, ImageHost, OTHER_HOST, PNG};
use percent_encoding::utf8_percent_encode;
use signin::{body_text, sign_in, store_with_account, with_cookie};
use tokio::sync::Semaphore;
use tokio::time::sleep;
use tower::ServiceExt;

/// How often and how long the slot test looks for the parked fetch.
const POLL: Duration = Duration::from_millis(10);
const PATIENCE: u32 = 300;

/// The headers every image answer carries, with their values.
const BLOB_HEADERS: [(&str, &str); 5] = [
    ("x-content-type-options", "nosniff"),
    ("content-security-policy", "sandbox; default-src 'none'"),
    ("cross-origin-resource-policy", "same-origin"),
    ("referrer-policy", "no-referrer"),
    ("cache-control", "private, max-age=86400"),
];

/// A private address for the inward host; the network rule refuses it
/// before anything connects.
const INWARD_ADDRESS: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 0);

/// Tokens that may come back while a test drains the allowance: two a
/// second, so a slow run still meets 429 within this many past the
/// burst.
const REFILL_MARGIN: u32 = 10;

struct Rig {
    router: Router,
    host: ImageHost,
    dns: Arc<FakeDns>,
    api: ApiState,
}

/// The router over the image host: its two names at the host, the
/// inward name at a private address, the host's CA trusted and the
/// loopback allowed.
async fn rig() -> Rig {
    let host = ImageHost::start().await;
    let mut dns = FakeDns::default();
    for name in [HOST, OTHER_HOST] {
        dns.addresses
            .insert(name.to_owned(), vec![host.server.address]);
    }
    dns.addresses
        .insert(INWARD_HOST.to_owned(), vec![INWARD_ADDRESS]);
    let dns = Arc::new(dns);
    let shared: Arc<dyn Dns> = dns.clone();
    let api = ApiState {
        upstream: Arc::new(Upstream::with_dns(&host.server.config(true), shared).unwrap()),
        ..common::api_state(store_with_account())
    };
    Rig {
        router: common::router_with(api.clone()),
        host,
        dns,
        api,
    }
}

impl Rig {
    fn lookups(&self) -> usize {
        self.dns.queries.lock().unwrap().len()
    }

    /// The paths the image host was asked for, in order.
    fn requests(&self) -> Vec<String> {
        self.host
            .server
            .requests()
            .iter()
            .map(|line| line.split(' ').nth(2).unwrap().to_owned())
            .collect()
    }

    /// A second session of the fixture user, opened on a plain router
    /// over the same store.
    async fn sign_in_again(&self) -> String {
        sign_in(&common::router_on(Arc::clone(&self.api.store))).await
    }
}

fn route(url: &str) -> String {
    format!(
        "/api/remote-image?url={}",
        utf8_percent_encode(url, ENCODED)
    )
}

async fn send(router: &Router, request: Request<Body>) -> Response<Body> {
    router.clone().oneshot(request).await.unwrap()
}

/// One fetch through the router: the status, the headers and the bytes.
async fn fetch(router: &Router, cookie: &str, url: &str) -> (StatusCode, HeaderMap, Vec<u8>) {
    let response = send(router, with_cookie(Method::GET, &route(url), cookie)).await;
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, headers, body.to_vec())
}

/// A fetch the route turns down: the status, the headers and what the
/// answer says.
async fn refused(router: &Router, cookie: &str, url: &str) -> (StatusCode, HeaderMap, String) {
    let response = send(router, with_cookie(Method::GET, &route(url), cookie)).await;
    let status = response.status();
    let headers = response.headers().clone();
    (status, headers, body_text(response).await)
}

fn assert_blob_headers(headers: &HeaderMap) {
    for (name, value) in BLOB_HEADERS {
        assert_eq!(headers.get(name).unwrap(), value, "{name}");
    }
}

#[tokio::test]
async fn a_request_without_a_session_is_refused_before_anything_resolves() {
    let rig = rig().await;
    let request = Request::get(route(&rig.host.url(HOST, "/image.png")))
        .body(Body::empty())
        .unwrap();
    let response = send(&rig.router, request).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(rig.lookups(), 0);
    assert!(rig.requests().is_empty());
}

#[tokio::test]
async fn a_url_off_the_rules_is_refused_before_anything_resolves() {
    let rig = rig().await;
    let cookie = sign_in(&rig.router).await;
    let port = rig.host.port();
    let base = rig.host.url(HOST, "/");
    let long = format!("{base}{}", "a".repeat(REMOTE_URL_BYTES + 1 - base.len()));
    for url in [
        "file:///etc/passwd".to_owned(),
        format!("ftp://{HOST}:{port}/image.png"),
        "data:image/png;base64,iVBORw0KGgo=".to_owned(),
        format!("https://sanne:secret@{HOST}:{port}/image.png"),
        format!("https://127.0.0.1:{port}/image.png"),
        format!("https://[::1]:{port}/image.png"),
        long,
        String::new(),
    ] {
        let (status, _, body) = refused(&rig.router, &cookie, &url).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{url}");
        assert!(body.contains("invalid_request"), "{url}: {body}");
    }
    let without_url = send(
        &rig.router,
        with_cookie(Method::GET, "/api/remote-image", &cookie),
    )
    .await;
    assert_eq!(without_url.status(), StatusCode::BAD_REQUEST);
    assert!(body_text(without_url).await.contains("invalid_request"));
    assert_eq!(rig.lookups(), 0);
    assert!(rig.requests().is_empty());
    let fitting = format!("{base}{}", "a".repeat(REMOTE_URL_BYTES - base.len()));
    let (status, _, _) = fetch(&rig.router, &cookie, &fitting).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
}

#[tokio::test]
async fn a_private_host_and_a_redirect_into_one_are_refused_before_the_connect() {
    let rig = rig().await;
    let cookie = sign_in(&rig.router).await;
    let (status, _, body) = refused(
        &rig.router,
        &cookie,
        &rig.host.url(INWARD_HOST, "/image.png"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("invalid_request"), "{body}");
    assert_eq!(rig.lookups(), 1);
    assert!(rig.requests().is_empty());
    let (status, _, body) = refused(&rig.router, &cookie, &rig.host.url(HOST, "/to/private")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("invalid_request"), "{body}");
    assert_eq!(rig.requests(), ["/to/private"]);
}

#[tokio::test]
async fn an_http_url_is_fetched_over_https_and_so_is_a_redirect_to_one() {
    let rig = rig().await;
    let cookie = sign_in(&rig.router).await;
    let plain = format!("http://{HOST}:{}/image.png", rig.host.port());
    let (status, headers, body) = fetch(&rig.router, &cookie, &plain).await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(headers[header::CONTENT_TYPE], "image/png");
    assert_eq!(body, PNG);
    let (status, _, body) = fetch(&rig.router, &cookie, &rig.host.url(HOST, "/to/http")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, PNG);
    assert_eq!(rig.requests(), ["/image.png", "/to/http", "/image.png"]);
}

#[tokio::test]
async fn three_redirects_are_followed_and_a_fourth_is_not() {
    let rig = rig().await;
    let cookie = sign_in(&rig.router).await;
    let (status, _, body) = fetch(&rig.router, &cookie, &rig.host.url(HOST, "/three/0")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, PNG);
    assert_eq!(rig.requests().len(), REMOTE_REDIRECTS + 1);
    for (chain, fetches) in [("/hop", 2), ("/astray", 3)] {
        let url = rig.host.url(HOST, &format!("{chain}/0"));
        let (status, _, body) = refused(&rig.router, &cookie, &url).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY, "{chain}");
        assert!(body.contains("upstream_unreachable"), "{chain}: {body}");
        assert_eq!(rig.requests().len(), fetches * (REMOTE_REDIRECTS + 1));
        assert_eq!(
            rig.requests().last().unwrap(),
            &format!("{chain}/{REMOTE_REDIRECTS}")
        );
    }
}

#[tokio::test]
async fn a_redirect_is_checked_like_the_first_url() {
    let rig = rig().await;
    let cookie = sign_in(&rig.router).await;
    let port = rig.host.port();
    let (status, _, body) = fetch(&rig.router, &cookie, &rig.host.url(HOST, "/to/other")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, PNG);
    let seen = rig.host.seen();
    assert_eq!(seen[1].0, "/image.png");
    assert_eq!(seen[1].1[header::HOST], format!("{OTHER_HOST}:{port}"));
    for name in [header::COOKIE, header::REFERER, header::AUTHORIZATION] {
        assert!(seen[1].1.get(&name).is_none(), "{name} on the hop");
    }
    let (status, _, body) = fetch(&rig.router, &cookie, &rig.host.url(HOST, "/to/relative")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, PNG);
    for (target, expected, code) in [
        ("user", StatusCode::BAD_REQUEST, "invalid_request"),
        ("literal", StatusCode::BAD_REQUEST, "invalid_request"),
        ("nowhere", StatusCode::BAD_GATEWAY, "upstream_unreachable"),
    ] {
        let url = rig.host.url(HOST, &format!("/to/{target}"));
        let (status, _, body) = refused(&rig.router, &cookie, &url).await;
        assert_eq!(status, expected, "{target}");
        assert!(body.contains(code), "{target}: {body}");
    }
    assert_eq!(rig.requests().len(), 7);
}

#[tokio::test]
async fn bytes_that_are_no_raster_image_are_refused() {
    let rig = rig().await;
    let cookie = sign_in(&rig.router).await;
    for path in ["/page.html", "/logo.svg"] {
        let (status, headers, body) =
            refused(&rig.router, &cookie, &rig.host.url(HOST, path)).await;
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{path}");
        assert!(body.contains("not_an_image"), "{path}: {body}");
        assert_eq!(headers[header::CONTENT_TYPE], "application/json");
    }
}

#[tokio::test]
async fn an_image_past_the_bound_is_refused_declared_or_not() {
    let rig = rig().await;
    let cookie = sign_in(&rig.router).await;
    for path in ["/declared-large", "/endless"] {
        let (status, _, body) = refused(&rig.router, &cookie, &rig.host.url(HOST, path)).await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{path}");
        assert!(body.contains("too_large"), "{path}: {body}");
    }
}

#[tokio::test]
async fn a_host_that_answers_anything_but_an_image_or_a_redirect_is_unreachable() {
    let rig = rig().await;
    let cookie = sign_in(&rig.router).await;
    let unknown = format!("https://unknown.test:{}/image.png", rig.host.port());
    for url in [
        rig.host.url(HOST, "/status/404"),
        rig.host.url(HOST, "/status/500"),
        rig.host.url(HOST, "/status/204"),
        unknown,
    ] {
        let (status, _, body) = refused(&rig.router, &cookie, &url).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY, "{url}");
        assert!(body.contains("upstream_unreachable"), "{url}: {body}");
    }
    assert_eq!(rig.requests().len(), 3);
}

#[tokio::test]
async fn the_request_carries_nothing_of_the_reader_and_the_answer_the_blob_headers() {
    let rig = rig().await;
    let cookie = sign_in(&rig.router).await;
    let url = rig.host.url(HOST, "/image.png");
    let (status, headers, body) = fetch(&rig.router, &cookie, &url).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "image/png");
    assert_eq!(headers[header::CONTENT_LENGTH], PNG.len().to_string());
    assert!(headers.get(header::CONTENT_DISPOSITION).is_none());
    assert_blob_headers(&headers);
    assert_eq!(body, PNG);
    let (path, seen) = rig.host.seen().remove(0);
    assert_eq!(path, "/image.png");
    for name in [header::COOKIE, header::REFERER, header::AUTHORIZATION] {
        assert!(seen.get(&name).is_none(), "{name}");
    }
    assert_eq!(seen[header::USER_AGENT], REMOTE_USER_AGENT);
    let strict = common::router_with(ApiState {
        privacy_strict: true,
        ..rig.api.clone()
    });
    let (status, headers, _) = fetch(&strict, &cookie, &url).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    for (name, value) in &BLOB_HEADERS[..4] {
        assert_eq!(headers.get(*name).unwrap(), value, "{name}");
    }
}

#[tokio::test]
async fn a_session_draws_from_its_own_allowance_and_meets_429_past_it() {
    let rig = rig().await;
    let cookie = sign_in(&rig.router).await;
    let inward = rig.host.url(INWARD_HOST, "/image.png");
    let mut met = None;
    for n in 0..=REMOTE_IMAGE_BURST + REFILL_MARGIN {
        let (status, headers, body) = refused(&rig.router, &cookie, &inward).await;
        if status == StatusCode::TOO_MANY_REQUESTS {
            met = Some((n, headers, body));
            break;
        }
        assert_eq!(status, StatusCode::BAD_REQUEST, "{n}: {body}");
    }
    let (n, headers, body) = met.expect("the allowance runs out");
    assert!(n >= REMOTE_IMAGE_BURST, "429 after {n} fetches");
    assert!(body.contains("rate_limited"), "{body}");
    assert!(headers.get(header::RETRY_AFTER).is_some());
    let (status, _, _) = refused(&rig.router, &cookie, "file:///etc/passwd").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let other = rig.sign_in_again().await;
    let (status, _, _) = refused(&rig.router, &other, &inward).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(rig.requests().is_empty());
}

#[tokio::test]
async fn a_fetch_in_flight_holds_a_slot_and_the_next_one_waits_for_it() {
    let rig = Arc::new(rig().await);
    let cookie = sign_in(&rig.router).await;
    let one_slot = common::router_with(ApiState {
        remote_fetches: Arc::new(Semaphore::new(1)),
        ..rig.api.clone()
    });
    let parked = {
        let (rig, router, cookie) = (Arc::clone(&rig), one_slot.clone(), cookie.clone());
        tokio::spawn(async move { fetch(&router, &cookie, &rig.host.url(HOST, "/parked")).await })
    };
    let mut waited = 0;
    while rig.requests().is_empty() && waited < PATIENCE {
        sleep(POLL).await;
        waited += 1;
    }
    assert_eq!(rig.requests(), ["/parked"]);
    let next = {
        let (rig, router, cookie) = (Arc::clone(&rig), one_slot, cookie);
        tokio::spawn(
            async move { fetch(&router, &cookie, &rig.host.url(HOST, "/image.png")).await },
        )
    };
    sleep(POLL * 10).await;
    assert_eq!(
        rig.requests(),
        ["/parked"],
        "the second fetch started while the slot was held"
    );
    rig.host.release();
    let (status, _, body) = parked.await.unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, PNG);
    let (status, _, body) = next.await.unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, PNG);
    assert_eq!(rig.requests(), ["/parked", "/image.png"]);
}
