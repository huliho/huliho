// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

mod blocks;

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use super::*;
use crate::blob::{Blob, BlobError};
use crate::testing::seal::TestSealer;

/// The chunk a test body arrives in: no divisor of a block.
const CHUNK: usize = 100_003;

/// What a test upload declares.
const MEDIA_TYPE: &str = "application/pdf";

/// A day and a bit: past the TTL of every store here.
const PAST: Duration = Duration::from_hours(25);

fn key(name: &str) -> AccountKey {
    AccountKey::new(name)
}

fn store_with(root: &Path, bounds: Bounds, sealer: TestSealer) -> Arc<Uploads> {
    Arc::new(Uploads::with_bounds(
        root.to_owned(),
        Arc::new(sealer),
        bounds,
    ))
}

fn store(root: &Path, bounds: Bounds) -> Arc<Uploads> {
    store_with(root, bounds, TestSealer::default())
}

/// Bytes that differ from block to block, so a block out of place shows.
fn content(size: usize) -> Vec<u8> {
    (0..size)
        .map(|index| u8::try_from(index % 251).unwrap())
        .collect()
}

async fn put(store: &Arc<Uploads>, owner: &AccountKey, bytes: &[u8]) -> Upload {
    let mut writing = store.begin(owner, MEDIA_TYPE).await.unwrap();
    for chunk in bytes.chunks(CHUNK) {
        writing.push(chunk).await.unwrap();
    }
    writing.finish().await.unwrap()
}

async fn drained(mut blob: Blob) -> Result<Vec<u8>, BlobError> {
    let mut bytes = Vec::new();
    while let Some(step) = blob.next().await {
        bytes.extend_from_slice(&step?);
    }
    Ok(bytes)
}

fn names(dir: &Path) -> BTreeSet<String> {
    fs::read_dir(dir)
        .map(|entries| {
            entries
                .map(|entry| entry.unwrap().file_name().into_string().unwrap())
                .collect()
        })
        .unwrap_or_default()
}

/// Moves an upload's arrival to `created_at`.
fn arrived(root: &Path, owner: &AccountKey, id: &UploadId, created_at: u64) {
    let path = Files::of(&root.join(owner.as_str()), id).sidecar;
    let mut sidecar: Sidecar = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    sidecar.created_at = created_at;
    fs::write(&path, serde_json::to_vec(&sidecar).unwrap()).unwrap();
}

fn now_seconds() -> u64 {
    epoch_seconds(SystemTime::now())
}

#[tokio::test]
async fn an_upload_reads_back_whole_and_as_a_stream_at_every_block_edge() {
    let root = tempfile::tempdir().unwrap();
    let uploads = store(root.path(), BOUNDS);
    let owner = key("a1");
    for size in [
        0,
        1,
        UPLOAD_BLOCK_BYTES - 1,
        UPLOAD_BLOCK_BYTES,
        UPLOAD_BLOCK_BYTES + 1,
        2 * UPLOAD_BLOCK_BYTES + 7,
    ] {
        let bytes = content(size);
        let upload = put(&uploads, &owner, &bytes).await;
        assert_eq!(upload.size, size as u64, "{size}");
        assert_eq!(upload.media_type, MEDIA_TYPE);
        let read = uploads.read(&owner, &upload.id, u64::MAX).await.unwrap();
        assert_eq!(read, bytes, "{size}");
        let opened = uploads.open(&owner, &upload.id, u64::MAX).await.unwrap();
        assert_eq!(drained(opened).await.unwrap(), bytes, "{size}");
        let files = Files::of(&root.path().join("a1"), &upload.id);
        let sidecar: Sidecar = serde_json::from_slice(&fs::read(files.sidecar).unwrap()).unwrap();
        assert_eq!(
            (sidecar.media_type.as_str(), sidecar.size),
            (MEDIA_TYPE, size as u64)
        );
        assert!(!files.part.exists());
    }
}

#[tokio::test]
async fn an_upload_answers_its_type_as_declared_and_its_sidecar_names_the_bare_type_rfc8620_6_1() {
    let root = tempfile::tempdir().unwrap();
    let uploads = store(root.path(), BOUNDS);
    let owner = key("a1");
    let name = "report.pdf";
    let declared = format!("{MEDIA_TYPE} ; name=\"{name}\"");
    let mut writing = uploads.begin(&owner, &declared).await.unwrap();
    writing.push(b"%PDF-1.7").await.unwrap();
    let upload = writing.finish().await.unwrap();
    assert_eq!(upload.media_type, declared);
    let dir = root.path().join("a1");
    let files = Files::of(&dir, &upload.id);
    let sidecar: Sidecar = serde_json::from_slice(&fs::read(files.sidecar).unwrap()).unwrap();
    assert_eq!(sidecar.media_type, MEDIA_TYPE);
    for file in names(&dir) {
        let bytes = fs::read(dir.join(&file)).unwrap();
        let holds = bytes
            .windows(name.len())
            .any(|window| window == name.as_bytes());
        assert!(!holds, "{file}");
    }
}

