// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The download route of an IMAP account: who reaches a blob, what the
//! URL of the session object serves for a part and for the whole
//! message, which ids find nothing, what is refused before a window is
//! read and what a server that fails costs.

mod answers;
mod blob_answers;
mod bridge_rig;
mod common;
mod fake_dns;
mod log_capture;
mod readers;
mod signin;

use std::collections::HashMap;

use axum::http::{StatusCode, header};
use blob_answers::{assert_blob_headers, download};
use bridge_rig::{ADDRESS, Instance, mailboxes};
use huliho_imap_bridge::session::BODY_WINDOW_BYTES;
use huliho_imap_bridge::store::EmailId;
use huliho_imap_bridge::testing::imap::HOST;
use huliho_imap_bridge::testing::parts::{base64_lines, corpus, varied};
use huliho_imap_bridge::testing::{Behavior, Extension, Folder, Mailboxes, Message, PASSWORD};
use huliho_server::accounts::{self, StopCause};
use huliho_server::events::Actor;
use huliho_server::gate::RUN_WINDOW;
use huliho_server::mail::download::BLOB_DOWNLOAD_LIMIT;
use log_capture::Capture;
use serde_json::json;

/// The UIDs of the model, by what each message is for.
const CORPUS: u32 = 1;
const PLAIN: u32 = 2;
const BINARY: u32 = 3;
const HUGE: u32 = 4;

/// The first bytes of every PNG, which the corpus image consists of.
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

/// The name of the corpus PDF as a browser encodes it into a path.
const PDF_NAME_ENCODED: &str = "rapport%20caf%C3%A9.pdf";

/// An email id no account holds.
fn unknown_email() -> String {
    EmailId::generate().to_string()
}

/// The content of the binary part, past two windows once encoded.
fn binary_content() -> Vec<u8> {
    varied(usize::try_from(BODY_WINDOW_BYTES).unwrap() * 3 / 2)
}

/// The corpus, a plain message, a message of one base64 part past two
/// windows and one whose row states more than the route carries.
fn model(behavior: Behavior) -> Mailboxes {
    let body = base64_lines(&binary_content());
    let binary = Message {
        structure: format!(
            "(\"APPLICATION\" \"OCTET-STREAM\" NIL NIL NIL \"BASE64\" {})",
            body.len()
        ),
        body,
        ..Message::new(BINARY)
    };
    let huge = Message {
        size: u32::try_from(BLOB_DOWNLOAD_LIMIT + 1).unwrap(),
        ..Message::new(HUGE)
    };
    let mail = vec![corpus(CORPUS), Message::new(PLAIN), binary, huge];
    let mut model = Mailboxes::new(vec![Folder::new("INBOX").with_mail(mail)], Extension::all());
    model.behavior = behavior;
    model
}

