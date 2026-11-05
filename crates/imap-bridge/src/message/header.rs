// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The header block of a message on its way out (RFC 8621 section 7.5):
//! the copy for the wire leaves every Bcc field behind, the copy for the
//! Sent mailbox keeps it, and both carry the moment of the send as their
//! one Date (RFC 5322 section 3.6.1). The body and every other field stay
//! byte for byte as the draft has them. A send goes only where the
//! recipient reader names the fields of the wire copy as it names the
//! draft's, Bcc and Date aside.

use mail_builder::headers::date::Date;
use mail_parser::{HeaderName, MessageParser};

use super::envelope::Unsendable;

/// The fields no copy on the wire carries (RFC 5322 sections 3.6.3 and
/// 3.6.6), lowercased.
const BLIND: [&[u8]; 2] = [b"bcc", b"resent-bcc"];

/// The field the send stamps, lowercased.
const DATE: &[u8] = b"date";

/// The two copies a send writes of one message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Forms {
    /// What the submission server gets.
    pub wire: Vec<u8>,
    /// What the Sent mailbox keeps.
    pub sent: Vec<u8>,
}

/// Both copies of the message, dated `date` in seconds since the epoch,
/// where the parser the recipient reader uses names the fields of the
/// wire copy as it names the draft's, in order, Bcc, Resent-Bcc and Date
/// aside, with one Date and no Bcc among them.
///
/// # Errors
///
/// Returns the property at fault where the two readings differ.
pub fn sendable(message: &[u8], date: i64) -> Result<Forms, Unsendable> {
    let forms = forms(message, date);
    let draft = names(message);
    let wire = names(&forms.wire);
    let kept = draft.iter().filter(|name| !blind(name) && !dated(name));
    let agreed = !wire.iter().any(blind)
        && wire.iter().filter(|name| dated(name)).count() == 1
        && wire.iter().filter(|name| !dated(name)).eq(kept);
    if agreed {
        Ok(forms)
    } else {
        Err(Unsendable::InvalidEmail("headers"))
    }
}

/// Whether the name is one no wire copy carries.
fn blind(name: &HeaderName<'_>) -> bool {
    matches!(name, HeaderName::Bcc | HeaderName::ResentBcc)
}

/// Whether the name is the field the send stamps.
fn dated(name: &HeaderName<'_>) -> bool {
    *name == HeaderName::Date
}

/// The names of the fields the recipient reader's parser finds, in
/// order.
pub(crate) fn names(bytes: &[u8]) -> Vec<HeaderName<'_>> {
    MessageParser::default()
        .parse_headers(bytes)
        .map(|parsed| {
            parsed
                .headers()
                .iter()
                .map(|header| header.name.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// Both copies of the message, dated `date` in seconds since the epoch.
/// The Date takes the place of the first Date field, or the top where the
/// message has none.
fn forms(message: &[u8], date: i64) -> Forms {
    let (header, body) = split(message);
    let fields = fields(header);
    let stamp = format!("Date: {}\r\n", Date::new(date).to_rfc822());
    Forms {
        wire: rewritten(&fields, body, stamp.as_bytes(), true),
        sent: rewritten(&fields, body, stamp.as_bytes(), false),
    }
}

/// The header block and what follows it, the empty line included; a
/// message without an empty line is all header.
fn split(message: &[u8]) -> (&[u8], &[u8]) {
    let mut start = 0;
    while start < message.len() {
        let end = message[start..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(message.len(), |at| start + at + 1);
        let line = &message[start..end];
        if line == b"\r\n" || line == b"\n" {
            return message.split_at(start);
        }
        start = end;
    }
    (message, &[])
}

/// The fields of a header block, each with its folded lines (RFC 5322
/// section 2.2.3): a line that opens with white space continues the field
/// before it.
fn fields(header: &[u8]) -> Vec<&[u8]> {
    let mut fields: Vec<&[u8]> = Vec::new();
    let mut start = 0;
    let mut field_start = 0;
    while start < header.len() {
        let end = header[start..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(header.len(), |at| start + at + 1);
        let folded = matches!(header[start], b' ' | b'\t');
        if !folded && start > field_start {
            fields.push(&header[field_start..start]);
            field_start = start;
        }
        start = end;
    }
    if header.len() > field_start {
        fields.push(&header[field_start..]);
    }
    fields
}

/// Whether the field carries one of the names, read as the recipient
/// reader reads one: colons before it and all white space dropped, case
/// left aside, up to the colon after it; a line without one yields to the next.
fn named(field: &[u8], names: &[&[u8]]) -> bool {
    let mut lines = field.split_inclusive(|byte| *byte == b'\n');
    let name = lines.find_map(name_of);
    name.is_some_and(|name| names.contains(&name.as_slice()))
}

/// The lowercased name of one line, where a colon follows it.
fn name_of(line: &[u8]) -> Option<Vec<u8>> {
    let mut name = Vec::new();
    for byte in line {
        match byte {
            b':' if name.is_empty() => {}
            b':' => return Some(name),
            b'\n' => return None,
            _ if byte.is_ascii_whitespace() => {}
            _ => name.push(byte.to_ascii_lowercase()),
        }
    }
    None
}

fn rewritten(fields: &[&[u8]], body: &[u8], stamp: &[u8], wire: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(
        fields.iter().map(|field| field.len()).sum::<usize>() + body.len() + stamp.len(),
    );
    let mut dated = !fields.iter().any(|field| named(field, &[DATE]));
    if dated {
        out.extend_from_slice(stamp);
    }
    for field in fields {
        if named(field, &[DATE]) {
            if !dated {
                out.extend_from_slice(stamp);
                dated = true;
            }
            continue;
        }
        if wire && named(field, &BLIND) {
            continue;
        }
        out.extend_from_slice(field);
        if !field.ends_with(b"\n") {
            out.extend_from_slice(b"\r\n");
        }
    }
    out.extend_from_slice(body);
    out
}

#[cfg(test)]
mod tests;
