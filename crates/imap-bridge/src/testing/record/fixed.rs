// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The fixed shapes a redacted transcript is made of and how a value is
//! found in a run of text: the redaction writes these shapes, a corpus
//! is written in them and the scan admits nothing else.

/// The domain every fixed address and message id sits under (RFC 2606).
pub const FIXED_DOMAIN: &str = "example.test";

/// The user the redacted LOGIN names; the replay signs in as it.
pub const RECORDED_USER: &str = "sanne@example.test";

/// The words of a greeting once its own are gone.
pub const GREETING: &str = "ready";

/// A renumbered Gmail id lies below this; Gmail's own are 64-bit values
/// far above it.
pub const MAX_RENUMBERED_ID: u64 = 1_000_000;

/// The filler a body text becomes, repeated to the length the server
/// sent.
pub const BODY_FILL: &str = "Text of the message body, replaced. ";

/// Renumbered subjects count from here; a corpus numbers its own from
/// one.
pub const SUBJECTS_FROM: usize = 1000;

/// The header fields that carry addresses.
pub const ADDRESS_FIELDS: [&str; 6] = ["FROM", "SENDER", "REPLY-TO", "TO", "CC", "BCC"];

/// The header fields that carry message ids.
pub const ID_FIELDS: [&str; 3] = ["MESSAGE-ID", "IN-REPLY-TO", "REFERENCES"];

/// The header field that carries the subject.
pub const SUBJECT_FIELD: &str = "SUBJECT";

/// The subject a message gets, in the shape a corpus uses.
#[must_use]
pub fn fixed_subject(number: usize) -> String {
    format!("Message {number}")
}

/// Whether a subject is one a corpus wrote, a reply to one included.
#[must_use]
pub fn is_fixed_subject(value: &str) -> bool {
    let value = value.strip_prefix("Re: ").unwrap_or(value);
    value.strip_prefix("Message ").is_some_and(|digits| {
        !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
    })
}

/// Whether an address, or the inside of a message id, sits under the
/// fixed domain.
#[must_use]
pub fn is_fixed_address(token: &str) -> bool {
    token
        .rsplit_once('@')
        .is_some_and(|(local, domain)| !local.is_empty() && domain == FIXED_DOMAIN)
}

/// The message id a live one becomes.
#[must_use]
pub fn fixed_message_id(number: usize) -> String {
    format!("<{number}@{FIXED_DOMAIN}>")
}

/// The filler for a body of `bytes` bytes.
#[must_use]
pub fn fill(bytes: usize) -> String {
    BODY_FILL.repeat(bytes.div_ceil(BODY_FILL.len()))[..bytes].to_owned()
}

/// What a literal holds, told by the section spec in front of it (RFC
/// 3501 section 7.4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    /// Header fields of the message or of one part.
    Header,
    /// The text of the message or of one part.
    Body,
    /// Anything else, a mailbox name for one.
    Other,
}

/// The section a run of text introduces.
#[must_use]
pub fn section(text_before: &str) -> Section {
    let Some((_, spec)) = text_before.rsplit_once("BODY[") else {
        return Section::Other;
    };
    let Some((inside, _)) = spec.split_once(']') else {
        return Section::Other;
    };
    let word = inside.rsplit_once('.').map_or(inside, |(_, last)| last);
    if word.eq_ignore_ascii_case("MIME")
        || inside
            .get(.."HEADER".len())
            .is_some_and(|start| start.eq_ignore_ascii_case("HEADER"))
    {
        return Section::Header;
    }
    if inside.eq_ignore_ascii_case("TEXT")
        || inside
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.')
    {
        return Section::Body;
    }
    Section::Other
}

/// One field of a header block as written: the name in upper case, the
/// label as written, the value unfolded and the lines themselves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub label: String,
    pub value: String,
    pub raw: String,
}

/// A field or a line that is no field, an empty one for instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Piece {
    Field(Field),
    Other(String),
}

/// The pieces of a header block, folds joined to their field (RFC 5322
/// section 2.2.3); `join` puts them back.
#[must_use]
pub fn pieces(block: &str) -> Vec<Piece> {
    let mut pieces: Vec<Piece> = Vec::new();
    for line in block.split("\r\n") {
        if line.starts_with([' ', '\t'])
            && let Some(Piece::Field(field)) = pieces.last_mut()
        {
            field.value.push(' ');
            field.value.push_str(line.trim());
            field.raw.push_str("\r\n");
            field.raw.push_str(line);
            continue;
        }
        pieces.push(match line.split_once(':') {
            Some((label, value)) if !label.is_empty() && !label.contains(' ') => {
                Piece::Field(Field {
                    name: label.to_ascii_uppercase(),
                    label: label.to_owned(),
                    value: value.trim().to_owned(),
                    raw: line.to_owned(),
                })
            }
            _ => Piece::Other(line.to_owned()),
        });
    }
    pieces
}

