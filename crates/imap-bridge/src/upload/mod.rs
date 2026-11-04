// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The upload store (RFC 8620 section 6): a blob a client sends before a
//! draft names it, kept sealed in a directory per account under the root
//! the host hands the bridge. One upload is its sealed blocks under its
//! id and a sidecar naming its type, its size and when it came. Nothing
//! of it reaches the database; it lives a day.

mod blocks;
mod disk;
mod store;
#[cfg(test)]
mod tests;
mod writing;

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::store::{AccountKey, UploadId};

pub use store::Uploads;
pub use writing::Writing;

/// What one upload and the attachments of one draft may weigh: the
/// common provider cap, under Dovecot's 40M, so a draft that saves on
/// the compose Dovecot also sends.
pub const MAX_ATTACHMENTS_BYTES: u64 = 25 * 1024 * 1024;

/// Uploads in flight per account; the host's lanes hold the same figure.
pub const MAX_CONCURRENT_UPLOADS: usize = 2;

/// What one account's uploads may take on disk at once, sealed.
pub const UPLOAD_QUOTA_BYTES: u64 = 256 * 1024 * 1024;

/// Uploads one account holds at once; it bounds the listing before a
/// write.
pub const MAX_UPLOADS_PER_ACCOUNT: usize = 1024;

/// How long an upload lives from its arrival: a day, above the hour RFC
/// 8620 section 6 keeps an unreferenced blob at least.
pub const UPLOAD_TTL: Duration = Duration::from_hours(24);

/// How often the store is swept; the first sweep runs at once.
pub const UPLOAD_SWEEP_INTERVAL: Duration = Duration::from_hours(1);

/// The plaintext of one sealed block; a read holds one at a time.
pub const UPLOAD_BLOCK_BYTES: usize = 1024 * 1024;

/// A file name longer than this is refused by every file system.
const MAX_DIRECTORY_NAME_BYTES: usize = 255;

/// What the sidecar's name adds to the upload's id.
const SIDECAR_SUFFIX: &str = ".json";

/// What a file still being written adds to the upload's id.
const PART_SUFFIX: &str = ".part";

/// What a store holds itself to.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Bounds {
    /// The bytes of one upload.
    pub bytes: u64,
    /// The bytes on disk of one account.
    pub quota: u64,
    /// The uploads of one account, in flight ones counted.
    pub count: usize,
    /// How long an upload lives.
    pub ttl: Duration,
}

/// The bounds of every store the host runs.
pub(crate) const BOUNDS: Bounds = Bounds {
    bytes: MAX_ATTACHMENTS_BYTES,
    quota: UPLOAD_QUOTA_BYTES,
    count: MAX_UPLOADS_PER_ACCOUNT,
    ttl: UPLOAD_TTL,
};

/// One upload as the store took it (RFC 8620 section 6.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upload {
    pub id: UploadId,
    pub media_type: String,
    pub size: u64,
}

/// Why an upload was not kept. Nothing of it stays behind.
#[derive(Debug, Error)]
pub enum UploadError {
    /// More bytes than one upload may hold.
    #[error("the upload is larger than the store takes")]
    TooLarge,
    /// No upload of the account left to evict for room.
    #[error("the account's uploads leave no room")]
    OverQuota,
    /// The host's key names no directory the store can make.
    #[error("the account key names no directory")]
    Unnamed,
    #[error("the host cannot seal the upload")]
    Sealing,
    #[error("the upload store cannot be written")]
    Io(#[from] std::io::Error),
    #[error("the task writing the upload ended early")]
    Task,
}

/// What the sidecar beside an upload says, in the clear: the type
/// without its parameters and the size name no person.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Sidecar {
    #[serde(rename = "type")]
    media_type: String,
    /// The plaintext length; the block count follows from it.
    size: u64,
    /// Seconds since the Unix epoch.
    created_at: u64,
}

impl Sidecar {
    /// Whether the upload still lives at `now`; a time past what the
    /// clock holds names no upload.
    fn lives(&self, now: SystemTime, ttl: Duration) -> bool {
        UNIX_EPOCH
            .checked_add(Duration::from_secs(self.created_at))
            .and_then(|created| created.checked_add(ttl))
            .is_some_and(|until| until > now)
    }
}

/// The directory of an account's uploads: the key as its name, when the
/// key holds only letters, digits, hyphens and underscores. Any other
/// key names no directory, so no key reaches past the root.
fn account_dir(root: &Path, key: &AccountKey) -> Option<PathBuf> {
    let name = key.as_str();
    let plain = name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
    (plain && !name.is_empty() && name.len() <= MAX_DIRECTORY_NAME_BYTES).then(|| root.join(name))
}

/// The files of one upload in its account's directory.
struct Files {
    bytes: PathBuf,
    sidecar: PathBuf,
    part: PathBuf,
}

impl Files {
    fn of(dir: &Path, id: &UploadId) -> Self {
        let id = id.as_str();
        Self {
            bytes: dir.join(id),
            sidecar: dir.join(format!("{id}{SIDECAR_SUFFIX}")),
            part: dir.join(format!("{id}{PART_SUFFIX}")),
        }
    }
}

/// Seconds since the Unix epoch at `now`.
fn epoch_seconds(now: SystemTime) -> u64 {
    now.duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}
