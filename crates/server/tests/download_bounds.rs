// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The download route under its bounds: the template it will and will
//! not expand, the size it refuses and the size it breaks off at, the
//! one range it honors and the lanes an account has.

mod answers;
mod common;
mod download_rig;
mod fake_dns;
mod jmap_upstream;
mod proxy_rig;
mod readers;
mod signin;
mod tls_server;

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Method, Response, StatusCode, header};
use download_rig::{NAME, assert_blob_headers, bearer, download, downloads, instance, path};
use huliho_server::ids::AccountId;
use huliho_server::mail::detect::DETECT_BYTES;
use huliho_server::mail::download::{BLOB_DOWNLOAD_LIMIT, MAX_CONCURRENT_DOWNLOADS};
use jmap_upstream::{Blob, HOST, PNG, Script};
use proxy_rig::{Instance, query_request};
use signin::with_cookie;
use tokio::task::JoinSet;
use tower::ServiceExt;

/// How often and how long the lane tests look for the parked
/// downloads.
const POLL: Duration = Duration::from_millis(10);
const PATIENCE: u32 = 300;

/// A download up to its headers, the body left unread.
async fn started(instance: &Instance, cookie: &str, uri: &str) -> Response<Body> {
    let request = with_cookie(Method::GET, uri, cookie);
    instance.router.clone().oneshot(request).await.unwrap()
}

#[tokio::test]
async fn another_users_range_request_is_not_found_and_nothing_connects() {
    let instance = instance().await;
    let id = instance.add_account(bearer());
    let other = instance.sign_in_other().await;
    let uri = path(&id, NAME, Some("image/png"));
    let (status, _, body) = download(&instance.router, &other, &uri, Some("bytes=0-99")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body:?}");
    assert!(instance.upstream.lines().is_empty());
    assert_eq!(
        instance.account_events(),
        [(
            "account.linked".to_owned(),
            instance.user_id().as_str().to_owned()
        )]
    );
}

