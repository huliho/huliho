// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The steps of a message that is opened, the same for every target: a
//! rich message arrives, its HTML body answers sanitized with its image
//! and its attachment listed, both download through the route and the
//! mark as read shows on the second connection as the flag.

use axum::http::{StatusCode, header};
use serde_json::{Value, json};

use super::corpus::{RICH_IMAGE_NAME, RICH_NOTES, RICH_NOTES_NAME, RICH_WORDS};
use super::{Editor, Live, Mail, listed, number_of, seed_rich, seen};
use crate::blob_answers::{assert_blob_headers, download};

/// The cap a client asks a body value under.
const BODY_VALUE_BYTES: u64 = 4 * 1024 * 1024;

/// The first bytes of every PNG (the PNG specification, section 5.2).
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

/// The rich message with that number arrives, is opened, has both of
/// its attachments downloaded and is marked read.
pub async fn opened(live: &Live, mail: &Mail, editor: &mut Editor, number: u32) {
    let id = arrived(live, mail, editor, number).await;
    let email = body(live, mail, &id).await;
    downloads(live, mail, &email).await;
    marked_read(live, mail, editor, (&id, number)).await;
}

/// The id of the rich message once the account shows it.
async fn arrived(live: &Live, mail: &Mail, editor: &mut Editor, number: u32) -> String {
    let before = live.state(mail).await;
    seed_rich(editor, number).await;
    let changed = live
        .changes_showing(mail, &before, |answer| {
            !listed(answer, "created").is_empty()
        })
        .await;
    let created = listed(&changed, "created");
    let found = live.emails(mail, &created, &["id", "messageId"]).await;
    let ours = found.iter().find(|email| number_of(email) == Some(number));
    let ours = ours.unwrap_or_else(|| panic!("{changed}"));
    ours["id"].as_str().unwrap().to_owned()
}

/// The body request a client sends: the HTML value arrives without its
/// script and with its `cid:` image, the image and the attachment are
/// listed.
async fn body(live: &Live, mail: &Mail, id: &str) -> Value {
    let arguments = json!({
        "accountId": mail.account,
        "ids": [id],
        "properties": ["keywords", "textBody", "htmlBody", "attachments", "bodyValues"],
        "fetchTextBodyValues": true,
        "fetchHTMLBodyValues": true,
        "maxBodyValueBytes": BODY_VALUE_BYTES,
    });
    let mut got = live.call(mail, "Email/get", arguments).await;
    let email = got["list"][0].take();
    assert_eq!(email["keywords"].get("$seen"), None, "{email}");
    let part = email["htmlBody"][0]["partId"].as_str().unwrap();
    let html = email["bodyValues"][part]["value"].as_str().unwrap();
    assert!(html.contains(RICH_WORDS), "{html}");
    assert!(html.contains("cid:"), "{html}");
    assert!(!html.contains("script"), "{html}");
    email
}

/// The part of the email's attachments under that name.
fn attachment<'a>(email: &'a Value, name: &str) -> &'a Value {
    let attachments = email["attachments"].as_array().unwrap();
    let found = attachments.iter().find(|part| part["name"] == name);
    found.unwrap_or_else(|| panic!("no attachment named {name}: {email}"))
}

/// The image downloads inline as the PNG it is; the text attachment as
/// bytes to save, equal to what was sent.
async fn downloads(live: &Live, mail: &Mail, email: &Value) {
    let fetch = async |name: &str, kind: &str| {
        let blob = attachment(email, name)["blobId"].as_str().unwrap();
        let uri = format!(
            "/api/jmap/{}/download/{}/{blob}/{name}?type={kind}",
            mail.id, mail.account
        );
        let (status, headers, bytes) = download(&live.router, &live.cookie, &uri, None).await;
        assert_eq!(status, StatusCode::OK, "{name}");
        assert_blob_headers(&headers);
        let kind = headers[header::CONTENT_TYPE].to_str().unwrap().to_owned();
        (kind, bytes.unwrap())
    };
    let (kind, image) = fetch(RICH_IMAGE_NAME, "image/png").await;
    assert_eq!(kind, "image/png");
    assert!(image.starts_with(PNG_SIGNATURE), "{} bytes", image.len());
    let (kind, notes) = fetch(RICH_NOTES_NAME, "text/plain").await;
    assert_eq!(kind, "application/octet-stream");
    assert_eq!(String::from_utf8_lossy(&notes).trim_end(), RICH_NOTES);
}

/// `Email/set` with `$seen`: the email reads as updated and seen, and
/// the second connection finds the flag on the server.
async fn marked_read(live: &Live, mail: &Mail, editor: &mut Editor, (id, number): (&str, u32)) {
    assert!(!seen(editor, number).await, "unread before the mark");
    let arguments = json!({
        "accountId": mail.account,
        "update": { id: { "keywords/$seen": true } },
    });
    let stored = live.call(mail, "Email/set", arguments).await;
    assert_eq!(stored["updated"], json!({ id: null }), "{stored}");
    let read = live.emails(mail, &[id.to_owned()], &["keywords"]).await;
    assert_eq!(read[0]["keywords"]["$seen"], true, "{read:?}");
    assert!(seen(editor, number).await, "the flag stands on the server");
}
