// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! One fetched message as the facts the store writes: flags to
//! keywords (RFC 8621 section 4.1.1), the attachment mark (section
//! 4.1.4) and the header fields. No connection is involved, so the
//! property tests run on these functions alone.

use std::collections::BTreeMap;

use super::{headers, preview};
use crate::gmail;
use crate::session::{BodyPart, FetchedMessage, GmailItems};
use crate::store::{EmailFacts, GmailFacts};

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
        gmail: message.gmail.as_ref().map(gmail_facts),
        personal,
    })
}

/// The Gmail items in the bridge's spelling, each label once.
fn gmail_facts(items: &GmailItems) -> GmailFacts {
    GmailFacts {
        labels: labels(&items.labels),
        msgid: items.msgid,
        thrid: items.thrid,
    }
}

/// The labels of one message in the bridge's spelling, each once.
#[must_use]
pub fn labels(found: &[String]) -> Vec<String> {
    let mut labels: Vec<String> = Vec::new();
    for label in found.iter().map(|label| gmail::canonical(label)) {
        if !labels.contains(&label) {
            labels.push(label);
        }
    }
    labels
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
    canonical_keyword(flag)
}

/// A keyword as the rows hold it, in lower case; `None` for a text that
/// is no keyword (RFC 8621 section 4.1.1).
#[must_use]
pub fn canonical_keyword(text: &str) -> Option<String> {
    let valid = !text.is_empty()
        && text.len() <= MAX_KEYWORD_BYTES
        && text
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && !KEYWORD_FORBIDDEN.contains(&byte));
    valid.then(|| text.to_ascii_lowercase())
}

/// The flag a keyword in lower case is stored as: the four system
/// keywords by their flags, any other keyword as itself, which is an
/// atom since a keyword holds no atom special.
#[must_use]
pub fn flag(keyword: &str) -> String {
    SYSTEM_KEYWORDS
        .iter()
        .find(|(_, name)| *name == keyword)
        .map_or(keyword, |(flag, _)| flag)
        .to_owned()
}

/// Whether the flag is one of the four a system keyword maps to.
#[must_use]
pub fn is_system_flag(flag: &str) -> bool {
    SYSTEM_KEYWORDS.iter().any(|(name, _)| *name == flag)
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
        BodyPart::Multipart(multipart) => multipart
            .parts
            .iter()
            .any(|part| attaches(part, multipart.subtype == "related")),
        BodyPart::Leaf(leaf) => {
            leaf.is_attachment()
                || (leaf.media_type != "text" && !(related && leaf.media_type == "image"))
        }
    }
}
