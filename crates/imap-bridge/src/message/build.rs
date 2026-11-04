// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The message of a draft through mail-builder: the header fields, the
//! tree as the object gives it, names under RFC 2183 and 2231, a
//! non-ASCII header under RFC 2047 and the transfer encoding per part
//! mail-builder's choice, so a line past 998 octets leaves encoded (RFC
//! 5322 section 2.1.1). No `User-Agent` and nothing of the instance.

use std::collections::HashMap;
use std::io;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use mail_builder::MessageBuilder;
use mail_builder::headers::address::Address as Written;
use mail_builder::headers::content_type::ContentType;
use mail_builder::headers::date::Date;
use mail_builder::headers::message_id::MessageId;
use mail_builder::mime::{BodyPart, MimePart};

use super::draft::{Content, Draft, Part, Stamp};
use crate::store::Address;

/// The bytes a message holds at most: the 25 MiB of attachments a draft
/// may carry grow to 34.3 MiB in base64 with its line breaks and a request
/// leaves 1 MiB for the text parts, so a draft that fits its parts fits
/// this; it equals the 40M Dovecot's submission takes by default.
pub const MAX_MESSAGE_BYTES: usize = 40 * 1024 * 1024;

/// The longest line a sender writes of base64 (RFC 2045 section 6.8).
const BASE64_LINE: usize = 76;

/// The longest line of a message (RFC 5322 section 2.1.1), its line end
/// left out.
const MAX_LINE_BYTES: usize = 998;

const LINE_END: &[u8] = b"\r\n";

/// The bytes behind the blob ids of a draft.
pub type Blobs = HashMap<String, Vec<u8>>;

/// The whole message. A blob the map lacks reads as empty, since the
/// caller resolved every id the tree names.
///
/// # Errors
///
/// Returns the writer's failure, which a `Vec` never gives.
pub fn build(draft: &Draft, stamp: &Stamp, blobs: &Blobs) -> io::Result<Vec<u8>> {
    let personal = &draft.personal;
    let mut message = MessageBuilder::new()
        .message_id(stamp.message_id.as_str())
        .date(Date::new(stamp.date));
    if let Some(senders) = filled(personal.from.as_deref()) {
        message = message.from(written(senders));
    }
    if let Some(sender) = filled(personal.sender.as_deref()) {
        message = message.sender(written(sender));
    }
    if let Some(replies) = filled(personal.reply_to.as_deref()) {
        message = message.reply_to(written(replies));
    }
    if let Some(to) = filled(personal.to.as_deref()) {
        message = message.to(written(to));
    }
    if let Some(cc) = filled(personal.cc.as_deref()) {
        message = message.cc(written(cc));
    }
    if let Some(bcc) = filled(personal.bcc.as_deref()) {
        message = message.bcc(written(bcc));
    }
    if let Some(subject) = &personal.subject {
        message = message.subject(subject.as_str());
    }
    if let Some(ids) = filled(personal.in_reply_to.as_deref()) {
        message = message.in_reply_to(ids_of(ids));
    }
    if let Some(ids) = filled(personal.references.as_deref()) {
        message = message.references(ids_of(ids));
    }
    message.body(mime(&draft.body, blobs)).write_to_vec()
}

/// The bytes the leaves of a tree add to its message before any encoding,
/// each leaf where it stands, so a blob many parts name weighs as many
/// times. The writer reserves what it estimates up front, so a tree is
/// weighed before it is built.
#[must_use]
pub fn weight(part: &Part, blobs: &Blobs) -> u64 {
    let bytes = |length: usize| u64::try_from(length).unwrap_or(u64::MAX);
    match &part.content {
        Content::Value(value) => bytes(value.len()),
        Content::Blob(id) => blobs.get(id).map_or(0, |blob| bytes(blob.len())),
        Content::Parts(parts) => parts
            .iter()
            .fold(0, |sum, part| sum.saturating_add(weight(part, blobs))),
    }
}

/// A list that holds something.
fn filled<T>(list: Option<&[T]>) -> Option<&[T]> {
    list.filter(|list| !list.is_empty())
}

fn written(list: &[Address]) -> Written<'_> {
    let addresses = list
        .iter()
        .map(|address| Written::new_address(address.name.as_deref(), address.email.as_str()));
    Written::new_list(addresses.collect())
}