#[tokio::test]
async fn an_upload_past_its_bound_is_refused_and_leaves_nothing() {
    let root = tempfile::tempdir().unwrap();
    let bounds = Bounds {
        bytes: 10,
        ..BOUNDS
    };
    let uploads = store(root.path(), bounds);
    let owner = key("a1");
    let mut writing = uploads.begin(&owner, MEDIA_TYPE).await.unwrap();
    writing.push(&[1; 10]).await.unwrap();
    assert!(matches!(
        writing.push(&[1]).await,
        Err(UploadError::TooLarge)
    ));
    drop(writing);
    assert_eq!(names(&root.path().join("a1")), BTreeSet::new());
    let read = uploads.read(&owner, &UploadId::generate(), u64::MAX).await;
    assert!(matches!(read, Err(BlobError::NotFound)));
    let small = put(&uploads, &owner, &[1; 10]).await;
    let refused = uploads.read(&owner, &small.id, 9).await;
    assert!(matches!(refused, Err(BlobError::TooLarge)));
}

#[tokio::test]
async fn past_the_quota_the_oldest_uploads_go_first_rfc8620_6() {
    let root = tempfile::tempdir().unwrap();
    let probe = store(root.path(), BOUNDS);
    let owner = key("a1");
    let one = put(&probe, &owner, &[7; 100]).await;
    let on_disk = fs::metadata(root.path().join("a1").join(one.id.as_str()))
        .unwrap()
        .len();
    probe.forget(&owner).await;
    // Room for two uploads, not for three.
    let bounds = Bounds {
        quota: 2 * on_disk + on_disk / 2,
        ..BOUNDS
    };
    let uploads = store(root.path(), bounds);
    let older = put(&uploads, &owner, &[7; 100]).await;
    let newer = put(&uploads, &owner, &[8; 100]).await;
    arrived(root.path(), &owner, &older.id, now_seconds() - 20);
    arrived(root.path(), &owner, &newer.id, now_seconds() - 10);
    let third = put(&uploads, &owner, &[9; 100]).await;
    let gone = uploads.read(&owner, &older.id, u64::MAX).await;
    assert!(matches!(gone, Err(BlobError::NotFound)));
    for kept in [&newer, &third] {
        assert!(uploads.read(&owner, &kept.id, u64::MAX).await.is_ok());
    }
    // An upload without its sidecar opens for no one and goes first.
    fs::remove_file(Files::of(&root.path().join("a1"), &third.id).sidecar).unwrap();
    put(&uploads, &owner, &[6; 100]).await;
    assert!(uploads.read(&owner, &newer.id, u64::MAX).await.is_ok());
    assert!(!root.path().join("a1").join(third.id.as_str()).exists());
}

#[tokio::test]
async fn past_the_count_the_oldest_upload_goes_first_rfc8620_6() {
    let root = tempfile::tempdir().unwrap();
    let uploads = store(root.path(), Bounds { count: 2, ..BOUNDS });
    let owner = key("a1");
    let first = put(&uploads, &owner, b"first").await;
    let second = put(&uploads, &owner, b"second").await;
    arrived(root.path(), &owner, &first.id, now_seconds() - 20);
    arrived(root.path(), &owner, &second.id, now_seconds() - 10);
    let third = put(&uploads, &owner, b"third").await;
    let left: Vec<bool> = [&first, &second, &third]
        .into_iter()
        .map(|upload| root.path().join("a1").join(upload.id.as_str()).exists())
        .collect();
    assert_eq!(left, [false, true, true]);
}

#[tokio::test]
async fn with_nothing_left_to_evict_the_store_answers_over_quota_and_keeps_nothing() {
    let root = tempfile::tempdir().unwrap();
    let uploads = store(root.path(), Bounds { count: 1, ..BOUNDS });
    let owner = key("a1");
    let held = uploads.begin(&owner, MEDIA_TYPE).await.unwrap();
    assert!(matches!(
        uploads.begin(&owner, MEDIA_TYPE).await,
        Err(UploadError::OverQuota)
    ));
    held.abandon().await;
    let tight = store(
        root.path(),
        Bounds {
            quota: UPLOAD_BLOCK_BYTES as u64 + 1024,
            ..BOUNDS
        },
    );
    let mut first = tight.begin(&owner, MEDIA_TYPE).await.unwrap();
    let mut second = tight.begin(&owner, MEDIA_TYPE).await.unwrap();
    first.push(&content(UPLOAD_BLOCK_BYTES + 1)).await.unwrap();
    let refused = second.push(&content(UPLOAD_BLOCK_BYTES + 1)).await;
    assert!(matches!(refused, Err(UploadError::OverQuota)));
    let second_part = Files::of(&root.path().join("a1"), second.id()).part;
    assert!(!second_part.exists());
    let first = first.finish().await.unwrap();
    assert_eq!(
        names(&root.path().join("a1")).len(),
        2,
        "the bytes and the sidecar"
    );
    assert!(tight.read(&owner, &first.id, u64::MAX).await.is_ok());
}