/// The block the pieces make.
#[must_use]
pub fn join(pieces: &[Piece]) -> String {
    pieces
        .iter()
        .map(|piece| match piece {
            Piece::Field(field) => field.raw.as_str(),
            Piece::Other(line) => line.as_str(),
        })
        .collect::<Vec<_>>()
        .join("\r\n")
}

/// Every address in a text as the span of its bytes: a local part, `@`
/// and a domain with a dot in it.
#[must_use]
pub fn addresses(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let local = |byte: u8| byte.is_ascii_alphanumeric() || b"._%+-".contains(&byte);
    let domain = |byte: u8| byte.is_ascii_alphanumeric() || b".-".contains(&byte);
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = bytes[from..].iter().position(|byte| *byte == b'@') {
        let at = from + at;
        let mut start = at;
        while start > 0 && local(bytes[start - 1]) {
            start -= 1;
        }
        let mut end = at + 1;
        while end < bytes.len() && domain(bytes[end]) {
            end += 1;
        }
        while end > at + 1 && bytes[end - 1] == b'.' {
            end -= 1;
        }
        from = end.max(at + 1);
        if start < at && end > at + 1 && bytes[at + 1..end].contains(&b'.') {
            found.push((start, end));
        }
    }
    found
}

/// Every angle-bracketed id in a text as the span inside the brackets.
#[must_use]
pub fn angle_ids(text: &str) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(open) = text[from..].find('<') {
        let open = from + open;
        let Some(close) = text[open..].find('>') else {
            break;
        };
        found.push((open + 1, open + close));
        from = open + close + 1;
    }
    found
}

/// The two Gmail id items with the span of their digits.
#[must_use]
pub fn gmail_ids(text: &str) -> Vec<(&'static str, usize, usize)> {
    let mut found = Vec::new();
    for item in ["X-GM-MSGID ", "X-GM-THRID "] {
        let mut from = 0;
        while let Some(at) = text[from..].find(item) {
            let start = from + at + item.len();
            let end = start + text[start..].bytes().take_while(u8::is_ascii_digit).count();
            if end > start {
                found.push((item, start, end));
            }
            from = end.max(start);
        }
    }
    found.sort_unstable_by_key(|(_, start, _)| *start);
    found
}

/// An opaque token a server puts on a tagged line, a connection id for
/// one, is at least this long; a word or a number of a sentence is
/// shorter.
pub const TOKEN_MIN_CHARS: usize = 16;

/// The word that stands where a server's opaque token was.
pub const TOKEN: &str = "token";

/// Every opaque token in a run of text as the span of its bytes: a word
/// of `TOKEN_MIN_CHARS` or more made of letters, digits and hyphens
/// with a letter and a digit both in it.
#[must_use]
pub fn opaque_tokens(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let word = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'-';
    let mut found = Vec::new();
    let mut start = 0;
    while start < bytes.len() {
        if !word(bytes[start]) {
            start += 1;
            continue;
        }
        let end = start
            + bytes[start..]
                .iter()
                .take_while(|byte| word(**byte))
                .count();
        let run = &bytes[start..end];
        if run.len() >= TOKEN_MIN_CHARS
            && run.iter().any(u8::is_ascii_alphabetic)
            && run.iter().any(u8::is_ascii_digit)
        {
            found.push((start, end));
        }
        start = end;
    }
    found
}

/// Whether a server line is a tagged answer rather than untagged data
/// or a continuation.
#[must_use]
pub fn is_tagged(opening: &str) -> bool {
    !opening.starts_with(['*', '+'])
}

