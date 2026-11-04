// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The store: room made under one lock per account, the reads of an
//! upload for a download and for a draft, the sweep and the removal of an
//! account's uploads. Every file call runs off the runtime.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::SystemTime;

use tokio::sync::mpsc;

use super::blocks::{self, Broken, Reader, block_count};
use super::disk::{Listing, account_names, sidecar, sweep_dir};
use super::writing::Begun;
use super::{
    BOUNDS, Bounds, Files, Sidecar, UPLOAD_SWEEP_INTERVAL, UploadError, Writing, account_dir,
};
use crate::blob::{BLOB_BUFFER_WINDOWS, Blob, BlobError, Step};
use crate::seal::Sealer;
use crate::store::{AccountKey, UploadId};

/// One lock per account, taken for a turn of room, a finish and a sweep.
type Locks = HashMap<AccountKey, Arc<tokio::sync::Mutex<()>>>;

/// The uploads of every account under one root.
pub struct Uploads {
    root: PathBuf,
    pub(super) sealer: Arc<dyn Sealer>,
    pub(super) bounds: Bounds,
    locks: Mutex<Locks>,
}

impl Uploads {
    /// The store under `root`, made on the first upload, on the bounds
    /// every host runs.
    #[must_use]
    pub fn new(root: PathBuf, sealer: Arc<dyn Sealer>) -> Self {
        Self::with_bounds(root, sealer, BOUNDS)
    }

    /// The store on the given bounds.
    #[must_use]
    pub(crate) fn with_bounds(root: PathBuf, sealer: Arc<dyn Sealer>, bounds: Bounds) -> Self {
        Self {
            root,
            sealer,
            bounds,
            locks: Mutex::new(HashMap::new()),
        }
    }