#[tokio::test]
async fn a_template_lacking_a_variable_or_off_the_rules_is_not_usable() {
    let instance = instance().await;
    let id = instance.add_account(bearer());
    let cookie = instance.sign_in().await;
    let port = instance.upstream.port();
    for template in [
        format!("https://{HOST}:{port}/jmap/download/{{accountId}}/{{blobId}}/{{name}}"),
        format!("https://{HOST}:{port}/jmap/download/{{+path}}"),
        format!(
            "https://sanne:secret@{HOST}:{port}/jmap/download/{{accountId}}/{{blobId}}/{{name}}?accept={{type}}"
        ),
        format!(
            "http://{HOST}:{port}/jmap/download/{{accountId}}/{{blobId}}/{{name}}?accept={{type}}"
        ),
    ] {
        instance.upstream.set(Script {
            download_url: template.clone(),
            ..Script::echo(port)
        });
        let (status, _, body) =
            download(&instance.router, &cookie, &path(&id, NAME, None), None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{template}");
        let body = String::from_utf8(body.unwrap()).unwrap();
        assert!(body.contains("upstream_unsupported"), "{template}: {body}");
        assert_eq!(instance.stopped_cause(&id), None, "{template}");
        // The template is learned once per account; the next one needs
        // a fresh session object.
        instance.api.endpoints.forget(&AccountId::from(id.clone()));
    }
    assert!(downloads(&instance).is_empty());
    instance.upstream.set(Script {
        download_url: "/jmap/download/{accountId}/{blobId}/{name}?accept={type}".to_owned(),
        ..Script::echo(port)
    });
    let (status, headers, body) = download(
        &instance.router,
        &cookie,
        &path(&id, NAME, Some("image/png")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{headers:?}");
    assert_eq!(body.unwrap(), PNG);
    assert_eq!(downloads(&instance).len(), 1);
}

#[tokio::test]
async fn a_declared_size_past_the_limit_is_refused_before_any_byte() {
    let instance = instance().await;
    let id = instance.add_account(bearer());
    let cookie = instance.sign_in().await;
    instance.upstream.set(Script {
        blob: Blob {
            declared: Some(BLOB_DOWNLOAD_LIMIT + 1),
            endless: true,
            ..Blob::of(PNG)
        },
        ..Script::echo(instance.upstream.port())
    });
    let (status, _, body) = download(&instance.router, &cookie, &path(&id, NAME, None), None).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    let body = String::from_utf8(body.unwrap()).unwrap();
    assert!(body.contains("too_large"), "{body}");
    assert_eq!(downloads(&instance).len(), 1);
}

#[tokio::test]
async fn an_undeclared_stream_past_the_limit_ends_the_answer() {
    let instance = instance().await;
    let id = instance.add_account(bearer());
    let cookie = instance.sign_in().await;
    instance.upstream.set(Script {
        blob: Blob {
            declared: None,
            endless: true,
            ..Blob::of(PNG)
        },
        ..Script::echo(instance.upstream.port())
    });
    let (status, headers, body) = download(
        &instance.router,
        &cookie,
        &path(&id, NAME, Some("image/png")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "image/png");
    assert!(headers.get(header::CONTENT_LENGTH).is_none());
    assert!(body.unwrap_err().contains("past the limit"));
}

#[tokio::test]
async fn a_range_from_byte_zero_answers_206_with_that_many_bytes() {
    let instance = instance().await;
    let id = instance.add_account(bearer());
    let cookie = instance.sign_in().await;
    let long: Vec<u8> = PNG
        .iter()
        .copied()
        .chain(std::iter::repeat_n(b'z', 1000))
        .collect();
    instance.upstream.set(Script {
        blob: Blob::of(&long),
        ..Script::echo(instance.upstream.port())
    });
    let uri = path(&id, NAME, Some("image/png"));
    let (status, headers, body) =
        download(&instance.router, &cookie, &uri, Some("bytes=0-99")).await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(headers[header::CONTENT_LENGTH], "100");
    assert_eq!(
        headers[header::CONTENT_RANGE],
        format!("bytes 0-99/{}", long.len())
    );
    assert_eq!(headers[header::CONTENT_TYPE], "image/png");
    assert_blob_headers(&headers);
    assert_eq!(body.unwrap(), long[..100]);
    for range in ["bytes=0-5000", "bytes=5-99", "bytes=-100"] {
        let (status, headers, body) = download(&instance.router, &cookie, &uri, Some(range)).await;
        assert_eq!(status, StatusCode::OK, "{range}");
        assert!(headers.get(header::CONTENT_RANGE).is_none(), "{range}");
        assert_eq!(body.unwrap().len(), long.len(), "{range}");
    }
}

#[tokio::test]
async fn a_third_download_waits_for_a_lane_while_two_stream_and_a_request_still_runs() {
    let instance = Arc::new(instance().await);
    let id = instance.add_account(bearer());
    let cookie = instance.sign_in().await;
    let (status, _) = instance.session(&cookie, &id).await;
    assert_eq!(status, StatusCode::OK);
    let port = instance.upstream.port();
    instance.upstream.set(Script {
        blob: Blob {
            hold: true,
            ..Blob::of(PNG)
        },
        ..Script::echo(port)
    });
    let mut running = JoinSet::new();
    for _ in 0..=MAX_CONCURRENT_DOWNLOADS {
        let (instance, cookie, id) = (Arc::clone(&instance), cookie.clone(), id.clone());
        running.spawn(async move {
            download(
                &instance.router,
                &cookie,
                &path(&id, NAME, Some("image/png")),
                None,
            )
            .await
        });
    }
    let mut waited = 0;
    while downloads(&instance).len() < MAX_CONCURRENT_DOWNLOADS && waited < PATIENCE {
        tokio::time::sleep(POLL).await;
        waited += 1;
    }
    tokio::time::sleep(POLL * 5).await;
    assert_eq!(downloads(&instance).len(), MAX_CONCURRENT_DOWNLOADS);
    let (status, answer) = instance.request(&cookie, &id, &query_request()).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    instance.upstream.set(Script::echo(port));
    while let Some(finished) = running.join_next().await {
        let (status, _, body) = finished.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.unwrap(), PNG);
    }
    assert_eq!(downloads(&instance).len(), MAX_CONCURRENT_DOWNLOADS + 1);
}

#[tokio::test]
async fn two_streaming_bodies_hold_the_lanes_until_they_end() {
    let instance = Arc::new(instance().await);
    let id = instance.add_account(bearer());
    let cookie = instance.sign_in().await;
    let port = instance.upstream.port();
    // A blob whose head arrives whole and whose body then never ends.
    instance.upstream.set(Script {
        blob: Blob {
            bytes: vec![b'a'; DETECT_BYTES + 100],
            declared: None,
            stalls: true,
            ..Blob::of(PNG)
        },
        ..Script::echo(port)
    });
    let uri = path(&id, NAME, None);
    let first = started(&instance, &cookie, &uri).await;
    let second = started(&instance, &cookie, &uri).await;
    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(second.status(), StatusCode::OK);
    assert_eq!(downloads(&instance).len(), MAX_CONCURRENT_DOWNLOADS);
    instance.upstream.set(Script::echo(port));
    let third = {
        let (instance, cookie, uri) = (Arc::clone(&instance), cookie.clone(), uri.clone());
        tokio::spawn(async move { download(&instance.router, &cookie, &uri, None).await })
    };
    tokio::time::sleep(POLL * 10).await;
    assert_eq!(
        downloads(&instance).len(),
        MAX_CONCURRENT_DOWNLOADS,
        "the third download started while two bodies streamed"
    );
    drop(first);
    drop(second);
    let (status, _, body) = third.await.unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.unwrap(), PNG);
    assert_eq!(downloads(&instance).len(), MAX_CONCURRENT_DOWNLOADS + 1);
}
