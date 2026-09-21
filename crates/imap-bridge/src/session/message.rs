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
    /// The mod-sequence the mailbox stood at; `None` without CONDSTORE
    /// (RFC 7162 section 3.1.2).
    pub highest_modseq: Option<u64>,
    /// The UID the next message gets; `None` when the server left it
    /// out, which RFC 3501 section 6.3.1 allows.
    pub uid_next: Option<u32>,
    /// EXISTS: the messages the mailbox holds.
    pub messages: u32,
}

/// The fixed words of an answer with more messages than one command may
/// carry.
pub const MESSAGE_LIMIT: &str = "the answer passes the message limit";

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

/// What the attachment test and the choice of a preview part need of a
/// MIME tree, every word in lower case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyPart {
    Leaf {
        /// The top-level media type, `text` for one.
        media_type: String,
        /// The subtype, `plain` or `html` for one.
        subtype: String,
        /// Whether the disposition is `attachment`.
        attachment: bool,
        /// The size of the part in its transfer encoding.
        bytes: u32,
    },
    Multipart {
        /// `related` for one.
        subtype: String,
        parts: Vec<BodyPart>,
    },
}

/// The flag fetches one command may answer with; a server that sends
/// more fails the answer.
pub const MAX_FLAGGED: usize = 10_000;

/// The previews one command fetches.
pub const MAX_PREVIEWS: usize = 100;

/// The bytes asked of the MIME header of a preview part. Whoever sent
/// the mail wrote that header and nothing between them and the server
/// bounds it.
pub const PREVIEW_HEADER_BYTES: u32 = 4096;

/// The most text one preview fetch asks for and keeps.
pub const MAX_PREVIEW_TEXT_BYTES: u32 = 64 * 1024;

/// Which messages of the selected mailbox a flag fetch covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlagFetch {
    /// Every message whose mod-sequence is above this one (RFC 7162
    /// section 3.1.4.1).
    ChangedSince(u64),
    /// Every message of the range.
    Range(UidRange),
}

/// The flags of one message as a flag fetch answered them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flagged {
    pub uid: u32,
    pub flags: Vec<String>,
}

/// One text part of several messages, asked for a preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewAsk<'a> {
    /// The messages, `MAX_PREVIEWS` at most.
    pub uids: &'a [u32],
    /// The part number, dots between its levels; empty for a message
    /// that is one part.
    pub path: &'a str,
    /// How much of the text to ask for, `MAX_PREVIEW_TEXT_BYTES` at most.
    pub text_bytes: u32,
}

/// The start of one text part: the header that says how it is encoded
/// and the first bytes of its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewBytes {
    pub uid: u32,
    pub header: Vec<u8>,
    pub text: Vec<u8>,
}