/// The head of a greeting (its status with its code) and the words
/// after it (RFC 3501 section 7.1).
#[must_use]
pub fn greeting_head(text: &str) -> (&str, &str) {
    let mut words = text.splitn(3, ' ');
    let (Some(star), Some(status)) = (words.next(), words.next()) else {
        return (text, "");
    };
    let head = star.len() + 1 + status.len();
    let rest = words.next().unwrap_or("");
    if rest.starts_with('[')
        && let Some(close) = rest.find(']')
    {
        let head = head + 1 + close + 1;
        return (&text[..head], text[head..].trim_start());
    }
    (&text[..head], rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_and_ids_are_found_by_shape() {
        let text = "a b@c.d, <e.f@g.h.i>. x@y z@w. <k@l.m>";
        let spans: Vec<&str> = addresses(text)
            .into_iter()
            .map(|(start, end)| &text[start..end])
            .collect();
        assert_eq!(spans, ["b@c.d", "e.f@g.h.i", "k@l.m"]);
        let ids: Vec<&str> = angle_ids(text)
            .into_iter()
            .map(|(start, end)| &text[start..end])
            .collect();
        assert_eq!(ids, ["e.f@g.h.i", "k@l.m"]);
        let items: Vec<&str> = gmail_ids("X-GM-THRID 7 X-GM-MSGID 1278455344230334865")
            .into_iter()
            .map(|(item, _, _)| item)
            .collect();
        assert_eq!(items, ["X-GM-THRID ", "X-GM-MSGID "]);
    }

    #[test]
    fn the_fixed_shapes_admit_a_corpus_and_nothing_else() {
        assert!(is_fixed_subject("Message 12") && is_fixed_subject("Re: Message 1"));
        assert!(!is_fixed_subject("Message") && !is_fixed_subject("Hello"));
        assert!(is_fixed_address("m1@example.test") && !is_fixed_address("@example.test"));
        assert!(!is_fixed_address("m1@example.test.com"));
        assert_eq!(fixed_message_id(7), "<7@example.test>");
        assert_eq!(fixed_subject(3), "Message 3");
        assert_eq!(fill(3), "Tex");
        assert_eq!(fill(BODY_FILL.len() + 1).len(), BODY_FILL.len() + 1);
    }

    #[test]
    fn a_section_spec_tells_a_header_from_a_body_rfc3501_7_4_2() {
        assert_eq!(section("BODY[TEXT]<0> "), Section::Body);
        assert_eq!(section("BODY[1.2]<0> "), Section::Body);
        assert_eq!(section("BODY[HEADER.FIELDS (SUBJECT)] "), Section::Header);
        assert_eq!(section("BODY[2.MIME]<0> "), Section::Header);
        assert_eq!(section("* LIST () \"/\" "), Section::Other);
        assert_eq!(
            section("UID 3 BODY[1.MIME]<0> x BODY[1]<0> "),
            Section::Body
        );
    }

    #[test]
    fn a_header_block_splits_into_fields_with_their_folds_and_joins_back() {
        let block = "Subject: a\r\n b\r\nX: y\r\n\r\n";
        let found = pieces(block);
        let Piece::Field(subject) = &found[0] else {
            panic!("{found:?}");
        };
        assert_eq!(
            (subject.name.as_str(), subject.value.as_str()),
            ("SUBJECT", "a b")
        );
        assert_eq!(subject.raw, "Subject: a\r\n b");
        assert_eq!(found.len(), 4);
        assert_eq!(join(&found), block);
    }

    #[test]
    fn an_opaque_token_is_a_long_word_of_letters_and_digits() {
        let text = "OK Thats all she wrote! 5b1f17b1804b1-49fbbf104a9mb2431653045e9";
        let spans: Vec<&str> = opaque_tokens(text)
            .into_iter()
            .map(|(start, end)| &text[start..end])
            .collect();
        assert_eq!(spans, ["5b1f17b1804b1-49fbbf104a9mb2431653045e9"]);
        assert!(opaque_tokens("OK [READ-ONLY] selected. (Success) A0001 authenticated").is_empty());
        assert!(opaque_tokens("00000000000000000000").is_empty());
        assert!(is_tagged("A0001 OK done") && !is_tagged("* OK ready") && !is_tagged("+ "));
    }

    #[test]
    fn a_greeting_splits_into_its_head_and_its_words_rfc3501_7_1() {
        assert_eq!(
            greeting_head("* OK Gimap ready for requests from 203.0.113.9 k4mb"),
            ("* OK", "Gimap ready for requests from 203.0.113.9 k4mb")
        );
        assert_eq!(
            greeting_head("* OK [CAPABILITY IMAP4rev1] Dovecot ready."),
            ("* OK [CAPABILITY IMAP4rev1]", "Dovecot ready.")
        );
        assert_eq!(greeting_head("* BYE"), ("* BYE", ""));
    }
}
