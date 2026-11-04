// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The upload route of a native account (RFC 8620 section 6.1): the
//! session, the header and the scope before anything connects, the type
//! and the declared length judged first, the body streamed to the
//! template the upstream named with the account's credential, the
//! upstream's answer passed back or worded, the lanes an account has.

mod answers;
mod common;
mod fake_dns;
mod jmap_upstream;
mod proxy_rig;
mod readers;
mod signin;
mod tls_server;

use std::sync::Arc;
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::http::{Method, Request, StatusCode, header};
use futures_util::stream;
use huliho_imap_bridge::testing::TOKEN;
use huliho_server::accounts::{self, Credential, StopCause};
use huliho_server::events::Actor;
use huliho_server::gate::RUN_WINDOW;
use huliho_server::ids::AccountId;
use huliho_server::mail::upload::{MAX_CONCURRENT_UPLOADS, UPLOAD_LIMIT};
use huliho_server::scope;
use jmap_upstream::upload::blob_id;
use jmap_upstream::{HOST, INWARD_HOST, Script, UPSTREAM_ACCOUNT, Upload};
use proxy_rig::{Instance, Setup, query_request};
use serde_json::{Value, json};
use signin::with_cookie;
use tokio::task::JoinSet;
use tower::ServiceExt;

/// How often and how long the lane test looks for the parked uploads.
const POLL: Duration = Duration::from_millis(10);
const PATIENCE: u32 = 300;

/// A file of a few windows of the client's chunks.
const FILE_BYTES: usize = 300 * 1024;

/// The instance over the fixture upstream with a JMAP account of the
/// fixture user on its token; the account's id beside it.
async fn instance() -> (Instance, String) {
    let setup = Setup {
        window: RUN_WINDOW,
        servers: &[],
    };
    let instance = Instance::start(setup).await;
    let token = TOKEN.to_owned();
    let id = instance.add_account(Credential::Bearer { token });
    (instance, id)
}

fn file() -> Vec<u8> {
    (0..FILE_BYTES)
        .map(|index| u8::try_from(index % 251).unwrap())
        .collect()
}

fn uri(id: &str) -> String {
    format!("/api/jmap/{id}/upload/{UPSTREAM_ACCOUNT}")
}

/// The uploads the fixture upstream saw, as their request lines.
fn uploads(instance: &Instance) -> Vec<String> {
    let lines = instance.upstream.lines();
    lines
        .into_iter()
        .filter(|line| line.contains("/jmap/upload/"))
        .collect()
}

/// A POST of `body` to the route, typed and with its length as a browser
/// sends a file.
fn post(cookie: &str, id: &str, media_type: Option<&str>, body: Vec<u8>) -> Request<Body> {
    let mut request = with_cookie(Method::POST, &uri(id), cookie);
    if let Some(media_type) = media_type {
        request
            .headers_mut()
            .insert(header::CONTENT_TYPE, media_type.parse().unwrap());
    }
    request
        .headers_mut()
        .insert(header::CONTENT_LENGTH, body.len().into());
    *request.body_mut() = Body::from(body);
    request
}