#[tokio::test]
async fn a_dropped_upload_leaves_no_part_file() {
    let root = tempfile::tempdir().unwrap();
    let uploads = store(root.path(), BOUNDS);
    let owner = key("a1");
    let mut writing = uploads.begin(&owner, MEDIA_TYPE).await.unwrap();
    writing.push(&content(CHUNK)).await.unwrap();
    let part = Files::of(&root.path().join("a1"), writing.id()).part;
    assert!(part.exists());
    drop(writing);
    for _ in 0..1000 {
        if !part.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    assert!(!part.exists());
}

#[tokio::test]
async fn a_key_that_names_no_plain_directory_reaches_no_file() {
    let root = tempfile::tempdir().unwrap();
    let uploads = store(&root.path().join("uploads"), BOUNDS);
    for name in [
        "",
        ".",
        "..",
        "../a1",
        "a1/b",
        "a 1",
        "ä1",
        &"a".repeat(256),
    ] {
        let owner = key(name);
        assert!(
            matches!(
                uploads.begin(&owner, MEDIA_TYPE).await,
                Err(UploadError::Unnamed)
            ),
            "{name:?}"
        );
        let read = uploads.read(&owner, &UploadId::generate(), u64::MAX).await;
        assert!(matches!(read, Err(BlobError::NotFound)), "{name:?}");
    }
    assert_eq!(names(root.path()), BTreeSet::new());
}

#[tokio::test]
async fn the_sweep_removes_what_lived_past_its_time_and_keeps_the_rest() {
    let root = tempfile::tempdir().unwrap();
    let uploads = store(root.path(), BOUNDS);
    let owner = key("a1");
    let dir = root.path().join("a1");
    let old = put(&uploads, &owner, b"old").await;
    let young = put(&uploads, &owner, b"young").await;
    arrived(root.path(), &owner, &old.id, now_seconds() - PAST.as_secs());
    let read = uploads.read(&owner, &old.id, u64::MAX).await;
    assert!(matches!(read, Err(BlobError::NotFound)));
    // What a crash leaves: a part file, bytes without a sidecar and a
    // sidecar without bytes, each as old as the TTL and more.
    let stray = |name: String| {
        let file = File::create(dir.join(name)).unwrap();
        file.set_modified(SystemTime::now() - PAST).unwrap();
    };
    stray(format!("{}{PART_SUFFIX}", UploadId::generate()));
    stray(UploadId::generate().to_string());
    stray(format!("{}{SIDECAR_SUFFIX}", UploadId::generate()));
    stray("not-an-upload".to_owned());
    uploads.sweep().await;
    let young_files = BTreeSet::from([
        young.id.to_string(),
        format!("{}{SIDECAR_SUFFIX}", young.id),
        "not-an-upload".to_owned(),
    ]);
    assert_eq!(names(&dir), young_files);
    fs::remove_file(dir.join("not-an-upload")).unwrap();
    uploads.sweep_at(SystemTime::now() + PAST).await;
    assert!(!dir.exists());
}

/// The interval of the periodic sweep in the test; real time, so the
/// test waits for it.
const SWEEP_EVERY: Duration = Duration::from_millis(50);

/// How often and how long the periodic test looks for the sweep.
const LOOK: Duration = Duration::from_millis(10);
const LOOKS: u32 = 500;

/// Whether the file is gone within the looks.
async fn swept(path: &Path) -> bool {
    for _ in 0..LOOKS {
        if !path.exists() {
            return true;
        }
        tokio::time::sleep(LOOK).await;
    }
    false
}

#[tokio::test]
async fn the_sweep_runs_at_once_and_again_at_every_interval() {
    let root = tempfile::tempdir().unwrap();
    let uploads = store(root.path(), BOUNDS);
    let owner = key("a1");
    let sidecar = |upload: &Upload| Files::of(&root.path().join("a1"), &upload.id).sidecar;
    let first = put(&uploads, &owner, b"first").await;
    let stays = put(&uploads, &owner, b"stays").await;
    arrived(
        root.path(),
        &owner,
        &first.id,
        now_seconds() - PAST.as_secs(),
    );
    let task = tokio::spawn(Arc::clone(&uploads).sweep_every(SWEEP_EVERY));
    assert!(swept(&sidecar(&first)).await);
    let second = put(&uploads, &owner, b"second").await;
    arrived(
        root.path(),
        &owner,
        &second.id,
        now_seconds() - PAST.as_secs(),
    );
    assert!(swept(&sidecar(&second)).await);
    assert!(sidecar(&stays).exists());
    task.abort();
}

#[tokio::test]
async fn forgetting_an_account_removes_its_uploads_and_no_others() {
    let root = tempfile::tempdir().unwrap();
    let uploads = store(root.path(), BOUNDS);
    let (gone, kept) = (key("a1"), key("a2"));
    let left = put(&uploads, &gone, b"one").await;
    let stays = put(&uploads, &kept, b"two").await;
    uploads.forget(&gone).await;
    assert!(!root.path().join("a1").exists());
    let read = uploads.read(&gone, &left.id, u64::MAX).await;
    assert!(matches!(read, Err(BlobError::NotFound)));
    assert!(uploads.read(&kept, &stays.id, u64::MAX).await.is_ok());
}
