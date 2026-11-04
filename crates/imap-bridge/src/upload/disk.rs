// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What an account's directory holds and what leaves it: the listing a
//! write makes room by, the eviction oldest first and the sweep. Each
//! runs on the blocking pool under the account's lock.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::{Bounds, Files, PART_SUFFIX, SIDECAR_SUFFIX, Sidecar, UploadError, account_dir};
use crate::store::{AccountKey, UploadId};

/// The sidecar at `path`; `None` where it is missing or unreadable.
pub(super) fn sidecar(path: &Path) -> Option<Sidecar> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

/// What an account's directory holds: its completed uploads and the
/// uploads in flight, with the octets both take on disk.
pub(super) struct Listing {
    dir: PathBuf,
    done: Vec<UploadId>,
    in_flight: usize,
    used: u64,
    lengths: HashMap<UploadId, u64>,
}

impl Listing {
    pub(super) fn of(dir: &Path) -> io::Result<Self> {
        let mut listing = Self {
            dir: dir.to_owned(),
            done: Vec::new(),
            in_flight: 0,
            used: 0,
            lengths: HashMap::new(),
        };
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let length = match entry.metadata() {
                Ok(metadata) => metadata.len(),
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            if name
                .strip_suffix(PART_SUFFIX)
                .and_then(UploadId::parse)
                .is_some()
            {
                listing.in_flight += 1;
                listing.used = listing.used.saturating_add(length);
            } else if let Some(id) = UploadId::parse(&name) {
                listing.used = listing.used.saturating_add(length);
                listing.lengths.insert(id.clone(), length);
                listing.done.push(id);
            }
        }
        Ok(listing)
    }

    /// Evicts the oldest completed uploads until `more` uploads of
    /// `bytes` octets fit (RFC 8620 section 6); an upload without its
    /// sidecar goes first, since it opens for no one.
    pub(super) fn make_room(
        &mut self,
        more: usize,
        bytes: u64,
        bounds: Bounds,
    ) -> Result<(), UploadError> {
        let in_flight = self.in_flight;
        let crowded = |done: usize, used: u64| {
            done + in_flight + more > bounds.count || used.saturating_add(bytes) > bounds.quota
        };
        if !crowded(self.done.len(), self.used) {
            return Ok(());
        }
        let dir = self.dir.clone();
        let mut aged: Vec<(Option<u64>, UploadId)> = self
            .done
            .drain(..)
            .map(|id| {
                let created = sidecar(&Files::of(&dir, &id).sidecar).map(|read| read.created_at);
                (created, id)
            })
            .collect();
        aged.sort();
        aged.reverse();
        while crowded(aged.len(), self.used) {
            let (_, oldest) = aged.pop().ok_or(UploadError::OverQuota)?;
            remove(&Files::of(&dir, &oldest))?;
            let length = self.lengths.remove(&oldest).unwrap_or_default();
            self.used = self.used.saturating_sub(length);
        }
        self.done = aged.into_iter().map(|(_, id)| id).collect();
        Ok(())
    }
}

/// Removes an upload's bytes and its sidecar; one already gone is fine.
fn remove(files: &Files) -> io::Result<()> {
    for path in [&files.bytes, &files.sidecar] {
        match fs::remove_file(path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
    }
    Ok(())
}

/// The names under the root that name an account's directory.
pub(super) fn account_names(root: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
        .filter(|name| account_dir(root, &AccountKey::new(name.as_str())).is_some())
        .collect()
}

/// One account's directory swept at `now`: an upload past its time, a
/// part file, an upload without its sidecar and a sidecar without its
/// upload older than the TTL; the directory itself once empty.
pub(super) fn sweep_dir(dir: &Path, now: SystemTime, ttl: Duration) -> io::Result<()> {
    let stale = |path: &Path| {
        fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .is_ok_and(|modified| modified.checked_add(ttl).is_some_and(|until| until <= now))
    };
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let id = name
            .strip_suffix(PART_SUFFIX)
            .or_else(|| name.strip_suffix(SIDECAR_SUFFIX))
            .unwrap_or(name);
        let Some(id) = UploadId::parse(id) else {
            continue;
        };
        let files = Files::of(dir, &id);
        if path == files.part {
            if stale(&path) {
                fs::remove_file(&path)?;
            }
            continue;
        }
        let gone = match sidecar(&files.sidecar) {
            Some(read) => !read.lives(now, ttl),
            None => stale(&path),
        };
        if gone {
            remove(&files)?;
        }
    }
    if fs::read_dir(dir)?.next().is_none() {
        fs::remove_dir(dir)?;
    }
    Ok(())
}
