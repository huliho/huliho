// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What the commands on a selected mailbox answer, in the bridge's own
//! types, so a swap of the client library stays inside this module.

/// The messages one UID FETCH may answer; the header sync sizes its
/// batch by it.
pub const MAX_FETCH_MESSAGES: usize = 500;

/// The header bytes kept per message. The eleven fields asked for take
/// a few KiB even with hundreds of recipients.
pub const MAX_HEADER_BYTES: usize = 64 * 1024;

/// A mailbox opened read-only with EXAMINE (RFC 3501 section 6.3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selected {
    pub uid_validity: u32,
}

/// The UIDs `low` to `high`, both included. A range whose `low` is
/// above its `high` is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UidRange {
    pub low: u32,
    pub high: u32,
}

/// One message as a UID FETCH answered it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedMessage {
    pub uid: u32,
    /// The flags as the server spells them.
    pub flags: Vec<String>,
    /// INTERNALDATE in seconds since the epoch.
    pub received_at: i64,
    /// RFC822.SIZE.
    pub size: u32,
    /// The header fields asked for, cut at `MAX_HEADER_BYTES`.
    pub header: Vec<u8>,
    /// BODYSTRUCTURE; `None` when it was not asked for, did not arrive
    /// or holds more parts than the bridge keeps.
    pub structure: Option<BodyPart>,
}

/// What the attachment test needs of a MIME tree, every word in lower
/// case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyPart {
    Leaf {
        /// The top-level media type, `text` for one.
        media_type: String,
        /// Whether the disposition is `attachment`.
        attachment: bool,
    },
    Multipart {
        /// `related` for one.
        subtype: String,
        parts: Vec<BodyPart>,
    },
}
