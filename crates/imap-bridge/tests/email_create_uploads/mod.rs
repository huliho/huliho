// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A draft that names an upload: the create reads the upload from the
//! store and sends its bytes in the message, the server never asked for
//! it; another account's upload or none answers `blobNotFound`; an upload
//! past what the server takes answers `tooLarge` with nothing sent (RFC
//! 8620 section 6).

use std::sync::Arc;

use huliho_imap_bridge::store::{AccountKey, UploadId};
use huliho_imap_bridge::testing::Extension;
use huliho_imap_bridge::testing::seal::TestSealer;
use huliho_imap_bridge::upload::Uploads;
use mail_parser::{MessageParser, MimeHeaders};
use serde_json::{Value, json};
use tempfile::TempDir;

use crate::draft_rig::{
    DRAFTS, account, appends, create, created, drafts, held, object, refused, synced,
};
use crate::set_rig::Set;
use crate::sync_rig::ACCOUNT;

/// The bytes of the test's attachment: several base64 lines, one block.
const ATTACHMENT_BYTES: usize = 3000;

/// An APPENDLIMIT the attachment alone passes.
const BELOW_THE_ATTACHMENT: u64 = 2000;

/// An account that is not the rig's.
const OTHER_ACCOUNT: &str = "a2";

fn attachment() -> Vec<u8> {
    (0..ATTACHMENT_BYTES)
        .map(|index| u8::try_from(index % 256).unwrap())
        .collect()
}

/// A store of uploads in a directory of the test's own.
fn uploads(dir: &TempDir) -> Arc<Uploads> {
    Arc::new(Uploads::new(
        dir.path().to_owned(),
        Arc::new(TestSealer::default()),
    ))
}

/// The bytes uploaded for `owner`; the id the store gave them.
async fn uploaded(uploads: &Arc<Uploads>, owner: &str, bytes: &[u8]) -> UploadId {
    let owner = AccountKey::new(owner);
    let mut writing = uploads.begin(&owner, "application/pdf").await.unwrap();
    writing.push(bytes).await.unwrap();
    writing.finish().await.unwrap().id
}

/// The client's object with a text part and the blob as an attachment.
fn attaching(mailbox: &str, blob_id: &UploadId) -> Value {
    let mut object = object(mailbox);
    object["bodyStructure"] = json!({
        "type": "multipart/mixed",
        "subParts": [
            { "partId": "t", "type": "text/plain" },
            {
                "blobId": blob_id.as_str(),
                "type": "application/pdf",
                "name": "plan.pdf",
                "disposition": "attachment",
            },
        ],
    });
    object
}

/// The draft account with its uploads in `dir`, under the server's
/// APPENDLIMIT where one is given.
async fn drafting(dir: &TempDir, append_limit: Option<u64>) -> Set {
    let mut mailboxes = account(Extension::all());
    mailboxes.append_limit = append_limit;
    let mut set = synced(mailboxes).await;
    set.rig.cache.uploads = uploads(dir);
    set
}

#[tokio::test]
async fn a_draft_carries_the_bytes_of_an_upload_it_names_rfc8620_6_1() {
    let dir = tempfile::tempdir().unwrap();
    let set = drafting(&dir, None).await;
    let id = uploaded(&set.rig.cache.uploads, ACCOUNT, &attachment()).await;
    let before = set.rig.fake.lines().len();
    created(&create(&set, attaching(&drafts(&set), &id)).await);
    let raw = held(&set, DRAFTS)[0].raw();
    let parsed = MessageParser::default().parse(raw.as_bytes()).unwrap();
    let part = parsed.attachment(0).unwrap();
    assert_eq!(part.attachment_name(), Some("plan.pdf"));
    assert_eq!(part.contents(), attachment().as_slice());
    let lines = set.rig.fake.lines();
    let fetched = lines[before..]
        .iter()
        .filter(|line| line.contains(" FETCH "));
    assert_eq!(fetched.count(), 0, "{:?}", &lines[before..]);
}

#[tokio::test]
async fn an_upload_of_another_account_or_none_is_not_found_and_sends_nothing_rfc8620_6() {
    let dir = tempfile::tempdir().unwrap();
    let set = drafting(&dir, None).await;
    let foreign = uploaded(&set.rig.cache.uploads, OTHER_ACCOUNT, &attachment()).await;
    for id in [foreign, UploadId::generate()] {
        let answer = create(&set, attaching(&drafts(&set), &id)).await;
        assert_eq!(refused(&answer), "blobNotFound", "{answer}");
        assert_eq!(answer["notCreated"]["d"]["notFound"], json!([id.as_str()]));
    }
    assert_eq!(appends(&set), Vec::<String>::new());
}

#[tokio::test]
async fn an_upload_past_what_the_server_takes_sends_nothing_rfc7889_4() {
    let dir = tempfile::tempdir().unwrap();
    let set = drafting(&dir, Some(BELOW_THE_ATTACHMENT)).await;
    let id = uploaded(&set.rig.cache.uploads, ACCOUNT, &attachment()).await;
    let answer = create(&set, attaching(&drafts(&set), &id)).await;
    assert_eq!(refused(&answer), "tooLarge", "{answer}");
    assert_eq!(appends(&set), Vec::<String>::new());
}
