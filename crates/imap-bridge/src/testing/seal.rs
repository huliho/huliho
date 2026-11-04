// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A sealer without a cipher: the blob names its row in front of the
//! plain bytes, so a test reads what was stored and a blob still opens
//! on no other row. A block names its place the same way.

use crate::seal::{Block, SealError, Sealer};
use crate::store::{AccountKey, EmailId};

/// The sealer of the tests; `locked` makes every blob refuse to open.
#[derive(Debug, Clone, Copy, Default)]
pub struct TestSealer {
    pub locked: bool,
}

fn binding(key: &AccountKey, id: &EmailId) -> Vec<u8> {
    format!("{}\n{id}\n", key.as_str()).into_bytes()
}

fn place(key: &AccountKey, block: Block<'_>) -> Vec<u8> {
    let Block {
        upload,
        index,
        last,
    } = block;
    format!("{}\n{upload}\n{index}\n{last}\n", key.as_str()).into_bytes()
}

impl Sealer for TestSealer {
    fn seal(&self, key: &AccountKey, id: &EmailId, plaintext: &[u8]) -> Result<Vec<u8>, SealError> {
        Ok([binding(key, id).as_slice(), plaintext].concat())
    }

    fn open(&self, key: &AccountKey, id: &EmailId, sealed: &[u8]) -> Option<Vec<u8>> {
        if self.locked {
            return None;
        }
        sealed
            .strip_prefix(binding(key, id).as_slice())
            .map(<[u8]>::to_vec)
    }

    fn seal_block(
        &self,
        key: &AccountKey,
        block: Block<'_>,
        plaintext: &[u8],
    ) -> Result<Vec<u8>, SealError> {
        Ok([place(key, block).as_slice(), plaintext].concat())
    }

    fn open_block(&self, key: &AccountKey, block: Block<'_>, sealed: &[u8]) -> Option<Vec<u8>> {
        if self.locked {
            return None;
        }
        sealed
            .strip_prefix(place(key, block).as_slice())
            .map(<[u8]>::to_vec)
    }

    fn keyed_hash(&self, message_id: &[u8]) -> Vec<u8> {
        [b"hash of ".as_slice(), message_id].concat()
    }
}
