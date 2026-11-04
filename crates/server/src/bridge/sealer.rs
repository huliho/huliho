// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The host's cryptography for the bridge: the personal fields of an
//! email row sealed under a key of the instance secret, bound to the
//! account and the email; the blocks of an upload under a key of their
//! own, bound to the account and the block's place; the keyed hash a
//! Message-ID is stored as.

use std::sync::Arc;

use hmac::Mac;
use huliho_imap_bridge::seal::{Block, SealError, Sealer};
use huliho_imap_bridge::store::{AccountKey, EmailId};

use crate::sealed;
use crate::secrets::Keys;

/// The bridge's sealer on the instance's keys.
pub struct HostSealer {
    keys: Arc<Keys>,
}

impl HostSealer {
    #[must_use]
    pub fn new(keys: Arc<Keys>) -> Self {
        Self { keys }
    }
}

/// The associated data of a blob: the account and the email it belongs
/// to, apart on a line break neither id holds.
fn binding(key: &AccountKey, id: &EmailId) -> Vec<u8> {
    format!("{}\n{id}", key.as_str()).into_bytes()
}

/// The associated data of a block: the account, the upload, the block's
/// index and whether it closes the upload, apart on line breaks none of
/// them holds.
fn place(key: &AccountKey, block: Block<'_>) -> Vec<u8> {
    let closing = if block.last { "last" } else { "more" };
    format!(
        "{}\n{}\n{}\n{closing}",
        key.as_str(),
        block.upload,
        block.index
    )
    .into_bytes()
}

impl Sealer for HostSealer {
    fn seal(&self, key: &AccountKey, id: &EmailId, plaintext: &[u8]) -> Result<Vec<u8>, SealError> {
        sealed::seal(self.keys.bridge_cache(), &binding(key, id), plaintext).map_err(|_| SealError)
    }

    fn open(&self, key: &AccountKey, id: &EmailId, sealed: &[u8]) -> Option<Vec<u8>> {
        sealed::open(self.keys.bridge_cache(), &binding(key, id), sealed)
    }

    fn seal_block(
        &self,
        key: &AccountKey,
        block: Block<'_>,
        plaintext: &[u8],
    ) -> Result<Vec<u8>, SealError> {
        sealed::seal(self.keys.uploads(), &place(key, block), plaintext).map_err(|_| SealError)
    }

    fn open_block(&self, key: &AccountKey, block: Block<'_>, sealed: &[u8]) -> Option<Vec<u8>> {
        sealed::open(self.keys.uploads(), &place(key, block), sealed)
    }

    fn keyed_hash(&self, message_id: &[u8]) -> Vec<u8> {
        self.keys
            .message_ids()
            .clone()
            .chain_update(message_id)
            .finalize()
            .into_bytes()
            .to_vec()
    }
}

#[cfg(test)]
mod tests {
    use huliho_imap_bridge::store::UploadId;

    use super::*;
    use crate::secrets::InstanceSecret;

    const SECRET: &[u8] = b"0123456789abcdef0123456789abcdef";
    const OTHER_SECRET: &[u8] = b"fedcba9876543210fedcba9876543210";
    const SUBJECT: &[u8] = b"{\"subject\":\"lunch on Friday\"}";
    const PICTURE: &[u8] = b"\x89PNG of the lunch table";

    fn sealer(secret: &[u8]) -> HostSealer {
        let keys = Keys::derive(&InstanceSecret::from_bytes(secret.to_vec()).unwrap());
        HostSealer::new(Arc::new(keys))
    }

    fn key(name: &str) -> AccountKey {
        AccountKey::new(name)
    }

    fn id(text: &str) -> EmailId {
        EmailId::from(text.to_owned())
    }

    #[test]
    fn a_blob_opens_on_its_own_row_and_nowhere_else() {
        let host = sealer(SECRET);
        let sealed = host.seal(&key("a1"), &id("e1"), SUBJECT).unwrap();
        assert_eq!(
            host.open(&key("a1"), &id("e1"), &sealed),
            Some(SUBJECT.to_vec())
        );
        assert_eq!(host.open(&key("a2"), &id("e1"), &sealed), None);
        assert_eq!(host.open(&key("a1"), &id("e2"), &sealed), None);
        assert_eq!(
            sealer(OTHER_SECRET).open(&key("a1"), &id("e1"), &sealed),
            None
        );
        assert!(
            !sealed
                .windows(b"lunch".len())
                .any(|window| window == b"lunch")
        );
    }

    #[test]
    fn a_block_opens_at_its_own_place_and_nowhere_else() {
        let host = sealer(SECRET);
        let (upload, other) = (UploadId::generate(), UploadId::generate());
        let at = |upload, index, last| Block {
            upload,
            index,
            last,
        };
        let sealed = host
            .seal_block(&key("a1"), at(&upload, 1, false), PICTURE)
            .unwrap();
        assert_eq!(
            host.open_block(&key("a1"), at(&upload, 1, false), &sealed),
            Some(PICTURE.to_vec())
        );
        for elsewhere in [
            at(&other, 1, false),
            at(&upload, 0, false),
            at(&upload, 1, true),
        ] {
            assert_eq!(host.open_block(&key("a1"), elsewhere, &sealed), None);
        }
        assert_eq!(
            host.open_block(&key("a2"), at(&upload, 1, false), &sealed),
            None
        );
        let foreign = sealer(OTHER_SECRET);
        assert_eq!(
            foreign.open_block(&key("a1"), at(&upload, 1, false), &sealed),
            None
        );
        // The uploads' key is not the header cache's: the same place under
        // that key opens nothing.
        let place = place(&key("a1"), at(&upload, 1, false));
        assert_eq!(
            sealed::open(host.keys.bridge_cache(), &place, &sealed),
            None
        );
        assert_eq!(
            sealed::open(host.keys.uploads(), &place, &sealed),
            Some(PICTURE.to_vec())
        );
        assert!(
            !sealed
                .windows(b"lunch".len())
                .any(|window| window == b"lunch")
        );
    }

    #[test]
    fn the_hash_is_keyed_and_stable() {
        let host = sealer(SECRET);
        let first = host.keyed_hash(b"<m1@example.test>");
        assert_eq!(first.len(), 32);
        assert_eq!(first, host.keyed_hash(b"<m1@example.test>"));
        assert_ne!(first, host.keyed_hash(b"<m2@example.test>"));
        assert_ne!(first, sealer(OTHER_SECRET).keyed_hash(b"<m1@example.test>"));
    }
}
