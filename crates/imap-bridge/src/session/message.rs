// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What the commands on a selected mailbox answer, in the bridge's own
//! types, so a swap of the client library stays inside this module.

use time::Date;
use tokio::time::Instant;

use super::Searching;

/// The messages one UID FETCH may answer; the header sync sizes its
/// batch by it.
pub const MAX_FETCH_MESSAGES: usize = 500;

/// The header bytes kept per message. The eleven fields asked for take
/// a few KiB even with hundreds of recipients.
pub const MAX_HEADER_BYTES: usize = 64 * 1024;

/// The bytes one window of a part asks for: one literal well under the
/// byte bound of a response, so a message of any size never lifts it.
pub const BODY_WINDOW_BYTES: u32 = 512 * 1024;

/// The longest text kept of one field of a part: a name, an id, a
/// description, a location or one parameter value. A file name runs to
/// a few hundred bytes; the rest is a sender padding a structure.
pub const MAX_PART_FIELD_BYTES: usize = 4096;

/// The parameters and the languages kept per part; a part carries a
/// handful.
pub const MAX_PART_FIELDS: usize = 32;

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

/// A mailbox opened for writing with SELECT (RFC 3501 section 6.3.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Writable {
    pub uid_validity: u32,
    /// The flags a STORE changes for good, as the server spells them
    /// (RFC 3501 section 7.1); `None` when the server named none, which
    /// reads as every flag.
    pub permanent: Option<Vec<String>>,
    /// The server answered with `[READ-ONLY]`: the session refuses every
    /// write on the mailbox before anything is sent.
    pub read_only: bool,
}

impl Writable {
    /// Whether a STORE of the flag lasts: the server names the flag or,
    /// for a keyword, `\*`.
    #[must_use]
    pub fn keeps(&self, flag: &str) -> bool {
        let Some(permanent) = &self.permanent else {
            return true;
        };
        let any_keyword = !flag.starts_with('\\') && permanent.iter().any(|found| found == "\\*");
        any_keyword
            || permanent
                .iter()
                .any(|found| found.eq_ignore_ascii_case(flag))
    }
}

/// The messages one STORE, MOVE, COPY or EXPUNGE names: with every UID
/// at its full ten digits the set stays near one KiB of the command
/// line.
pub const MAX_STORE_UIDS: usize = 100;

/// What one STORE changes on its messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreItem {
    /// `FLAGS` (RFC 3501 section 6.4.6).
    Flags,
    /// `X-GM-LABELS`, the labels of a message on a Gmail account.
    Labels,
}

/// One STORE: the flags or labels these messages of the selected
/// mailbox gain or lose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreAsk<'a> {
    /// The messages, `MAX_STORE_UIDS` at most.
    pub uids: &'a [u32],
    /// The flags as the wire carries them, a system flag a keyword maps
    /// to or an atom; or the labels as the server spells them.
    pub flags: &'a [String],
    /// Whether the messages gain them; they lose them otherwise.
    pub add: bool,
    pub item: StoreItem,
}

/// One `UID MOVE` or `UID COPY`: these messages of the selected mailbox
/// to another mailbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveAsk<'a> {
    /// The messages, `MAX_STORE_UIDS` at most.
    pub uids: &'a [u32],
    /// The wire name of the mailbox they go to.
    pub target: &'a str,
}

/// What the `COPYUID` code of a move or a copy names (RFC 4315 section
/// 3): the UIDVALIDITY of the target and, for every message that went,
/// its UID in the source and its UID in the target. A UID that names no
/// message of the source is passed over (RFC 3501 section 6.4.8) and has
/// no pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Moved {
    pub uid_validity: u32,
    /// `(source, target)`, in the order the code names them.
    pub uids: Vec<(u32, u32)>,
}

/// The bytes one term of a search may hold.
pub const SEARCH_TERM_BYTES: usize = 256;

/// The terms one search may carry, its text and its sender together.
pub const SEARCH_TERMS: usize = 8;

/// One search of the selected mailbox (RFC 3501 section 6.4.4): every
/// key must match. The text and the sender split into terms on
/// whitespace, a run in double quotes one term.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SearchAsk<'a> {
    /// As typed; a TEXT key per term, its quotes dropped.
    pub text: &'a str,
    /// As typed; a FROM key per term, its quotes dropped.
    pub from: &'a str,
    /// UNSEEN.
    pub unseen: bool,
    /// FLAGGED.
    pub flagged: bool,
    /// SINCE: an internal date on this day or later, its time and zone
    /// left out.
    pub since: Option<Date>,
    /// BEFORE: an internal date before this day.
    pub before: Option<Date>,
    /// The Gmail form, where the terms of the text and the sender as one
    /// phrase go as one X-GM-RAW string; `None` elsewhere.
    pub gmail: Option<GmailSearch<'a>>,
    /// How the server takes literals and whether it has the Gmail keys.
    pub server: Searching,
    /// When the search must be over: each read of its answer waits until
    /// then in place of the session's step. `None` keeps the step.
    pub until: Option<Instant>,
}