/// The status and the answer as JSON, or `Null` where it is none.
async fn sent(instance: &Instance, request: Request<Body>) -> (StatusCode, Value) {
    let response = instance.router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn another_users_account_is_not_found_and_nothing_reaches_the_upstream() {
    let (instance, id) = instance().await;
    let other = instance.sign_in_other().await;
    let (status, _) = sent(&instance, post(&other, &id, Some("image/png"), file())).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(instance.upstream.lines(), Vec::<String>::new());
    assert_eq!(instance.account_events().len(), 1);
}

#[tokio::test]
async fn the_route_needs_a_session_and_the_header_before_anything_connects() {
    let (instance, id) = instance().await;
    let (status, _) = sent(&instance, post("huliho_session=stale", &id, None, file())).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let cookie = instance.sign_in().await;
    let mut bare = post(&cookie, &id, None, file());
    bare.headers_mut().remove("x-requested-with");
    let (status, answer) = sent(&instance, bare).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(answer["error"], "missing_csrf_header");
    assert_eq!(instance.upstream.lines(), Vec::<String>::new());
}

#[tokio::test]
async fn a_stopped_account_answers_409_before_anything_connects() {
    let (instance, id) = instance().await;
    let cookie = instance.sign_in().await;
    let account = AccountId::from(id.clone());
    let scope = scope::resolve(&instance.store, &instance.user_id(), Some(&account)).unwrap();
    accounts::stop(
        &instance.store,
        &scope,
        StopCause::Connection,
        &Actor::System,
    )
    .unwrap();
    let (status, answer) = sent(&instance, post(&cookie, &id, None, file())).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(answer["error"], "still_stopped");
    assert_eq!(instance.upstream.lines(), Vec::<String>::new());
    assert_eq!(instance.stopped_cause(&id).as_deref(), Some("connection"));
}

#[tokio::test]
async fn a_file_streams_to_the_template_with_its_type_and_the_answer_comes_back_rfc8620_6_1() {
    let (instance, id) = instance().await;
    let cookie = instance.sign_in().await;
    let (status, answer) = sent(
        &instance,
        post(&cookie, &id, Some("application/pdf"), file()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    // The fixture's answer as it came: the blob id is a digest of what
    // arrived, `declared` the length the forward stated.
    let expected = json!({
        "accountId": UPSTREAM_ACCOUNT,
        "blobId": blob_id(&file()),
        "type": "application/pdf",
        "size": FILE_BYTES,
        "declared": FILE_BYTES.to_string(),
    });
    assert_eq!(answer, expected);
    let posted = uploads(&instance);
    assert_eq!(posted.len(), 1);
    assert!(
        posted[0].contains(&format!("/jmap/upload/{UPSTREAM_ACCOUNT}/")),
        "{posted:?}"
    );
    assert!(posted[0].contains(&format!("Bearer {TOKEN}")), "{posted:?}");
}

#[tokio::test]
async fn the_type_is_judged_before_anything_connects() {
    let (instance, id) = instance().await;
    let cookie = instance.sign_in().await;
    for malformed in ["image", "ima ge/png", "image/"] {
        let (status, answer) = sent(&instance, post(&cookie, &id, Some(malformed), file())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{malformed}");
        assert_eq!(answer["error"], "invalid_request", "{malformed}");
    }
    assert_eq!(instance.upstream.lines(), Vec::<String>::new());
    let (status, answer) = sent(&instance, post(&cookie, &id, None, file())).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer["type"], "application/octet-stream");
}

#[tokio::test]
async fn a_length_past_the_route_is_refused_before_anything_connects_and_a_body_past_it_is_cut() {
    let (instance, id) = instance().await;
    let cookie = instance.sign_in().await;
    let mut declared = post(&cookie, &id, None, file());
    declared
        .headers_mut()
        .insert(header::CONTENT_LENGTH, (UPLOAD_LIMIT + 1).into());
    let (status, answer) = sent(&instance, declared).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(answer["error"], "too_large");
    assert_eq!(instance.upstream.lines(), Vec::<String>::new());
    let mut broken = with_cookie(Method::POST, &uri(&id), &cookie);
    let chunks: Vec<Result<Bytes, std::io::Error>> = vec![
        Ok(Bytes::from(file())),
        Err(std::io::Error::other("the client went away")),
    ];
    *broken.body_mut() = Body::from_stream(stream::iter(chunks));
    let (status, answer) = sent(&instance, broken).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{answer}");
    assert_eq!(answer["error"], "invalid_request");
    assert_eq!(instance.stopped_cause(&id), None);
}

#[tokio::test]
async fn what_the_upstream_answers_has_its_own_word() {
    let (instance, id) = instance().await;
    let cookie = instance.sign_in().await;
    let port = instance.upstream.port();
    let foreign = br#"{"accountId":"u9","blobId":"G1","type":"image/png","size":1}"#.to_vec();
    for (status, answer, expected, word) in [
        (413, None, StatusCode::PAYLOAD_TOO_LARGE, "too_large"),
        (429, None, StatusCode::TOO_MANY_REQUESTS, "rate_limited"),
        (503, None, StatusCode::BAD_GATEWAY, "upstream_failed"),
        (302, None, StatusCode::BAD_REQUEST, "upstream_unsupported"),
        (404, None, StatusCode::BAD_REQUEST, "upstream_unsupported"),
        (
            201,
            Some(b"[]".to_vec()),
            StatusCode::BAD_REQUEST,
            "upstream_unsupported",
        ),
        (
            201,
            Some(foreign.clone()),
            StatusCode::BAD_REQUEST,
            "upstream_unsupported",
        ),
        (401, None, StatusCode::UNAUTHORIZED, "upstream_credentials"),
    ] {
        instance.upstream.set(Script {
            upload: Upload {
                status,
                answer,
                hold: false,
            },
            ..Script::echo(port)
        });
        let (got, body) = sent(&instance, post(&cookie, &id, None, file())).await;
        assert_eq!(got, expected, "{status}: {body}");
        assert_eq!(body["error"], word, "{status}");
    }
    // The upstream's 401 reached the gate as the proxy's does.
    assert_eq!(instance.stopped_cause(&id).as_deref(), Some("credentials"));
}

#[tokio::test]
async fn a_template_off_the_rules_is_not_usable_and_nothing_connects() {
    let (instance, id) = instance().await;
    let cookie = instance.sign_in().await;
    let port = instance.upstream.port();
    for template in [
        format!("https://{HOST}:{port}/jmap/upload/"),
        format!("https://{HOST}:{port}/jmap/upload/{{accountId}}/{{blobId}}"),
        format!("http://{HOST}:{port}/jmap/upload/{{accountId}}/"),
        format!("https://sanne:secret@{HOST}:{port}/jmap/upload/{{accountId}}/"),
        format!("https://{INWARD_HOST}:{port}/jmap/upload/{{accountId}}/"),
    ] {
        instance.upstream.set(Script {
            upload_url: template.clone(),
            ..Script::echo(port)
        });
        let (status, answer) = sent(&instance, post(&cookie, &id, None, file())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{template}");
        assert_eq!(answer["error"], "upstream_unsupported", "{template}");
        // The template is learned once per account; the next one needs
        // a fresh session object.
        instance.api.endpoints.forget(&AccountId::from(id.clone()));
    }
    assert_eq!(uploads(&instance), Vec::<String>::new());
    assert_eq!(instance.stopped_cause(&id), None);
}

#[tokio::test]
async fn the_session_object_carries_the_upload_limits_the_route_enforces_rfc8620_2() {
    let (instance, id) = instance().await;
    let cookie = instance.sign_in().await;
    let (status, session) = instance.session(&cookie, &id).await;
    assert_eq!(status, StatusCode::OK);
    let core = &session["capabilities"]["urn:ietf:params:jmap:core"];
    // The fixture advertises 50,000,000 octets, under the route's bound,
    // and four uploads at once, past its lanes.
    const { assert!(50_000_000 < UPLOAD_LIMIT) };
    assert_eq!(core["maxSizeUpload"], 50_000_000);
    assert_eq!(core["maxConcurrentUpload"], MAX_CONCURRENT_UPLOADS);
    assert_eq!(
        session["uploadUrl"],
        format!("/api/jmap/{id}/upload/{{accountId}}")
    );
}

#[tokio::test]
async fn a_third_upload_waits_for_a_lane_while_two_are_held_and_a_request_still_runs() {
    let (instance, id) = instance().await;
    let instance = Arc::new(instance);
    let cookie = instance.sign_in().await;
    let port = instance.upstream.port();
    instance.upstream.set(Script {
        upload: Upload {
            hold: true,
            ..Upload::taken()
        },
        ..Script::echo(port)
    });
    let mut running = JoinSet::new();
    for _ in 0..=MAX_CONCURRENT_UPLOADS {
        let (instance, cookie, id) = (Arc::clone(&instance), cookie.clone(), id.clone());
        running.spawn(async move { sent(&instance, post(&cookie, &id, None, file())).await });
    }
    let mut waited = 0;
    while uploads(&instance).len() < MAX_CONCURRENT_UPLOADS && waited < PATIENCE {
        tokio::time::sleep(POLL).await;
        waited += 1;
    }
    tokio::time::sleep(POLL * 5).await;
    assert_eq!(uploads(&instance).len(), MAX_CONCURRENT_UPLOADS);
    let (status, answer) = instance.request(&cookie, &id, &query_request()).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    instance.upstream.set(Script::echo(port));
    while let Some(finished) = running.join_next().await {
        let (status, answer) = finished.unwrap();
        assert_eq!(status, StatusCode::OK, "{answer}");
    }
    assert_eq!(uploads(&instance).len(), MAX_CONCURRENT_UPLOADS + 1);
}
