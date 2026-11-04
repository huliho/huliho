// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The sealed blocks of an upload as they lie in its file: each block
//! behind its sealed length in four octets, big-endian. A reader opens
//! them one at a time in order; a block that fails to open, a plaintext
//! of the wrong length or a byte after the last block ends the read.

use std::fs::File;
use std::io::{self, Read, Write};
use std::sync::Arc;

use super::UPLOAD_BLOCK_BYTES;
use crate::seal::{Block, Sealer};
use crate::store::{AccountKey, UploadId};

/// The octets that state a block's sealed length.
const LENGTH_BYTES: usize = 4;

/// What a host's seal may add to a block's plaintext: room for the nonce
/// and the tag of any cipher. A length past it names no block.
pub(super) const MAX_SEAL_OVERHEAD: usize = 4096;

/// The plaintext the store holds in one block, as a count of octets.
const BLOCK: u64 = UPLOAD_BLOCK_BYTES as u64;

/// An upload whose file does not open as the store wrote it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Broken;

impl From<io::Error> for Broken {
    fn from(_error: io::Error) -> Self {
        Self
    }
}

/// How many blocks an upload of `size` octets has: one at least, so an
/// empty upload still has a block that closes it.
pub(super) fn block_count(size: u64) -> u32 {
    u32::try_from(size.div_ceil(BLOCK).max(1)).unwrap_or(u32::MAX)
}

/// The plaintext of block `index` of an upload of `size` octets.
fn block_len(size: u64, index: u32) -> u64 {
    size.saturating_sub(u64::from(index) * BLOCK).min(BLOCK)
}

/// Appends one sealed block behind its length.
pub(super) fn append(file: &mut File, sealed: &[u8]) -> io::Result<()> {
    let length = u32::try_from(sealed.len()).map_err(io::Error::other)?;
    file.write_all(&length.to_be_bytes())?;
    file.write_all(sealed)
}

/// The octets one block takes in the file, its length included.
pub(super) fn record_bytes(sealed: &[u8]) -> u64 {
    u64::try_from(LENGTH_BYTES + sealed.len()).unwrap_or(u64::MAX)
}

/// One upload's blocks as they open, in order.
pub(super) struct Reader {
    pub file: File,
    pub key: AccountKey,
    pub id: UploadId,
    pub sealer: Arc<dyn Sealer>,
    pub size: u64,
    pub index: u32,
}

impl Reader {
    /// The next block's plaintext; `None` once the last block opened and
    /// the file ended right after it.
    pub(super) fn next(&mut self) -> Result<Option<Vec<u8>>, Broken> {
        let count = block_count(self.size);
        if self.index == count {
            return Ok(None);
        }
        let sealed = record(&mut self.file)?.ok_or(Broken)?;
        let last = self.index + 1 == count;
        let block = Block {
            upload: &self.id,
            index: self.index,
            last,
        };
        let plain = self
            .sealer
            .open_block(&self.key, block, &sealed)
            .ok_or(Broken)?;
        if u64::try_from(plain.len()).ok() != Some(block_len(self.size, self.index)) {
            return Err(Broken);
        }
        if last && record(&mut self.file)?.is_some() {
            return Err(Broken);
        }
        self.index += 1;
        Ok(Some(plain))
    }
}

/// One sealed block of the file; `None` where the file ends before its
/// length. A length no block of the store reaches is broken.
pub(super) fn record(file: &mut File) -> Result<Option<Vec<u8>>, Broken> {
    let mut length = [0; LENGTH_BYTES];
    let read = file.read(&mut length[..1])?;
    if read == 0 {
        return Ok(None);
    }
    file.read_exact(&mut length[1..])?;
    let length = usize::try_from(u32::from_be_bytes(length)).map_err(|_| Broken)?;
    if length > UPLOAD_BLOCK_BYTES + MAX_SEAL_OVERHEAD {
        return Err(Broken);
    }
    let mut sealed = vec![0; length];
    file.read_exact(&mut sealed)?;
    Ok(Some(sealed))
}
