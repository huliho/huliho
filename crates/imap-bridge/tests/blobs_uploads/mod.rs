// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! An upload id as a blob: it streams from the store block by block with
//! the server never asked; another account's upload is not found and one
//! past the caller's limit is refused before a block opens (RFC 8620
//! section 6.2).

use std::sync::Arc;

use huliho_imap_bridge::blob::{BlobError, open};
use huliho_imap_bridge::store::AccountKey;
use huliho_imap_bridge::testing::seal::TestSealer;
use huliho_imap_bridge::upload::{UPLOAD_BLOCK_BYTES, Uploads};

use crate::blobs_rig::{ROOMY, drained, started};
use crate::sync_rig::ACCOUNT;

/// Two blocks and a little: the stream hands on more than its first.
const PICTURE_BYTES: usize = 2 * UPLOAD_BLOCK_BYTES + 3;

#[tokio::test]
async fn an_upload_id_streams_from_the_store_with_the_server_never_asked_rfc8620_6_2() {
    let dir = tempfile::tempdir().unwrap();
    let mut blobs = started().await;
    let store = Arc::new(Uploads::new(
        dir.path().to_owned(),
        Arc::new(TestSealer::default()),
    ));
    blobs.rig.cache.uploads = Arc::clone(&store);
    let picture: Vec<u8> = (0..PICTURE_BYTES)
        .map(|index| u8::try_from(index % 253).unwrap())
        .collect();
    let mut ids = Vec::new();
    for owner in [ACCOUNT, "a2"] {
        let mut writing = store
            .begin(&AccountKey::new(owner), "image/png")
            .await
            .unwrap();
        writing.push(&picture).await.unwrap();
        ids.push(writing.finish().await.unwrap().id);
    }
    let before = blobs.rig.fake.lines().len();
    let own = open(&blobs.rig.cache, &blobs.link, ids[0].as_str(), ROOMY).await;
    assert_eq!(drained(own.unwrap()).await.unwrap(), picture);
    let foreign = open(&blobs.rig.cache, &blobs.link, ids[1].as_str(), ROOMY).await;
    assert!(matches!(foreign, Err(BlobError::NotFound)));
    let past = open(&blobs.rig.cache, &blobs.link, ids[0].as_str(), 1).await;
    assert!(matches!(past, Err(BlobError::TooLarge)));
    assert_eq!(blobs.rig.fake.lines().len(), before);
}
