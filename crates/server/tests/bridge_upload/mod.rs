// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The upload route of an IMAP account (RFC 8620 section 6.1): a file
//! lands sealed in the bridge's store under the data volume and the
//! download route serves it back; the store's bound refuses a larger one
//! with nothing kept; another account on the path finds nothing; the
//! account's removal takes its uploads along.

use std::path::{Path, PathBuf};

use axum::body::{Body, Bytes};
use axum::http::{Method, Request, StatusCode, header};
use futures_util::stream;
use huliho_imap_bridge::testing::{Extension, Folder, Mailboxes, PASSWORD};
use huliho_imap_bridge::upload::{MAX_ATTACHMENTS_BYTES, UPLOAD_BLOCK_BYTES};
use huliho_server::gate::RUN_WINDOW;
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::answers::answer;
use crate::blob_answers::download;
use crate::bridge_rig::Instance;
use crate::signin::with_cookie;

/// A picture of a block and a half: two sealed blocks on disk.
const PICTURE_BYTES: usize = UPLOAD_BLOCK_BYTES + UPLOAD_BLOCK_BYTES / 2;

/// The first bytes of every PNG.
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

/// A run of the picture that shows where it stands in the clear.
const MARKER: &[u8] = b"the lunch table at noon";

fn picture() -> Vec<u8> {
    let mut bytes = PNG_SIGNATURE.to_vec();
    while bytes.len() < PICTURE_BYTES {
        bytes.extend_from_slice(MARKER);
    }
    bytes.truncate(PICTURE_BYTES);
    bytes
}

async fn instance() -> (Instance, String, String) {
    let mailboxes = Mailboxes::new(vec![Folder::new("INBOX")], Extension::all());
    let instance = Instance::start(mailboxes, RUN_WINDOW).await;
    let id = instance.add_account(PASSWORD);
    let cookie = instance.sign_in().await;
    (instance, id, cookie)
}

fn post(cookie: &str, uri: &str, body: Body) -> Request<Body> {
    let mut request = with_cookie(Method::POST, uri, cookie);
    request
        .headers_mut()
        .insert(header::CONTENT_TYPE, "image/png".parse().unwrap());
    *request.body_mut() = body;
    request
}

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

/// The files of the account's uploads.
fn files(uploads: &Path, id: &str) -> Vec<PathBuf> {
    std::fs::read_dir(uploads.join(id))
        .map(|entries| entries.map(|entry| entry.unwrap().path()).collect())
        .unwrap_or_default()
}

#[tokio::test]
async fn a_file_lands_sealed_and_the_download_route_serves_it_rfc8620_6_1() {
    let (instance, id, cookie) = instance().await;
    let uri = format!("/api/jmap/{id}/upload/{id}");
    let (status, answer) = sent(&instance, post(&cookie, &uri, Body::from(picture()))).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let blob_id = answer["blobId"].as_str().unwrap().to_owned();
    assert!(blob_id.starts_with('u'), "{blob_id}");
    let expected =
        json!({ "accountId": id, "blobId": blob_id, "type": "image/png", "size": PICTURE_BYTES });
    assert_eq!(answer, expected);
    let stored = std::fs::read(instance.api.uploads.join(&id).join(&blob_id)).unwrap();
    assert!(stored.len() > PICTURE_BYTES);
    assert!(!stored.windows(MARKER.len()).any(|window| window == MARKER));
    let sidecar = std::fs::read(
        instance
            .api
            .uploads
            .join(&id)
            .join(format!("{blob_id}.json")),
    );
    let sidecar: Value = serde_json::from_slice(&sidecar.unwrap()).unwrap();
    assert_eq!(sidecar["type"], "image/png");
    assert_eq!(sidecar["size"], PICTURE_BYTES);
    let back = format!("/api/jmap/{id}/download/{id}/{blob_id}/lunch.png?type=image/png");
    let (status, headers, body) = download(&instance.router, &cookie, &back, None).await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(headers[header::CONTENT_TYPE], "image/png");
    assert_eq!(body.unwrap(), picture());
}

#[tokio::test]
async fn past_the_store_bound_an_upload_is_refused_and_nothing_stays() {
    let (instance, id, cookie) = instance().await;
    let uri = format!("/api/jmap/{id}/upload/{id}");
    let mut declared = post(&cookie, &uri, Body::from(picture()));
    declared
        .headers_mut()
        .insert(header::CONTENT_LENGTH, (MAX_ATTACHMENTS_BYTES + 1).into());
    let (status, answer) = sent(&instance, declared).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(answer["error"], "too_large");
    assert_eq!(files(&instance.api.uploads, &id), Vec::<PathBuf>::new());
    // The same without a length: cut where the count passes the bound.
    let blocks = usize::try_from(MAX_ATTACHMENTS_BYTES).unwrap() / UPLOAD_BLOCK_BYTES;
    let chunks =
        (0..=blocks).map(|_| Ok::<_, std::io::Error>(Bytes::from(vec![7; UPLOAD_BLOCK_BYTES])));
    let streamed = post(&cookie, &uri, Body::from_stream(stream::iter(chunks)));
    let (status, answer) = sent(&instance, streamed).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(answer["error"], "too_large");
    assert_eq!(files(&instance.api.uploads, &id), Vec::<PathBuf>::new());
}

#[tokio::test]
async fn another_account_on_the_path_or_another_user_finds_nothing() {
    let (instance, id, cookie) = instance().await;
    let elsewhere = format!("/api/jmap/{id}/upload/another-account");
    let (status, _) = sent(&instance, post(&cookie, &elsewhere, Body::from(picture()))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let other = instance.sign_in_other().await;
    let uri = format!("/api/jmap/{id}/upload/{id}");
    let (status, _) = sent(&instance, post(&other, &uri, Body::from(picture()))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(!instance.api.uploads.join(&id).exists());
}

#[tokio::test]
async fn removing_the_account_takes_its_uploads_along() {
    let (instance, id, cookie) = instance().await;
    let uri = format!("/api/jmap/{id}/upload/{id}");
    let (status, stored) = sent(&instance, post(&cookie, &uri, Body::from(picture()))).await;
    assert_eq!(status, StatusCode::OK, "{stored}");
    assert_eq!(files(&instance.api.uploads, &id).len(), 2);
    let removal = with_cookie(Method::DELETE, &format!("/api/accounts/{id}"), &cookie);
    let (status, _) = answer(&instance.router, removal).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(!instance.api.uploads.join(&id).exists());
}
