// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The host's cryptography behind one trait, so the bridge stores the
//! personal fields of an email and the bytes of an upload sealed without
//! carrying a cipher of its own.

use thiserror::Error;

use crate::store::{AccountKey, EmailId, UploadId};

/// Where one block of an upload belongs: the upload, its place in it and
/// whether it closes it. A block opens only there; a file cut short has
/// no closing block, so it opens not at all.
#[derive(Debug, Clone, Copy)]
pub struct Block<'a> {
    pub upload: &'a UploadId,
    pub index: u32,
    pub last: bool,
}

/// Seals and opens the personal fields of an email row and the blocks of
/// an upload; hashes a Message-ID for threading. The host binds a blob to
/// the account key and the email id. It binds a block to the account key
/// and its place. Either opens nowhere else.
pub trait Sealer: Send + Sync {
    /// Seals `plaintext` for the row named by `key` and `id`.
    ///
    /// # Errors
    ///
    /// Returns an error when the host's randomness or cipher fails.
    fn seal(&self, key: &AccountKey, id: &EmailId, plaintext: &[u8]) -> Result<Vec<u8>, SealError>;

    /// Opens a blob sealed for the same row; `None` for another row,
    /// another key or a changed byte.
    fn open(&self, key: &AccountKey, id: &EmailId, sealed: &[u8]) -> Option<Vec<u8>>;

    /// Seals one block of an upload of the account.
    ///
    /// # Errors
    ///
    /// Returns an error when the host's randomness or cipher fails.
    fn seal_block(
        &self,
        key: &AccountKey,
        block: Block<'_>,
        plaintext: &[u8],
    ) -> Result<Vec<u8>, SealError>;

    /// Opens a block sealed for the same account and place; `None`
    /// anywhere else or for a changed byte.
    fn open_block(&self, key: &AccountKey, block: Block<'_>, sealed: &[u8]) -> Option<Vec<u8>>;

    /// A keyed hash of a normalized Message-ID, the form the tables
    /// store so threading never keeps the id itself.
    fn keyed_hash(&self, message_id: &[u8]) -> Vec<u8>;
}

/// The host could not seal a value.
#[derive(Debug, Error)]
#[error("the host cannot seal a value")]
pub struct SealError;