/// The email id of every message of the inbox, by its UID.
async fn emails(instance: &Instance, cookie: &str, id: &str) -> HashMap<u32, String> {
    let (_, listed) = instance.call(cookie, id, mailboxes(id)).await;
    let inbox = listed["list"][0]["id"].clone();
    let query = json!(["Email/query", { "accountId": id, "filter": { "inMailbox": inbox } }, "c1"]);
    let (_, window) = instance.call(cookie, id, query).await;
    let get = json!(["Email/get", { "accountId": id, "ids": window["ids"], "properties": ["subject"] }, "c1"]);
    let (_, answer) = instance.call(cookie, id, get).await;
    let named = |email: &serde_json::Value| {
        let subject = email["subject"].as_str()?;
        let uid = subject.strip_prefix("Message ")?.parse().ok()?;
        Some((uid, email["id"].as_str()?.to_owned()))
    };
    answer["list"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(named)
        .collect()
}

/// A signed-in instance over the model with its account synced: the
/// instance, the cookie, the account id and the email ids.
async fn synced(instance: Instance) -> (Instance, String, String, HashMap<u32, String>) {
    let id = instance.add_account(PASSWORD);
    let cookie = instance.sign_in().await;
    assert!(
        instance.wait_synced(&cookie, &id).await,
        "{:?}",
        instance.fake.lines()
    );
    let emails = emails(&instance, &cookie, &id).await;
    (instance, cookie, id, emails)
}

/// The path of the route for a blob of the account.
fn path(id: &str, blob_id: &str, name: &str) -> String {
    format!("/api/jmap/{id}/download/{id}/{blob_id}/{name}")
}

/// The window fetches the scripted server received.
fn windows(instance: &Instance) -> usize {
    let window = format!(".{BODY_WINDOW_BYTES}>)");
    instance
        .fake
        .lines()
        .iter()
        .filter(|line| line.ends_with(&window))
        .count()
}

#[tokio::test]
async fn another_users_download_is_not_found_and_a_stopped_account_answers_409_before_anything_connects()
 {
    let instance = Instance::start(model(Behavior::default()), RUN_WINDOW).await;
    let id = instance.add_account(PASSWORD);
    let uri = path(&id, &unknown_email(), "message.eml");
    let other = instance.sign_in_other().await;
    let (status, _, body) = download(&instance.router, &other, &uri, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body:?}");
    let cookie = instance.sign_in().await;
    accounts::stop(
        &instance.store,
        &instance.scope(Some(&id)),
        StopCause::Connection,
        &Actor::System,
    )
    .unwrap();
    let (status, _, body) = download(&instance.router, &cookie, &uri, None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let body = String::from_utf8(body.unwrap()).unwrap();
    assert!(body.contains("still_stopped"), "{body}");
    assert!(instance.fake.lines().is_empty());
    assert_eq!(instance.bridge_rows(&id), 0);
    assert_eq!(
        instance.account_events(),
        [
            (
                "account.linked".to_owned(),
                instance.user_id().as_str().to_owned()
            ),
            ("account.stopped".to_owned(), "system".to_owned()),
        ]
    );
}

#[tokio::test]
async fn the_url_the_session_object_names_serves_a_part_decoded_and_the_whole_message_raw() {
    let capture = Capture::install();
    let on_disk = Instance::on_disk(model(Behavior::default())).await;
    let (instance, cookie, id, emails) = synced(on_disk).await;
    let (_, session) = instance.session(&cookie, &id).await;
    let template = session["downloadUrl"].as_str().unwrap();
    let url = |blob_id: &str, name: &str, media_type: &str| {
        template
            .replace("{accountId}", &id)
            .replace("{blobId}", blob_id)
            .replace("{name}", name)
            .replace("{type}", media_type)
    };
    let message = &emails[&CORPUS];
    let get = json!(["Email/get", { "accountId": id, "ids": [message], "properties": ["blobId", "attachments"] }, "c1"]);
    let (_, answer) = instance.call(&cookie, &id, get).await;
    let email = &answer["list"][0];
    assert_eq!(email["blobId"], *message);
    let blob_of = |name: &str| {
        let attachments = email["attachments"].as_array().unwrap();
        let part = attachments
            .iter()
            .find(|part| part["name"] == name)
            .unwrap();
        part["blobId"].as_str().unwrap().to_owned()
    };
    // The image, asked as the type its bytes carry, renders inline.
    let uri = url(&blob_of("logo.png"), "logo.png", "image/png");
    let (status, headers, body) = download(&instance.router, &cookie, &uri, None).await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(headers[header::CONTENT_TYPE], "image/png");
    assert_eq!(
        headers[header::CONTENT_DISPOSITION],
        "inline; filename*=UTF-8''logo.png"
    );
    assert_blob_headers(&headers);
    assert!(headers.get(header::CONTENT_LENGTH).is_none());
    assert_eq!(body.unwrap(), PNG_SIGNATURE);
    // Any other part is a download under its name.
    let pdf = blob_of("rapport caf\u{e9}.pdf");
    let uri = url(&pdf, PDF_NAME_ENCODED, "application/pdf");
    let (status, headers, body) = download(&instance.router, &cookie, &uri, None).await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(headers[header::CONTENT_TYPE], "application/octet-stream");
    assert_eq!(
        headers[header::CONTENT_DISPOSITION],
        format!("attachment; filename*=UTF-8''{PDF_NAME_ENCODED}")
    );
    assert_eq!(body.unwrap(), b"%PDF-1");
    // A part of several windows arrives whole, its base64 undone.
    let uri = url(&format!("{}-1", emails[&BINARY]), "data.bin", "");
    let (status, _, body) = download(&instance.router, &cookie, &uri, None).await;
    assert_eq!(status, StatusCode::OK);
    let content = body.unwrap();
    assert!(content == binary_content(), "{}", content.len());
    // The message itself streams raw; a bridge blob declares no length,
    // so a range answers the whole of it.
    let uri = url(message, "message.eml", "message/rfc822");
    let (status, headers, body) =
        download(&instance.router, &cookie, &uri, Some("bytes=0-9")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "application/octet-stream");
    assert!(headers.get(header::CONTENT_RANGE).is_none());
    assert_blob_headers(&headers);
    assert_eq!(body.unwrap(), corpus(CORPUS).raw().into_bytes());
    assert_eq!(instance.stopped_cause(&id), None);
    let text = capture.text();
    for word in ["logo.png", "rapport", message.as_str(), HOST, ADDRESS] {
        assert!(!text.contains(word), "{word} in {text}");
    }
}

#[tokio::test]
async fn an_id_that_names_no_blob_is_not_found_and_a_blob_past_the_limit_is_refused_unread() {
    let instance = Instance::start(model(Behavior::default()), RUN_WINDOW).await;
    let (instance, cookie, id, emails) = synced(instance).await;
    let plain = &emails[&PLAIN];
    let elsewhere = format!("/api/jmap/{id}/download/other/{plain}/message.eml");
    let cases = [
        (
            path(&id, "nonsense", "x"),
            StatusCode::NOT_FOUND,
            "not_found",
        ),
        (
            path(&id, &unknown_email(), "x"),
            StatusCode::NOT_FOUND,
            "not_found",
        ),
        (
            path(&id, &format!("{plain}-7"), "x"),
            StatusCode::NOT_FOUND,
            "not_found",
        ),
        (elsewhere, StatusCode::NOT_FOUND, "not_found"),
        (
            path(&id, &emails[&HUGE], "message.eml"),
            StatusCode::PAYLOAD_TOO_LARGE,
            "too_large",
        ),
    ];
    for (uri, expected, code) in cases {
        let (status, _, body) = download(&instance.router, &cookie, &uri, None).await;
        assert_eq!(status, expected, "{uri}");
        let body = String::from_utf8(body.unwrap()).unwrap();
        assert!(body.contains(code), "{uri}: {body}");
    }
    assert_eq!(windows(&instance), 0, "{:?}", instance.fake.lines());
    assert_eq!(instance.stopped_cause(&id), None);
}

#[tokio::test]
async fn a_server_that_refuses_answers_502_and_a_window_it_misplaces_breaks_the_download_off() {
    // Every window answer names origin zero, so a blob reads its first
    // window as asked and no second one.
    let behavior = Behavior {
        misplaced_origin: Some(0),
        refuses_body_of: Some(PLAIN),
        ..Behavior::default()
    };
    let instance = Instance::start(model(behavior), RUN_WINDOW).await;
    let (instance, cookie, id, emails) = synced(instance).await;
    let sessions = instance.logins();
    let uri = path(&id, &emails[&PLAIN], "message.eml");
    let (status, _, body) = download(&instance.router, &cookie, &uri, None).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    let body = String::from_utf8(body.unwrap()).unwrap();
    assert!(body.contains("upstream_failed"), "{body}");
    // A NO is the message's alone: the conversation keeps its session.
    assert_eq!(instance.logins(), sessions);
    let uri = path(&id, &format!("{}-1", emails[&BINARY]), "data.bin");
    let (status, headers, body) = download(&instance.router, &cookie, &uri, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_blob_headers(&headers);
    assert!(body.unwrap_err().contains("could not be read"));
    // Neither failure is the account's: it keeps running.
    assert_eq!(instance.stopped_cause(&id), None);
}
