// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The preview of an email (RFC 8621 section 4.1.4): which part of the
//! body it is read from and how the start of that part becomes a line
//! of text. No connection is involved; whoever sent the mail wrote
//! these bytes.

use mail_parser::MessageParser;

use crate::session::BodyPart;
use crate::store::PreviewPart;

/// The longest preview (RFC 8621 section 4.1.4).
pub const PREVIEW_CHARS: usize = 256;

/// What a plain part is asked for: a preview of `PREVIEW_CHARS` fits
/// many times, whatever the charset and the transfer encoding.
pub const PREVIEW_PLAIN_FETCH_BYTES: u32 = 2048;

/// An HTML part below this size is asked for in full, since its text
/// may start behind a head and its styles.
pub const PREVIEW_HTML_PART_BYTES: u32 = 64 * 1024;

/// What a larger HTML part is asked for.
pub const PREVIEW_HTML_FETCH_BYTES: u32 = 16 * 1024;

/// The part a preview is read from: the first plain text leaf, else the
/// first HTML leaf, a part sent as an attachment never. A message inside
/// a message is a leaf, so its text is never taken for the outer one.
#[must_use]
pub fn part(structure: &BodyPart) -> Option<PreviewPart> {
    let mut leaves = Vec::new();
    walk(structure, &mut Vec::new(), &mut leaves);
    let first = |html: bool| leaves.iter().find(|leaf| leaf.html == html).cloned();
    first(false).or_else(|| first(true))
}

/// The text leaves in the order of the message, each under its part
/// number (RFC 3501 section 6.4.5).
fn walk(part: &BodyPart, number: &mut Vec<usize>, leaves: &mut Vec<PreviewPart>) {
    match part {
        BodyPart::Multipart { parts, .. } => {
            for (index, child) in parts.iter().enumerate() {
                number.push(index + 1);
                walk(child, number, leaves);
                number.pop();
            }
        }
        BodyPart::Leaf {
            media_type,
            subtype,
            attachment: false,
            bytes,
        } if media_type == "text" && (subtype == "plain" || subtype == "html") => {
            let levels: Vec<String> = number.iter().map(usize::to_string).collect();
            leaves.push(PreviewPart {
                path: levels.join("."),
                html: subtype == "html",
                bytes: *bytes,
            });
        }
        BodyPart::Leaf { .. } => {}
    }
}

/// How much of the part to ask for.
#[must_use]
pub fn fetch_bytes(part: &PreviewPart) -> u32 {
    match (part.html, part.bytes < PREVIEW_HTML_PART_BYTES) {
        (false, _) => PREVIEW_PLAIN_FETCH_BYTES,
        (true, true) => PREVIEW_HTML_PART_BYTES,
        (true, false) => PREVIEW_HTML_FETCH_BYTES,
    }
}

/// The preview from the header of a part and the start of its text:
/// decoded by its transfer encoding and its charset, HTML turned into
/// text, runs of white space folded into one space, `PREVIEW_CHARS`
/// characters at most. Bytes that decode to nothing give an empty line.
#[must_use]
pub fn text(header: &[u8], start: &[u8]) -> String {
    let header = header.trim_ascii_end();
    let mut message = Vec::with_capacity(header.len() + start.len() + 4);
    message.extend_from_slice(header);
    message.extend_from_slice(b"\r\n\r\n");
    message.extend_from_slice(start);
    let Some(parsed) = MessageParser::default().parse(&message) else {
        return String::new();
    };
    let body = parsed.body_text(0).unwrap_or_default();
    let mut preview = String::new();
    for word in body.split_whitespace() {
        if !preview.is_empty() {
            preview.push(' ');
        }
        preview.push_str(word);
        if preview.chars().count() >= PREVIEW_CHARS {
            break;
        }
    }
    preview.chars().take(PREVIEW_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(subtype: &str, attachment: bool, bytes: u32) -> BodyPart {
        BodyPart::Leaf {
            media_type: "text".to_owned(),
            subtype: subtype.to_owned(),
            attachment,
            bytes,
        }
    }

    fn multipart(subtype: &str, parts: Vec<BodyPart>) -> BodyPart {
        BodyPart::Multipart {
            subtype: subtype.to_owned(),
            parts,
        }
    }

    #[test]
    fn the_first_plain_leaf_wins_then_the_first_html_one_rfc3501_6_4_5() {
        let lone = part(&leaf("plain", false, 10)).unwrap();
        assert_eq!((lone.path.as_str(), lone.html), ("", false));
        let alternative = multipart(
            "mixed",
            vec![
                multipart(
                    "alternative",
                    vec![leaf("html", false, 900), leaf("plain", false, 300)],
                ),
                leaf("plain", true, 50),
            ],
        );
        let chosen = part(&alternative).unwrap();
        assert_eq!((chosen.path.as_str(), chosen.bytes), ("1.2", 300));
        let html_only = multipart(
            "mixed",
            vec![leaf("plain", true, 5), leaf("html", false, 7)],
        );
        let chosen = part(&html_only).unwrap();
        assert_eq!((chosen.path.as_str(), chosen.html), ("2", true));
        assert_eq!(
            part(&multipart("mixed", vec![leaf("calendar", false, 5)])),
            None
        );
    }

    #[test]
    fn the_ask_follows_the_kind_and_the_size_of_the_part() {
        let sized = |html, bytes| PreviewPart {
            path: "1".to_owned(),
            html,
            bytes,
        };
        assert_eq!(
            fetch_bytes(&sized(false, u32::MAX)),
            PREVIEW_PLAIN_FETCH_BYTES
        );
        assert_eq!(
            fetch_bytes(&sized(true, PREVIEW_HTML_PART_BYTES - 1)),
            PREVIEW_HTML_PART_BYTES
        );
        assert_eq!(
            fetch_bytes(&sized(true, PREVIEW_HTML_PART_BYTES)),
            PREVIEW_HTML_FETCH_BYTES
        );
    }

    #[test]
    fn a_preview_decodes_its_transfer_encoding_and_folds_white_space() {
        let quoted = text(
            b"Content-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\n",
            b"Caf=C3=A9 om drie\r\n   uur?=\r\n Tot dan.",
        );
        assert_eq!(quoted, "Caf\u{e9} om drie uur? Tot dan.");
        let base64 = text(
            b"Content-Type: text/plain; charset=iso-8859-1\r\nContent-Transfer-Encoding: base64",
            b"Q2Fm6SBvbSBkcmll",
        );
        assert_eq!(base64, "Caf\u{e9} om drie");
    }

    #[test]
    fn html_gives_its_text_and_a_long_body_is_cut_at_the_character_limit() {
        let html = text(
            b"Content-Type: text/html; charset=utf-8\r\n\r\n",
            b"<html><head><style>p { color: red }</style></head><body><p>Hello <b>there</b></p></body></html>",
        );
        assert_eq!(html, "Hello there");
        let long = text(
            b"Content-Type: text/plain\r\n\r\n",
            "\u{e9}a ".repeat(400).as_bytes(),
        );
        assert_eq!(long.chars().count(), PREVIEW_CHARS);
        assert!(long.starts_with("\u{e9}a \u{e9}a"));
    }

    #[test]
    fn bytes_that_hold_no_text_give_an_empty_preview() {
        assert_eq!(text(b"", b""), "");
        assert_eq!(
            text(b"\xff\xfe", b"\x00\x01"),
            text(b"\xff\xfe", b"\x00\x01")
        );
        assert_eq!(text(b"Content-Type: text/plain", b"   \r\n \t "), "");
    }
}
