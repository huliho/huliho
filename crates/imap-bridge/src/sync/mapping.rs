// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! One fetched message as the facts the store writes: flags to
//! keywords (RFC 8621 section 4.1.1), the attachment mark (section
//! 4.1.4) and the header fields. No connection is involved, so the
//! property tests run on these functions alone.

use std::collections::BTreeMap;

use super::{headers, preview};
use crate::session::{BodyPart, FetchedMessage};
use crate::store::EmailFacts;

/// The four system flags with a keyword of their own.
const SYSTEM_KEYWORDS: [(&str, &str); 4] = [
    ("\\Seen", "$seen"),
    ("\\Flagged", "$flagged"),
    ("\\Answered", "$answered"),
    ("\\Draft", "$draft"),
];

/// The bytes no keyword may hold next to controls, the space and
/// everything past ASCII (RFC 8621 section 4.1.1).
const KEYWORD_FORBIDDEN: &[u8] = b"(){]%*\"\\";

/// The longest keyword (RFC 8621 section 4.1.1).
const MAX_KEYWORD_BYTES: usize = 255;

/// The facts of one message; `None` for one flagged `\Deleted`, which
/// JMAP never shows (RFC 8621 section 4.1.1). The part its preview is
/// read from is chosen here, while the structure is at hand.
#[must_use]
pub fn email(message: &FetchedMessage) -> Option<EmailFacts> {
    if is_deleted(&message.flags) {
        return None;
    }
    let headers = headers::parse(&message.header);
    let mut personal = headers.personal;
    personal.preview_part = message.structure.as_ref().and_then(preview::part);
    Some(EmailFacts {
        uid: message.uid,
        keywords: keywords(&message.flags),
        size: message.size,
        received_at: message.received_at,
        sent_at: headers.sent_at,
        has_attachment: has_attachment(&message.flags, message.structure.as_ref()),
        personal,
    })
}

/// Whether the flags hold `\Deleted` in any case.
#[must_use]
pub fn is_deleted(flags: &[String]) -> bool {
    flags.iter().any(|flag| is(flag, "\\Deleted"))
}

fn is(flag: &str, name: &str) -> bool {
    flag.eq_ignore_ascii_case(name)
}

/// The keywords of a message: the four system flags by their names,
/// every other backslash flag dropped, every other flag in lower case
/// where it is a keyword at all.
#[must_use]
pub fn keywords(flags: &[String]) -> BTreeMap<String, bool> {
    flags
        .iter()
        .filter_map(|flag| keyword(flag))
        .map(|keyword| (keyword, true))
        .collect()
}

fn keyword(flag: &str) -> Option<String> {
    if let Some((_, keyword)) = SYSTEM_KEYWORDS.iter().find(|(name, _)| is(flag, name)) {
        return Some((*keyword).to_owned());
    }
    let valid = !flag.is_empty()
        && flag.len() <= MAX_KEYWORD_BYTES
        && flag
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && !KEYWORD_FORBIDDEN.contains(&byte));
    valid.then(|| flag.to_ascii_lowercase())
}

/// Dovecot's two flags win where present; otherwise the structure
/// decides and a message without one has no mark.
#[must_use]
pub fn has_attachment(flags: &[String], structure: Option<&BodyPart>) -> bool {
    if flags.iter().any(|flag| is(flag, "$HasAttachment")) {
        return true;
    }
    if flags.iter().any(|flag| is(flag, "$HasNoAttachment")) {
        return false;
    }
    structure.is_some_and(|part| attaches(part, false))
}

/// Every leaf that is not text and every part sent as an attachment
/// count, outside the inline images of a multipart/related body.
fn attaches(part: &BodyPart, related: bool) -> bool {
    match part {
        BodyPart::Multipart { subtype, parts } => parts
            .iter()
            .any(|part| attaches(part, subtype == "related")),
        BodyPart::Leaf {
            attachment: true, ..
        } => true,
        BodyPart::Leaf { media_type, .. } => {
            media_type != "text" && !(related && media_type == "image")
        }
    }
}
