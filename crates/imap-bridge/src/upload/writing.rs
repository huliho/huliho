// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! One upload while its body arrives: the bytes counted against the
//! bound as they come, cut into blocks, each sealed off the runtime and
//! appended to its part file once the store made room for it. An upload
//! that fails or is dropped leaves nothing behind.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use super::store::blocking;
use super::{Sidecar, UPLOAD_BLOCK_BYTES, Upload, UploadError, Uploads, epoch_seconds};
use crate::seal::Block;
use crate::store::{AccountKey, UploadId};

/// An upload the store made room for: its id and the file it fills.
pub(super) struct Begun {
    pub id: UploadId,
    pub part: PathBuf,
}

/// An upload on its way into the store.
pub struct Writing {
    store: Arc<Uploads>,
    key: AccountKey,
    id: UploadId,
    media_type: String,
    part: PathBuf,
    /// The bytes of the block still filling.
    pending: Vec<u8>,
    index: u32,
    size: u64,
    /// Whether the part file still stands for this upload to remove.
    open: bool,
}

impl Writing {
    pub(super) fn new(
        store: Arc<Uploads>,
        key: &AccountKey,
        begun: Begun,
        media_type: &str,
    ) -> Self {
        Self {
            store,
            key: key.clone(),
            id: begun.id,
            media_type: media_type.to_owned(),
            part: begun.part,
            pending: Vec::new(),
            index: 0,
            size: 0,
            open: true,
        }
    }

    /// The id the upload goes by.
    #[must_use]
    pub fn id(&self) -> &UploadId {
        &self.id
    }

    /// The next bytes of the body. A block goes out once the next one
    /// starts, so the last block is known to be the last when it is
    /// sealed.
    ///
    /// # Errors
    ///
    /// Returns `TooLarge` past the bound, `OverQuota` where the store
    /// cannot make room and the store's failure; the upload is gone then.
    pub async fn push(&mut self, bytes: &[u8]) -> Result<(), UploadError> {
        let pushed = self.pushed(bytes).await;
        if pushed.is_err() {
            self.discard().await;
        }
        pushed
    }

    /// The last block written, the file renamed into place and its
    /// sidecar beside it: the upload as the store holds it.
    ///
    /// # Errors
    ///
    /// As [`Writing::push`].
    pub async fn finish(mut self) -> Result<Upload, UploadError> {
        let finished = self.finished().await;
        if finished.is_err() {
            self.discard().await;
        }
        finished
    }

    /// Drops the upload with its part file.
    pub async fn abandon(mut self) {
        self.discard().await;
    }

    async fn pushed(&mut self, bytes: &[u8]) -> Result<(), UploadError> {
        let more = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        self.size = self.size.saturating_add(more);
        if self.size > self.store.bounds.bytes {
            return Err(UploadError::TooLarge);
        }
        self.pending.extend_from_slice(bytes);
        while self.pending.len() > UPLOAD_BLOCK_BYTES {
            let rest = self.pending.split_off(UPLOAD_BLOCK_BYTES);
            let block = std::mem::replace(&mut self.pending, rest);
            self.write(block, false).await?;
        }
        Ok(())
    }

    async fn finished(&mut self) -> Result<Upload, UploadError> {
        let block = std::mem::take(&mut self.pending);
        self.write(block, true).await?;
        let sidecar = Sidecar {
            media_type: bare_type(&self.media_type).to_owned(),
            size: self.size,
            created_at: epoch_seconds(SystemTime::now()),
        };
        self.store.complete(&self.key, &self.id, sidecar).await?;
        self.open = false;
        // The answer's type is the request's (RFC 8620 section 6.1).
        Ok(Upload {
            id: self.id.clone(),
            media_type: self.media_type.clone(),
            size: self.size,
        })
    }

    /// One block sealed off the runtime, then appended once there is room.
    async fn write(&mut self, plain: Vec<u8>, last: bool) -> Result<(), UploadError> {
        let (host, key, id) = (
            Arc::clone(&self.store.sealer),
            self.key.clone(),
            self.id.clone(),
        );
        let index = self.index;
        let record = blocking(move || {
            let block = Block {
                upload: &id,
                index,
                last,
            };
            host.seal_block(&key, block, &plain)
                .map_err(|_| UploadError::Sealing)
        })
        .await?;
        self.store.append(&self.key, &self.part, record).await?;
        self.index = self.index.checked_add(1).ok_or(UploadError::TooLarge)?;
        Ok(())
    }

    async fn discard(&mut self) {
        if !std::mem::take(&mut self.open) {
            return;
        }
        let part = self.part.clone();
        let _gone = tokio::task::spawn_blocking(move || fs::remove_file(part)).await;
    }
}

/// The declared type before its first `;`, trimmed: a file name in a
/// `name` parameter stays out of the sidecar.
fn bare_type(declared: &str) -> &str {
    declared
        .split_once(';')
        .map_or(declared, |(bare, _parameters)| bare)
        .trim()
}

/// An upload dropped on its way, its caller gone with the body, leaves
/// no part file.
impl Drop for Writing {
    fn drop(&mut self) {
        if !self.open {
            return;
        }
        let part = std::mem::take(&mut self.part);
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => {
                runtime.spawn_blocking(move || fs::remove_file(part));
            }
            Err(_) => {
                let _gone = fs::remove_file(part);
            }
        }
    }
}