/// What a search on a Gmail account adds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GmailSearch<'a> {
    /// The label a mailbox scope stands for, as Gmail lists and stores
    /// it; the X-GM-LABELS key carries it in UTF-8.
    pub label: Option<&'a str>,
}

/// One quota root a mailbox falls under (RFC 9208 section 4.2.2) with
/// the two resources the bridge reads; one the server does not name is
/// `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotaRoot {
    /// The name as the server spells it; often empty.
    pub name: String,
    /// STORAGE in octets; the server counts in units of 1024 (RFC 9208
    /// section 5.1).
    pub storage: Option<Usage>,
    /// MESSAGE, in messages (RFC 9208 section 5.2).
    pub messages: Option<Usage>,
}

/// What one resource of a quota root uses and its limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Usage {
    pub used: u64,
    pub limit: u64,
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

/// The items of one header fetch beyond the fixed ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FetchItems {
    /// BODYSTRUCTURE.
    pub structure: bool,
    /// X-GM-LABELS, X-GM-MSGID and X-GM-THRID, which a Gmail account
    /// carries.
    pub gmail: bool,
}

/// The three items of one message on a Gmail account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GmailItems {
    /// X-GM-LABELS as the server spells them.
    pub labels: Vec<String>,
    /// X-GM-MSGID.
    pub msgid: u64,
    /// X-GM-THRID.
    pub thrid: u64,
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
    /// The Gmail items; `None` when they were not asked for or the line
    /// lacks one of the three.
    pub gmail: Option<GmailItems>,
}

/// One part of a MIME tree as BODYSTRUCTURE describes it (RFC 3501
/// section 7.4.2), every media word in lower case. A message inside a
/// message is a leaf: its own tree is never read (RFC 8621 section
/// 4.1.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyPart {
    Leaf(Leaf),
    Multipart(Multipart),
}

/// A part that holds content.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Leaf {
    /// The top-level media type, `text` for one.
    pub media_type: String,
    /// The subtype, `plain` or `html` for one.
    pub subtype: String,
    /// The parameters of the type as the server spells them.
    pub parameters: Vec<(String, String)>,
    /// Content-ID as written, angle brackets included.
    pub id: Option<String>,
    pub description: Option<String>,
    /// The transfer encoding in lower case, `base64` for one.
    pub encoding: String,
    /// The size of the part in its transfer encoding.
    pub bytes: u32,
    /// The line count a text or message part carries.
    pub lines: Option<u32>,
    pub disposition: Option<Disposition>,
    pub language: Option<Vec<String>>,
    pub location: Option<String>,
}

impl Leaf {
    /// Whether the sender marked the part as an attachment.
    #[must_use]
    pub fn is_attachment(&self) -> bool {
        self.disposition
            .as_ref()
            .is_some_and(|disposition| disposition.kind == "attachment")
    }

    /// A `text/plain` or `text/html` part that is not an attachment.
    #[must_use]
    pub fn is_text_body(&self) -> bool {
        self.media_type == "text"
            && (self.subtype == "plain" || self.subtype == "html")
            && !self.is_attachment()
    }
}

/// A part that holds other parts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Multipart {
    /// `related` for one.
    pub subtype: String,
    pub parameters: Vec<(String, String)>,
    pub disposition: Option<Disposition>,
    pub language: Option<Vec<String>>,
    pub location: Option<String>,
    pub parts: Vec<BodyPart>,
}

#[cfg(test)]
impl Multipart {
    /// A multipart of that subtype over the parts, for the tests.
    pub(crate) fn of(subtype: &str, parts: Vec<BodyPart>) -> BodyPart {
        BodyPart::Multipart(Self {
            subtype: subtype.to_owned(),
            parts,
            ..Self::default()
        })
    }
}

/// Content-Disposition (RFC 2183): the kind in lower case and its
/// parameters as the server spells them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Disposition {
    pub kind: String,
    pub parameters: Vec<(String, String)>,
}

#[cfg(test)]
impl Disposition {
    /// An attachment without parameters, for the tests.
    pub(crate) fn attachment() -> Self {
        Self {
            kind: "attachment".to_owned(),
            parameters: Vec::new(),
        }
    }
}

/// What one structure fetch answered: the tree and the header fields
/// asked beside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Structure {
    /// `None` for a message with more parts than the bridge keeps.
    pub tree: Option<BodyPart>,
    /// The fields asked for, cut at `MAX_HEADER_BYTES`; empty when none
    /// were.
    pub header: Vec<u8>,
}

/// One window of a part: `bytes` from `offset`, `BODY_WINDOW_BYTES` at
/// most (RFC 3501 section 6.4.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartWindow {
    pub offset: u32,
    pub bytes: u32,
}

/// One window of one section of one message, asked by a partial fetch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartAsk<'a> {
    pub uid: u32,
    /// Empty for the whole message, `TEXT` for a message that is one
    /// part, a part number otherwise.
    pub section: &'a str,
    pub window: PartWindow,
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
    /// X-GM-LABELS where they were asked for and the line carried them.
    pub labels: Option<Vec<String>>,
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
