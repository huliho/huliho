// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The download route of a native account: who reaches a blob, what
//! its answer says about itself and what the upstream sees of the
//! request.

mod answers;
mod common;
mod download_rig;
mod fake_dns;
mod jmap_upstream;
mod proxy_rig;
mod readers;
mod signin;
mod tls_server;

use std::time::Duration;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use download_rig::{
    BLOB, BLOB_HEADERS, NAME, assert_blob_headers, bearer, download, downloads, instance, path,
};
use huliho_imap_bridge::testing::{CLOSED, TOKEN};
use huliho_server::accounts::{self, StopCause};
use huliho_server::api::ApiState;
use huliho_server::events::Actor;
use huliho_server::gate::MAX_REFUSED_RUN;
use huliho_server::ids::AccountId;
use huliho_server::scope;
use jmap_upstream::{Blob, HOST, PNG, Script, UPSTREAM_ACCOUNT};
use proxy_rig::{Instance, Setup, query_request};
use signin::{body_text, with_cookie};
use tower::ServiceExt;

#[tokio::test]
async fn another_users_account_is_not_found_and_nothing_connects() {
    let instance = instance().await;
    let id = instance.add_account(bearer());
    let other = instance.sign_in_other().await;
    let (status, _, body) = download(
        &instance.router,
        &other,
        &path(&id, NAME, Some("image/png")),
        None,
    )
    .await;
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
async fn the_route_needs_a_session_like_the_request_route_and_never_the_header() {
    let instance = instance().await;
    let id = instance.add_account(bearer());
    let uri = path(&id, NAME, Some("image/png"));
    let (status, _, _) = download(&instance.router, "huliho_session=stale", &uri, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = instance
        .request("huliho_session=stale", &id, &query_request())
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let bare = Request::builder()
        .method(Method::GET)
        .uri(&uri)
        .body(Body::empty())
        .unwrap();
    let response = instance.router.clone().oneshot(bare).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(instance.upstream.lines().is_empty());
    let cookie = instance.sign_in().await;
    let without_header = Request::builder()
        .method(Method::GET)
        .uri(&uri)
        .header(header::COOKIE, &cookie)
        .body(Body::empty())
        .unwrap();
    let response = instance
        .router
        .clone()
        .oneshot(without_header)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn a_stopped_account_answers_409_before_anything_connects() {
    let instance = instance().await;
    let id = instance.add_account(bearer());
    let cookie = instance.sign_in().await;
    let scope = scope::resolve(
        &instance.store,
        &instance.user_id(),
        Some(&AccountId::from(id.clone())),
    )
    .unwrap();
    accounts::stop(
        &instance.store,
        &scope,
        StopCause::Connection,
        &Actor::System,
    )
    .unwrap();
    let (status, _, body) = download(&instance.router, &cookie, &path(&id, NAME, None), None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let body = String::from_utf8(body.unwrap()).unwrap();
    assert!(body.contains("still_stopped"), "{body}");
    assert!(instance.upstream.lines().is_empty());
    assert_eq!(instance.stopped_cause(&id).as_deref(), Some("connection"));
}

#[tokio::test]
async fn a_png_asked_as_itself_answers_inline_with_the_credential_on_the_expanded_template() {
    let instance = instance().await;
    let id = instance.add_account(bearer());
    let cookie = instance.sign_in().await;
    let (status, _) = instance.session(&cookie, &id).await;
    assert_eq!(status, StatusCode::OK);
    let uri = path(&id, "Q3%20photo.png", Some("image/png"));
    let (status, headers, body) = download(&instance.router, &cookie, &uri, None).await;
    assert_eq!(status, StatusCode::OK, "{headers:?}");
    assert_eq!(headers[header::CONTENT_TYPE], "image/png");
    assert_eq!(
        headers[header::CONTENT_DISPOSITION],
        "inline; filename*=UTF-8''Q3%20photo.png"
    );
    assert_eq!(headers[header::CONTENT_LENGTH], PNG.len().to_string());
    assert_blob_headers(&headers);
    assert_eq!(body.unwrap(), PNG);
    // The session object named the template; the blob request carries
    // the credential and the expanded path and nothing else.
    let lines = downloads(&instance);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].contains(&format!(
            "GET {HOST}:{} /jmap/download/{UPSTREAM_ACCOUNT}/{BLOB}/Q3%20photo.png?accept=image%2Fpng Bearer {TOKEN}",
            instance.upstream.port()
        )),
        "{lines:?}"
    );
    let (status, _, _) = download(&instance.router, &cookie, &uri, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(downloads(&instance).len(), 2);
}

#[tokio::test]
async fn an_svg_and_an_html_file_named_png_and_a_png_asked_otherwise_are_downloads() {
    let instance = instance().await;
    let id = instance.add_account(bearer());
    let cookie = instance.sign_in().await;
    let port = instance.upstream.port();
    let svg = b"<?xml version=\"1.0\"?><svg xmlns=\"http://www.w3.org/2000/svg\"><script>top.__x=1</script></svg>";
    let html = b"<html><body><script>top.__x=1</script></body></html>";
    for (bytes, name, media_type) in [
        (&svg[..], "logo.svg", Some("image/svg+xml")),
        (&html[..], "photo.png", Some("image/png")),
        (PNG, "photo.png", Some("image/jpeg")),
        (PNG, "photo.png", None),
    ] {
        instance.upstream.set(Script {
            blob: Blob::of(bytes),
            ..Script::echo(port)
        });
        let (status, headers, body) = download(
            &instance.router,
            &cookie,
            &path(&id, name, media_type),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{name} {media_type:?}");
        assert_eq!(
            headers[header::CONTENT_TYPE],
            "application/octet-stream",
            "{name} {media_type:?}"
        );
        assert_eq!(
            headers[header::CONTENT_DISPOSITION],
            format!("attachment; filename*=UTF-8''{name}"),
            "{name} {media_type:?}"
        );
        assert_blob_headers(&headers);
        assert_eq!(body.unwrap(), bytes);
    }
}

#[tokio::test]
async fn a_name_comes_back_cleaned_and_cut() {
    let instance = instance().await;
    let id = instance.add_account(bearer());
    let cookie = instance.sign_in().await;
    let control = path(&id, "re%0Aport%7F.png", Some("image/png"));
    let (status, headers, _) = download(&instance.router, &cookie, &control, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers[header::CONTENT_DISPOSITION],
        "inline; filename*=UTF-8''report.png"
    );
    let long = path(&id, &"x".repeat(1000), Some("image/png"));
    let (status, headers, _) = download(&instance.router, &cookie, &long, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers[header::CONTENT_DISPOSITION],
        format!("inline; filename*=UTF-8''{}", "x".repeat(255))
    );
}

#[tokio::test]
async fn under_strict_a_blob_is_never_stored() {
    let instance = instance().await;
    let strict = common::router_with(ApiState {
        privacy_strict: true,
        ..instance.api.clone()
    });
    let id = instance.add_account(bearer());
    let cookie = instance.sign_in().await;
    let (status, headers, _) =
        download(&strict, &cookie, &path(&id, NAME, Some("image/png")), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    assert_eq!(headers[header::CONTENT_TYPE], "image/png");
    for (name, value) in &BLOB_HEADERS[..4] {
        assert_eq!(headers.get(*name).unwrap(), value, "{name}");
    }
    let session = strict
        .clone()
        .oneshot(with_cookie(Method::GET, "/api/session", &cookie))
        .await
        .unwrap();
    assert!(body_text(session).await.contains("\"privacyStrict\":true"));
}

#[tokio::test]
async fn what_the_upstream_answers_beside_the_blob_has_its_own_word() {
    let instance = instance().await;
    let id = instance.add_account(bearer());
    let cookie = instance.sign_in().await;
    let port = instance.upstream.port();
    for (status, expected, code) in [
        (404, StatusCode::NOT_FOUND, "not_found"),
        (503, StatusCode::BAD_GATEWAY, "upstream_failed"),
        (302, StatusCode::BAD_REQUEST, "upstream_unsupported"),
    ] {
        instance.upstream.set(Script {
            blob: Blob {
                status,
                ..Blob::of(PNG)
            },
            ..Script::echo(port)
        });
        let (answered, _, body) =
            download(&instance.router, &cookie, &path(&id, NAME, None), None).await;
        assert_eq!(answered, expected, "{status}");
        let body = String::from_utf8(body.unwrap()).unwrap();
        assert!(body.contains(code), "{status}: {body}");
        assert_eq!(instance.stopped_cause(&id), None, "{status}");
    }
    instance.upstream.set(Script {
        blob: Blob {
            status: 401,
            ..Blob::of(PNG)
        },
        ..Script::echo(port)
    });
    let (status, _, body) = download(&instance.router, &cookie, &path(&id, NAME, None), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body:?}");
    assert_eq!(instance.stopped_cause(&id).as_deref(), Some("credentials"));
}

#[tokio::test]
async fn a_blob_that_is_not_there_leaves_a_run_of_refused_connections_standing() {
    let instance = Instance::start(Setup {
        window: Duration::ZERO,
        servers: &[],
    })
    .await;
    let id = instance.add_account(bearer());
    let cookie = instance.sign_in().await;
    // The API endpoint sits on a port nothing listens on; the download
    // endpoint answers on the live one.
    instance.upstream.set(Script {
        api_url: format!("https://{HOST}:{}/jmap/api", CLOSED.port()),
        blob: Blob {
            status: StatusCode::NOT_FOUND.as_u16(),
            ..Blob::of(PNG)
        },
        ..Script::echo(instance.upstream.port())
    });
    for _ in 1..MAX_REFUSED_RUN {
        let (status, body) = instance.request(&cookie, &id, &query_request()).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    }
    let (status, _, _) = download(&instance.router, &cookie, &path(&id, NAME, None), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(instance.stopped_cause(&id), None);
    let (status, body) = instance.request(&cookie, &id, &query_request()).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert_eq!(instance.stopped_cause(&id).as_deref(), Some("connection"));
}