    /// Starts an upload of the account: room made for one more, the
    /// oldest uploads evicted where it takes that, its file begun.
    ///
    /// # Errors
    ///
    /// Returns `OverQuota` where nothing is left to evict, `Unnamed` for
    /// a key that names no directory and the file system's failure.
    pub async fn begin(
        self: &Arc<Self>,
        key: &AccountKey,
        media_type: &str,
    ) -> Result<Writing, UploadError> {
        let dir = account_dir(&self.root, key).ok_or(UploadError::Unnamed)?;
        let id = UploadId::generate();
        let files = Files::of(&dir, &id);
        let bounds = self.bounds;
        let lock = self.lock(key);
        let _held = lock.lock().await;
        let part = files.part.clone();
        blocking(move || {
            fs::create_dir_all(&dir)?;
            Listing::of(&dir)?.make_room(1, 0, bounds)?;
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&part)?;
            Ok(())
        })
        .await?;
        let begun = Begun {
            id,
            part: files.part,
        };
        Ok(Writing::new(Arc::clone(self), key, begun, media_type))
    }

    /// Room for one sealed block of an upload in flight, then the block
    /// appended to its file.
    pub(super) async fn append(
        &self,
        key: &AccountKey,
        part: &Path,
        sealed: Vec<u8>,
    ) -> Result<(), UploadError> {
        let dir = part
            .parent()
            .map(Path::to_owned)
            .ok_or(UploadError::Unnamed)?;
        let (part, bounds) = (part.to_owned(), self.bounds);
        let lock = self.lock(key);
        let _held = lock.lock().await;
        blocking(move || {
            Listing::of(&dir)?.make_room(0, blocks::record_bytes(&sealed), bounds)?;
            let mut file = OpenOptions::new().append(true).open(&part)?;
            blocks::append(&mut file, &sealed)?;
            Ok(())
        })
        .await
    }

    /// The file renamed into place and its sidecar written beside it.
    pub(super) async fn complete(
        &self,
        key: &AccountKey,
        id: &UploadId,
        sidecar: Sidecar,
    ) -> Result<(), UploadError> {
        let dir = account_dir(&self.root, key).ok_or(UploadError::Unnamed)?;
        let files = Files::of(&dir, id);
        let lock = self.lock(key);
        let _held = lock.lock().await;
        blocking(move || {
            let text = serde_json::to_vec(&sidecar).map_err(io::Error::other)?;
            fs::rename(&files.part, &files.bytes)?;
            fs::write(&files.sidecar, text).inspect_err(|_| {
                let _gone = fs::remove_file(&files.bytes);
            })?;
            Ok(())
        })
        .await
    }

    /// An upload of the account read whole, for a draft that names it.
    ///
    /// # Errors
    ///
    /// Returns `NotFound` for an upload the account lacks, one past its
    /// time or one whose file does not open, `TooLarge` past `limit`.
    pub(crate) async fn read(
        &self,
        key: &AccountKey,
        id: &UploadId,
        limit: u64,
    ) -> Result<Vec<u8>, BlobError> {
        let mut reader = self.reader(key, id, limit).await?;
        tokio::task::spawn_blocking(move || {
            let mut bytes = Vec::new();
            while let Some(block) = reader.next()? {
                bytes.extend_from_slice(&block);
            }
            Ok::<_, Broken>(bytes)
        })
        .await
        .map_err(|_join| BlobError::Task)?
        .map_err(|Broken| BlobError::NotFound)
    }

    /// An upload of the account as a blob: its first block opened, the
    /// rest opened one at a time ahead of whoever reads it.
    ///
    /// # Errors
    ///
    /// As [`Uploads::read`].
    pub(crate) async fn open(
        &self,
        key: &AccountKey,
        id: &UploadId,
        limit: u64,
    ) -> Result<Blob, BlobError> {
        let reader = self.reader(key, id, limit).await?;
        let (reader, first) = next_block(reader)
            .await
            .map_err(|Broken| BlobError::NotFound)?;
        let first = first.unwrap_or_default();
        if reader.index == block_count(reader.size) {
            return Ok(Blob::streamed(first, None));
        }
        let (sender, receiver) = mpsc::channel(BLOB_BUFFER_WINDOWS - 1);
        tokio::spawn(stream(reader, sender));
        Ok(Blob::streamed(first, Some(receiver)))
    }

    /// Removes what lived past its time, a file a write left behind and
    /// a directory left empty.
    pub async fn sweep(&self) {
        self.sweep_at(SystemTime::now()).await;
    }

    /// The sweep as it runs at `now`.
    pub(crate) async fn sweep_at(&self, now: SystemTime) {
        let root = self.root.clone();
        let Ok(names) = tokio::task::spawn_blocking(move || account_names(&root)).await else {
            return;
        };
        for name in names {
            let key = AccountKey::new(name);
            let Some(dir) = account_dir(&self.root, &key) else {
                continue;
            };
            let (lock, ttl) = (self.lock(&key), self.bounds.ttl);
            let _held = lock.lock().await;
            if let Err(error) = tried(move || sweep_dir(&dir, now, ttl)).await {
                tracing::debug!(account = key.as_str(), %error, "an upload directory was not swept");
            }
        }
    }

    /// Sweeps at once and then every `UPLOAD_SWEEP_INTERVAL`.
    pub async fn sweep_periodically(self: Arc<Self>) {
        self.sweep_every(UPLOAD_SWEEP_INTERVAL).await;
    }

    /// Sweeps at once and then every `interval`.
    pub(crate) async fn sweep_every(self: Arc<Self>, interval: std::time::Duration) {
        let mut ticks = tokio::time::interval(interval);
        ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticks.tick().await;
            self.sweep().await;
        }
    }

    /// Removes every upload of the account with its directory.
    pub async fn forget(&self, key: &AccountKey) {
        let Some(dir) = account_dir(&self.root, key) else {
            return;
        };
        let lock = self.lock(key);
        let held = lock.lock().await;
        if let Err(error) = tried(move || fs::remove_dir_all(&dir)).await
            && error.kind() != io::ErrorKind::NotFound
        {
            tracing::debug!(account = key.as_str(), %error, "an upload directory was not removed");
        }
        drop(held);
        self.locks().remove(key);
    }

    /// The reader of a living upload within `limit`, its file open.
    async fn reader(
        &self,
        key: &AccountKey,
        id: &UploadId,
        limit: u64,
    ) -> Result<Reader, BlobError> {
        let dir = account_dir(&self.root, key).ok_or(BlobError::NotFound)?;
        let files = Files::of(&dir, id);
        let ttl = self.bounds.ttl;
        let (key, id, sealer) = (key.clone(), id.clone(), Arc::clone(&self.sealer));
        tokio::task::spawn_blocking(move || {
            let sidecar = sidecar(&files.sidecar).ok_or(BlobError::NotFound)?;
            if !sidecar.lives(SystemTime::now(), ttl) {
                return Err(BlobError::NotFound);
            }
            if sidecar.size > limit {
                return Err(BlobError::TooLarge);
            }
            let file = File::open(&files.bytes).map_err(|_| BlobError::NotFound)?;
            Ok(Reader {
                file,
                key,
                id,
                sealer,
                size: sidecar.size,
                index: 0,
            })
        })
        .await
        .map_err(|_join| BlobError::Task)?
    }

    fn lock(&self, key: &AccountKey) -> Arc<tokio::sync::Mutex<()>> {
        Arc::clone(self.locks().entry(key.clone()).or_default())
    }

    fn locks(&self) -> MutexGuard<'_, Locks> {
        self.locks.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A file call off the runtime.
pub(super) async fn blocking<T: Send + 'static>(
    call: impl FnOnce() -> Result<T, UploadError> + Send + 'static,
) -> Result<T, UploadError> {
    tokio::task::spawn_blocking(call)
        .await
        .map_err(|_join| UploadError::Task)?
}

/// A file call off the runtime whose failure the caller logs. A task
/// that ends early reads as an error with a fixed text.
async fn tried(call: impl FnOnce() -> io::Result<()> + Send + 'static) -> io::Result<()> {
    tokio::task::spawn_blocking(call)
        .await
        .map_err(|_join| io::Error::other("the task ended early"))
        .flatten()
}

/// The next block off the runtime, the reader handed back with it.
async fn next_block(mut reader: Reader) -> Result<(Reader, Option<Vec<u8>>), Broken> {
    tokio::task::spawn_blocking(move || {
        let block = reader.next()?;
        Ok((reader, block))
    })
    .await
    .map_err(|_join| Broken)?
}

/// Opens the blocks after the first until the upload ends, nobody reads
/// the stream or a block fails to open; the closing step or the error is
/// the last thing sent.
async fn stream(mut reader: Reader, sender: mpsc::Sender<Step>) {
    while !sender.is_closed() {
        match next_block(reader).await {
            Ok((next, Some(block))) => {
                reader = next;
                if !block.is_empty() && sender.send(Ok(Some(block))).await.is_err() {
                    return;
                }
            }
            Ok((_, None)) => {
                let _unread = sender.send(Ok(None)).await;
                return;
            }
            Err(Broken) => {
                let _unread = sender.send(Err(BlobError::NotFound)).await;
                return;
            }
        }
    }
}
