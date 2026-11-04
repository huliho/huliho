// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! How a sync goes through the UIDs of its folder, one slice at a time.

use crate::session::UidRange;
use crate::store::Synced;

/// Which way a sync walks the UIDs of its folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Walk {
    /// The first sync, newest first, with the server's values of the
    /// opening.
    Down { synced: Synced },
    /// The new mail of a folder that is done, oldest first, so every
    /// batch moves the recorded UIDNEXT.
    Up,
    /// A few UIDs read beside both, which move nothing on record.
    Beside,
}

/// UIDs lowest first, so the range from the first to the last names
/// them and nothing a batch before it took.
pub(super) struct Slice {
    pub(super) uids: Vec<u32>,
    pub(super) structure: bool,
}

impl Slice {
    pub(super) fn range(&self) -> Option<UidRange> {
        Some(UidRange {
            low: *self.uids.first()?,
            high: *self.uids.last()?,
        })
    }
}