fn ids_of(ids: &[String]) -> MessageId<'_> {
    MessageId::new_list(ids.iter().map(String::as_str))
}

/// One part of the tree with its Content-Type, its disposition and its
/// Content-ID. A name stands in the disposition, in the type where the
/// object names no disposition (RFC 2183 section 2.3, RFC 2045 section
/// 5.1).
fn mime<'x>(part: &'x Part, blobs: &'x Blobs) -> MimePart<'x> {
    let mut content_type = ContentType::new(part.media_type.as_str());
    if matches!(part.content, Content::Value(_)) {
        content_type = content_type.attribute("charset", "utf-8");
    }
    for (name, value) in &part.parameters {
        content_type = content_type.attribute(name.as_str(), value.as_str());
    }
    if let (Some(name), None) = (&part.name, &part.disposition) {
        content_type = content_type.attribute("name", name.as_str());
    }
    let mut mime = match &part.content {
        Content::Value(value) => MimePart::new(content_type, BodyPart::Text(crlf(value).into())),
        Content::Blob(id) => {
            let bytes = blobs.get(id).map(Vec::as_slice).unwrap_or_default();
            blob(content_type, bytes)
        }
        Content::Parts(parts) => {
            let children = parts.iter().map(|part| mime(part, blobs)).collect();
            MimePart::new(content_type, BodyPart::Multipart(children))
        }
    };
    if let Some(kind) = &part.disposition {
        let mut disposition = ContentType::new(kind.as_str());
        if let Some(name) = &part.name {
            disposition = disposition.attribute("filename", name.as_str());
        }
        mime = mime.header("Content-Disposition", disposition);
    }
    if let Some(cid) = &part.cid {
        mime = mime.cid(cid.as_str());
    }
    mime
}

/// A part by its bytes. A message inside a message stays as it is where
/// its lines allow, since RFC 2046 section 5.2.1 admits no base64 there.
/// Text that holds what mail-builder would write as it stands, a NUL or a
/// lone CR, leaves in base64; every other choice is mail-builder's.
fn blob<'x>(content_type: ContentType<'x>, bytes: &'x [u8]) -> MimePart<'x> {
    if content_type.c_type.starts_with("message/") && whole_lines(bytes) {
        let encoding = if bytes.is_ascii() { "7bit" } else { "8bit" };
        return MimePart::new(content_type, bytes).transfer_encoding(encoding);
    }
    if content_type.is_text() && !sendable_text(bytes) {
        return MimePart::new(content_type, base64_lines(bytes)).transfer_encoding("base64");
    }
    MimePart::new(content_type, bytes)
}

/// Whether the bytes go on the wire as they are: no NUL, every line end a
/// CRLF and no line past the limit.
fn whole_lines(bytes: &[u8]) -> bool {
    !bytes.contains(&0)
        && sendable_text(bytes)
        && !has_bare_line_feed(bytes)
        && bytes
            .split(|byte| *byte == b'\n')
            .all(|line| line.len() <= MAX_LINE_BYTES + 1)
}

/// Whether text holds neither a NUL nor a CR outside a line end.
fn sendable_text(bytes: &[u8]) -> bool {
    !bytes.contains(&0)
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| *byte != b'\r' || bytes.get(index + 1) == Some(&b'\n'))
}

fn has_bare_line_feed(bytes: &[u8]) -> bool {
    let mut before = 0;
    bytes.iter().any(|byte| {
        let bare = *byte == b'\n' && before != b'\r';
        before = *byte;
        bare
    })
}

fn base64_lines(bytes: &[u8]) -> Vec<u8> {
    let encoded = BASE64.encode(bytes);
    let lines = encoded.len().div_ceil(BASE64_LINE) * LINE_END.len();
    let mut out = Vec::with_capacity(encoded.len() + lines);
    for line in encoded.as_bytes().chunks(BASE64_LINE) {
        out.extend_from_slice(line);
        out.extend_from_slice(LINE_END);
    }
    out
}

/// A body value with every line end a CRLF (RFC 5322 section 2.3); JMAP
/// gives line ends as LF (RFC 8621 section 4.1.4).
fn crlf(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            chars.next_if_eq(&'\n');
            out.push_str("\r\n");
        } else if c == '\n' {
            out.push_str("\r\n");
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests;
