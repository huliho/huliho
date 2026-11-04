// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! An upload's file changed on disk: a block of another upload, a file
//! cut short or grown, a host that opens nothing, a byte flipped anywhere.

use std::hash::{DefaultHasher, Hash, Hasher};

use proptest::prelude::*;

use super::*;
use crate::seal::{Block, SealError, Sealer};
use crate::store::EmailId;

#[tokio::test]
async fn a_block_opens_only_at_its_own_place_and_a_cut_file_not_at_all() {
    let root = tempfile::tempdir().unwrap();
    let uploads = store(root.path(), BOUNDS);
    let (owner, other) = (key("a1"), key("a2"));
    let bytes = content(2 * UPLOAD_BLOCK_BYTES + 1);
    let upload = put(&uploads, &owner, &bytes).await;
    let twin = put(&uploads, &owner, &bytes).await;
    let dir = root.path().join("a1");
    let file = |upload: &Upload| dir.join(upload.id.as_str());
    let whole = fs::read(file(&upload)).unwrap();
    let not_found = |read: Result<Vec<u8>, BlobError>| matches!(read, Err(BlobError::NotFound));
    // Another account's key opens nothing of it.
    assert!(not_found(uploads.read(&other, &upload.id, u64::MAX).await));
    // The twin's file under this upload's id: every block names its upload.
    fs::copy(file(&twin), file(&upload)).unwrap();
    assert!(not_found(uploads.read(&owner, &upload.id, u64::MAX).await));
    // The file cut before its last block, or with a byte after it.
    let first_record = 4 + "a1\n".len() + upload.id.as_str().len() + "\n0\nfalse\n".len();
    let cut = &whole[..first_record + UPLOAD_BLOCK_BYTES];
    fs::write(file(&upload), cut).unwrap();
    assert!(not_found(uploads.read(&owner, &upload.id, u64::MAX).await));
    fs::write(file(&upload), [whole.as_slice(), &[0]].concat()).unwrap();
    assert!(not_found(uploads.read(&owner, &upload.id, u64::MAX).await));
    let opened = uploads.open(&owner, &upload.id, u64::MAX).await.unwrap();
    assert!(matches!(drained(opened).await, Err(BlobError::NotFound)));
    // A host that opens no block.
    fs::write(file(&upload), &whole).unwrap();
    let locked = store_with(root.path(), BOUNDS, TestSealer { locked: true });
    assert!(not_found(locked.read(&owner, &upload.id, u64::MAX).await));
    assert_eq!(
        uploads.read(&owner, &upload.id, u64::MAX).await.unwrap(),
        bytes
    );
}

#[tokio::test]
async fn a_size_in_the_sidecar_other_than_the_blocks_hold_opens_nothing() {
    let root = tempfile::tempdir().unwrap();
    let uploads = store(root.path(), BOUNDS);
    let owner = key("a1");
    let bytes = content(3000);
    let upload = put(&uploads, &owner, &bytes).await;
    for size in [2999, 3001, u64::try_from(UPLOAD_BLOCK_BYTES).unwrap() + 1] {
        let path = Files::of(&root.path().join("a1"), &upload.id).sidecar;
        let mut sidecar: Sidecar = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        sidecar.size = size;
        fs::write(&path, serde_json::to_vec(&sidecar).unwrap()).unwrap();
        let read = uploads.read(&owner, &upload.id, u64::MAX).await;
        assert!(matches!(read, Err(BlobError::NotFound)), "{size}");
    }
}

#[test]
fn a_record_longer_than_any_block_is_never_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("record");
    let long = UPLOAD_BLOCK_BYTES + super::super::blocks::MAX_SEAL_OVERHEAD + 1;
    let mut written = u32::try_from(long).unwrap().to_be_bytes().to_vec();
    written.resize(4 + long, 7);
    fs::write(&path, &written).unwrap();
    let read = super::super::blocks::record(&mut File::open(&path).unwrap());
    assert!(read.is_err());
    let fits = long - 1;
    let mut written = u32::try_from(fits).unwrap().to_be_bytes().to_vec();
    written.resize(4 + fits, 7);
    fs::write(&path, &written).unwrap();
    let read = super::super::blocks::record(&mut File::open(&path).unwrap());
    assert_eq!(
        read.map(|sealed| sealed.map(|sealed| sealed.len())),
        Ok(Some(fits))
    );
}

/// A sealer whose block carries a hash of its place and its bytes, so a
/// changed byte opens nothing, as a cipher's tag would refuse it.
struct Tagged;

fn tag(key: &AccountKey, block: Block<'_>, plain: &[u8]) -> [u8; 8] {
    let mut hasher = DefaultHasher::new();
    (
        key.as_str(),
        block.upload.as_str(),
        block.index,
        block.last,
        plain,
    )
        .hash(&mut hasher);
    hasher.finish().to_be_bytes()
}

impl Sealer for Tagged {
    fn seal(&self, _: &AccountKey, _: &EmailId, plain: &[u8]) -> Result<Vec<u8>, SealError> {
        Ok(plain.to_vec())
    }

    fn open(&self, _: &AccountKey, _: &EmailId, sealed: &[u8]) -> Option<Vec<u8>> {
        Some(sealed.to_vec())
    }

    fn seal_block(
        &self,
        key: &AccountKey,
        block: Block<'_>,
        plain: &[u8],
    ) -> Result<Vec<u8>, SealError> {
        Ok([plain, &tag(key, block, plain)].concat())
    }

    fn open_block(&self, key: &AccountKey, block: Block<'_>, sealed: &[u8]) -> Option<Vec<u8>> {
        let (plain, mark) = sealed.split_at_checked(sealed.len().checked_sub(8)?)?;
        (mark == tag(key, block, plain)).then(|| plain.to_vec())
    }

    fn keyed_hash(&self, message_id: &[u8]) -> Vec<u8> {
        message_id.to_vec()
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    /// A file changed at any byte or cut at any length reads as the upload
    /// it was or not at all.
    #[test]
    fn a_changed_or_cut_file_never_reads_as_another_upload(
        size in 0usize..3000,
        at in any::<prop::sample::Index>(),
        flip in 1u8..,
        cut in any::<bool>(),
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let root = tempfile::tempdir().unwrap();
            let uploads = Arc::new(Uploads::new(root.path().to_owned(), Arc::new(Tagged)));
            let owner = key("a1");
            let bytes = content(size);
            let upload = put(&uploads, &owner, &bytes).await;
            let path = root.path().join("a1").join(upload.id.as_str());
            let mut file = fs::read(&path).unwrap();
            let index = at.index(file.len());
            if cut {
                file.truncate(index);
            } else {
                file[index] ^= flip;
            }
            fs::write(&path, &file).unwrap();
            let read = uploads.read(&owner, &upload.id, u64::MAX).await;
            let whole_or_nothing = match read {
                Ok(read) => read == bytes,
                Err(BlobError::NotFound) => true,
                Err(_) => false,
            };
            prop_assert!(whole_or_nothing);
            Ok(())
        })?;
    }
}
