// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::message::{Content, Part};
use crate::seal::{Block, SealError};

/// The bytes of the blob of the test and the parts that name it.
const BLOB_BYTES: usize = 10;
const LEAVES: usize = 3;

/// A host that counts the hashes a message asks for.
#[derive(Default)]
struct Counting(AtomicUsize);

impl Sealer for Counting {
    fn seal(&self, _: &AccountKey, _: &EmailId, plain: &[u8]) -> Result<Vec<u8>, SealError> {
        Ok(plain.to_vec())
    }

    fn open(&self, _: &AccountKey, _: &EmailId, sealed: &[u8]) -> Option<Vec<u8>> {
        Some(sealed.to_vec())
    }

    fn seal_block(&self, _: &AccountKey, _: Block<'_>, plain: &[u8]) -> Result<Vec<u8>, SealError> {
        Ok(plain.to_vec())
    }

    fn open_block(&self, _: &AccountKey, _: Block<'_>, sealed: &[u8]) -> Option<Vec<u8>> {
        Some(sealed.to_vec())
    }

    fn keyed_hash(&self, message_id: &[u8]) -> Vec<u8> {
        self.0.fetch_add(1, Ordering::Relaxed);
        message_id.to_vec()
    }
}

fn part(content: Content, media_type: &str) -> Part {
    Part {
        media_type: media_type.to_owned(),
        parameters: Vec::new(),
        name: None,
        disposition: None,
        cid: None,
        content,
    }
}

#[test]
fn a_message_answers_to_the_lower_of_the_two_bounds_rfc7889_3() {
    let most = u64::try_from(MAX_MESSAGE_BYTES).unwrap();
    assert_eq!(bound(None), most);
    assert_eq!(bound(Some(most - 1)), most - 1);
    assert_eq!(bound(Some(most + 1)), most);
}

#[test]
fn a_tree_past_the_limit_is_never_assembled_though_its_blob_is_read_once() {
    let leaf = part(Content::Blob("e1".to_owned()), "image/png");
    let draft = Draft {
        mailbox: "m1".to_owned(),
        keywords: Vec::new(),
        personal: Personal::default(),
        body: part(Content::Parts(vec![leaf; LEAVES]), "multipart/mixed"),
    };
    let blobs: Blobs = [("e1".to_owned(), vec![0; BLOB_BYTES])].into();
    let sealer = Counting::default();
    let weighed = u64::try_from(LEAVES * BLOB_BYTES).unwrap();
    let refused = assembled(&draft, &blobs, &sealer, weighed - 1);
    assert!(matches!(refused, Err(Failure::TooLarge)));
    assert_eq!(sealer.0.load(Ordering::Relaxed), 0);
    let most = u64::try_from(MAX_MESSAGE_BYTES).unwrap();
    assert!(assembled(&draft, &blobs, &sealer, most).is_ok());
    assert!(sealer.0.load(Ordering::Relaxed) > 0);
}

#[test]
fn a_copy_lands_by_the_message_id_it_keeps_under_an_id_of_its_own() {
    let personal = Personal {
        message_id: Some(vec!["d1@example.test".to_owned()]),
        references: Some(vec!["p1@example.test".to_owned()]),
        ..Personal::default()
    };
    let facts = CopyFacts {
        keywords: [("$seen".to_owned(), true)].into(),
        size: 42,
        date: 7,
    };
    let copy = Unsealed::copied(personal, true, facts, &Counting::default());
    assert_eq!(copy.hash, b"d1@example.test");
    assert_eq!(
        copy.named,
        [b"d1@example.test".to_vec(), b"p1@example.test".to_vec()]
    );
    assert!(copy.has_attachment);
    let none = CopyFacts {
        keywords: BTreeMap::new(),
        size: 0,
        date: 0,
    };
    let other = Unsealed::copied(Personal::default(), false, none, &Counting::default());
    assert_ne!(copy.id, other.id);
    assert_eq!(other.hash, Vec::<u8>::new());
}
