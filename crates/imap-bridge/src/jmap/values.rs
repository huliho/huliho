// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A body value (RFC 8621 section 4.1.4): the bytes of a text part,
//! fetched in windows, decoded by their transfer encoding and charset,
//! line endings folded to LF and cut at the cap on a character border.
//! Bytes the fetch stopped short of the part end on a border of their
//! encoding, and a character the stop split is left out. No connection
//! is involved; whoever sent the mail wrote these bytes.

use std::fmt::Write as _;

use mail_parser::MessageParser;
use mail_parser::decoders::charsets::map::charset_decoder;
use serde_json::{Value, json};

use crate::session::Leaf;

/// The most a value may hold, whatever the client asks; a larger part
/// is cut and marked truncated.
pub const MAX_BODY_VALUE_BYTES: u32 = 8 * 1024 * 1024;

/// The part windows one request fetches at most, 16 MiB on the wire,
/// what the proxy admits of a native server's answer; a value the
/// budget does not reach is empty and marked truncated.
pub const MAX_BODY_WINDOWS: usize = 32;

/// The charset a text part has without a parameter (RFC 2045 section
/// 5.2), which needs no decoder.
const DEFAULT_CHARSET: &str = "us-ascii";

/// The charset names mail-parser decodes without a table entry.
const PLAIN_CHARSETS: [&str; 4] = ["utf-8", "utf8", "us-ascii", "ascii"];

/// The transfer encodings the decoder knows (RFC 2045 section 6).
const KNOWN_ENCODINGS: [&str; 5] = ["7bit", "8bit", "binary", "base64", "quoted-printable"];

/// What a decoder writes for bytes it cannot read.
const REPLACEMENT: char = '\u{fffd}';

/// The octet an ISO-2022 escape sequence opens with (RFC 1468).
const ESCAPE: u8 = 0x1b;

/// The digits every name of an ISO-2022 charset carries.
const ISO_2022: &str = "2022";

/// The longest unfinished escape a cut can leave: `=X` or `ESC $`.
const CUT_ESCAPE_BYTES: usize = 2;

/// Which values a call wants and how much of each.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ValueAsk {
    /// The text parts of `textBody`.
    pub text: bool,
    /// The text parts of `htmlBody`.
    pub html: bool,
    /// Every text part of the structure.
    pub all: bool,
    /// The octets one value may hold.
    pub cap: u32,
}

impl ValueAsk {
    pub(super) fn wants_any(self) -> bool {
        self.text || self.html || self.all
    }
}

/// The cap on one value: the client's `maxBodyValueBytes` where it is
/// above zero, the bridge's own bound where that is lower or the ask is
/// unbounded.
pub(super) fn cap(max_body_value_bytes: u64) -> u32 {
    let asked = u32::try_from(max_body_value_bytes).unwrap_or(u32::MAX);
    if asked == 0 {
        MAX_BODY_VALUE_BYTES
    } else {
        asked.min(MAX_BODY_VALUE_BYTES)
    }
}

/// One `EmailBodyValue`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct BodyValue {
    pub value: String,
    pub is_encoding_problem: bool,
    pub is_truncated: bool,
}

impl BodyValue {
    pub(super) fn render(&self) -> Value {
        json!({
            "value": self.value,
            "isEncodingProblem": self.is_encoding_problem,
            "isTruncated": self.is_truncated,
        })
    }
}

/// The charset parameter of a text part, else the MIME default; `None`
/// for a part that is not text.
pub(super) fn charset(leaf: &Leaf) -> Option<String> {
    if leaf.media_type != "text" {
        return None;
    }
    Some(parameter(&leaf.parameters, "charset").unwrap_or_else(|| DEFAULT_CHARSET.to_owned()))
}

/// The value of one parameter, its name matched without regard to case.
pub(super) fn parameter(parameters: &[(String, String)], name: &str) -> Option<String> {
    parameters
        .iter()
        .find(|(found, _)| found.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.clone())
}

/// The bytes fetched of a text part, decoded as mail-parser decodes a
/// message of that one part. `whole` says the bytes are the whole part;
/// otherwise they stop where the fetch did, and what that stop split is
/// the fetch's doing rather than the sender's.
pub(super) fn decode(leaf: &Leaf, bytes: &[u8], cap: u32, whole: bool) -> BodyValue {
    let bytes = &bytes[..border(leaf, bytes, whole)];
    let mut message = mime_header(leaf);
    message.extend_from_slice(bytes);
    let parsed = MessageParser::default().parse(&message);
    let part = parsed.as_ref().and_then(|parsed| parsed.parts.first());
    let (mut text, mut problem) = match part {
        Some(part) => (
            part.text_contents().map_or_else(
                || String::from_utf8_lossy(part.contents()).into_owned(),
                str::to_owned,
            ),
            part.is_encoding_problem,
        ),
        None => (String::new(), !bytes.is_empty()),
    };
    if !whole {
        // A character or a line ending cut in two decodes to one
        // replacement character or one bare CR at the tail.
        for tail in [REPLACEMENT, '\r'] {
            if text.ends_with(tail) {
                text.pop();
            }
        }
    }
    problem |= unknown_charset(leaf) || unknown_encoding(leaf) || text.contains(REPLACEMENT);
    let mut value = text.replace("\r\n", "\n");
    let cut = truncate(
        &mut value,
        usize::try_from(cap).unwrap_or(usize::MAX),
        leaf.subtype == "html",
    );
    BodyValue {
        value,
        is_encoding_problem: problem,
        is_truncated: !whole || cut,
    }
}

/// The MIME header the structure describes, so the decoder reads the
/// bytes as the sender's own header would have it read them.
fn mime_header(leaf: &Leaf) -> Vec<u8> {
    let mut header = format!("Content-Type: {}/{}", leaf.media_type, leaf.subtype);
    if let Some(charset) = charset(leaf) {
        let _ = write!(header, "; charset=\"{}\"", quoted(&charset));
    }
    let _ = write!(
        header,
        "\r\nContent-Transfer-Encoding: {}\r\n\r\n",
        leaf.encoding
    );
    header.into_bytes()
}

/// A parameter value inside quotes (RFC 2045 section 5.1).
fn quoted(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn unknown_charset(leaf: &Leaf) -> bool {
    charset(leaf).is_some_and(|charset| {
        !PLAIN_CHARSETS
            .iter()
            .any(|plain| plain.eq_ignore_ascii_case(&charset))
            && charset_decoder(charset.as_bytes()).is_none()
    })
}

fn unknown_encoding(leaf: &Leaf) -> bool {
    !KNOWN_ENCODINGS.contains(&leaf.encoding.as_str())
}

/// Where the fetched bytes end: at their end for the whole part, else
/// on a border of their encoding: after the last whole base64 quartet,
/// before a quoted-printable escape or an ISO-2022 escape sequence the
/// fetch cut. Nothing past the border decodes to bytes the sender wrote.
fn border(leaf: &Leaf, bytes: &[u8], whole: bool) -> usize {
    let introducer = match leaf.encoding.as_str() {
        _ if whole => return bytes.len(),
        "base64" => return quartets(bytes),
        "quoted-printable" => b'=',
        _ if charset(leaf).is_some_and(|name| name.contains(ISO_2022)) => ESCAPE,
        _ => return bytes.len(),
    };
    let tail = bytes.len().saturating_sub(CUT_ESCAPE_BYTES);
    bytes[tail..]
        .iter()
        .position(|byte| *byte == introducer)
        .map_or(bytes.len(), |at| tail + at)
}

/// The end of the last whole quartet of base64 symbols; the line
/// breaks between symbols do not count.
fn quartets(bytes: &[u8]) -> usize {
    let symbol = |byte: &u8| byte.is_ascii_alphanumeric() || b"+/".contains(byte);
    let mut extra = bytes.iter().filter(|byte| symbol(byte)).count() % 4;
    let mut end = bytes.len();
    while extra > 0 {
        end -= 1;
        extra -= usize::from(symbol(&bytes[end]));
    }
    end
}

/// Cuts the value at `cap` octets on a character border; an HTML value
/// is cut before a tag it would otherwise split. Whether it was cut.
fn truncate(value: &mut String, cap: usize, html: bool) -> bool {
    if value.len() <= cap {
        return false;
    }
    let mut end = cap;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    if html
        && let Some(open) = value[..end].rfind('<')
        && !value[open..end].contains('>')
    {
        end = open;
    }
    value.truncate(end);
    true
}

#[cfg(test)]
mod tests {
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD as BASE64;
    use proptest::prelude::*;

    use super::*;

    fn text(subtype: &str, charset: Option<&str>, encoding: &str, bytes: u32) -> Leaf {
        Leaf {
            media_type: "text".to_owned(),
            subtype: subtype.to_owned(),
            parameters: charset
                .map(|charset| vec![("CHARSET".to_owned(), charset.to_owned())])
                .unwrap_or_default(),
            encoding: encoding.to_owned(),
            bytes,
            ..Leaf::default()
        }
    }

    /// A plain part of `bytes` decoded under a cap above every test body.
    fn decoded(charset: Option<&str>, encoding: &str, bytes: &[u8], whole: bool) -> BodyValue {
        let size = u32::try_from(bytes.len()).unwrap();
        decode(&text("plain", charset, encoding, size), bytes, 1024, whole)
    }

    #[test]
    fn the_cap_is_the_ask_under_the_bound_and_the_bound_otherwise_rfc8621_4_2() {
        assert_eq!(cap(0), MAX_BODY_VALUE_BYTES);
        assert_eq!(cap(4096), 4096);
        assert_eq!(
            cap(u64::from(MAX_BODY_VALUE_BYTES) + 1),
            MAX_BODY_VALUE_BYTES
        );
        assert_eq!(cap(u64::MAX), MAX_BODY_VALUE_BYTES);
    }

    #[test]
    fn a_value_decodes_its_encoding_and_charset_and_folds_line_endings_rfc8621_4_1_4() {
        let folded = b"Caf=C3=A9 om drie\r\nuur.";
        let quoted = decoded(Some("utf-8"), "quoted-printable", folded, true);
        assert_eq!(
            quoted,
            BodyValue {
                value: "Caf\u{e9} om drie\nuur.".to_owned(),
                is_encoding_problem: false,
                is_truncated: false,
            }
        );
        let latin = decoded(Some("ISO-8859-1"), "base64", b"Q2Fm6SBvbSBkcmll", true);
        assert_eq!(latin.value, "Caf\u{e9} om drie");
        assert!(!latin.is_encoding_problem);
        let plain = decode(&text("html", None, "7bit", 9), b"<p>hi</p>", 1024, true);
        assert_eq!(plain.value, "<p>hi</p>");
        assert_eq!(
            charset(&text("plain", None, "7bit", 0)).as_deref(),
            Some(DEFAULT_CHARSET)
        );
        assert_eq!(
            charset(&Leaf {
                media_type: "image".to_owned(),
                ..Leaf::default()
            }),
            None
        );
    }

    #[test]
    fn a_broken_charset_an_unknown_one_or_an_unknown_encoding_is_an_encoding_problem() {
        let broken = decoded(Some("utf-8"), "8bit", b"a\xffb", true);
        assert!(broken.is_encoding_problem);
        assert_eq!(broken.value, "a\u{fffd}b");
        let unknown = decoded(Some("x-bogus"), "7bit", b"ab", true);
        assert!(unknown.is_encoding_problem);
        assert_eq!(unknown.value, "ab");
        assert!(decoded(Some("utf-8"), "x-uuencode", b"ab", true).is_encoding_problem);
        assert!(!decoded(Some("UTF-8"), "7bit", b"ab", true).is_encoding_problem);
    }

    #[test]
    fn a_value_is_cut_at_the_cap_on_a_character_border_and_before_a_split_tag() {
        let cut = decode(
            &text("plain", Some("utf-8"), "8bit", 8),
            "\u{e9}\u{e9}\u{e9}\u{e9}".as_bytes(),
            5,
            true,
        );
        assert_eq!(cut.value, "\u{e9}\u{e9}");
        assert!(cut.is_truncated);
        let html = decode(
            &text("html", None, "7bit", 20),
            b"<p>hi</p><a href=x>",
            14,
            true,
        );
        assert_eq!(html.value, "<p>hi</p>");
        assert!(html.is_truncated);
        let mut short = "abc".to_owned();
        assert!(!truncate(&mut short, 3, true));
    }

    #[test]
    fn a_value_the_fetch_stopped_short_ends_readable_and_is_no_encoding_problem_rfc8621_4_1_4() {
        let three = "\u{e9}\u{e9}\u{e9}";
        let encoded = BASE64.encode(three);
        let cases: [(&str, &str, &[u8], &str); 11] = [
            ("utf-8", "8bit", &three.as_bytes()[..5], "\u{e9}\u{e9}"),
            ("utf-8", "8bit", b"lines\r", "lines"),
            ("shift_jis", "8bit", b"\x93\xfa\x96", "\u{65e5}"),
            ("iso-2022-jp", "7bit", b"\x1b$BF|\x1b(", "\u{65e5}"),
            ("csISO2022JP", "7bit", b"\x1b$", ""),
            ("utf-8", "base64", &encoded.as_bytes()[..6], "\u{e9}"),
            ("utf-8", "quoted-printable", b"Caf=C3=A9=C3", "Caf\u{e9}"),
            ("utf-8", "quoted-printable", b"Caf=C3=A9=", "Caf\u{e9}"),
            ("utf-8", "quoted-printable", b"Caf=C3=A9=C", "Caf\u{e9}"),
            ("us-ascii", "7bit", b"first window", "first window"),
            ("us-ascii", "7bit", b"", ""),
        ];
        for (charset, encoding, bytes, expected) in cases {
            let value = decoded(Some(charset), encoding, bytes, false);
            assert_eq!(value.value, expected, "{encoding} {bytes:?}");
            assert!(!value.is_encoding_problem, "{encoding} {bytes:?}");
            assert!(value.is_truncated && value.render()["isTruncated"] == true);
        }
        let broken = decoded(Some("utf-8"), "8bit", b"a\xffb\xc3", false);
        assert_eq!(broken.value, "a\u{fffd}b");
        assert!(broken.is_encoding_problem && broken.is_truncated);
        let broken_lines = text("plain", None, "base64", 8);
        assert_eq!(border(&broken_lines, b"w6nD\r\nqc", false), 6);
        assert_eq!(border(&broken_lines, b"w6nD\r\nqc", true), 8);
    }

    #[test]
    fn empty_bytes_give_an_empty_value_and_a_quoted_charset_stays_inside_its_quotes() {
        let empty = decoded(Some("utf-8"), "7bit", b"", true);
        assert_eq!(
            empty,
            BodyValue {
                value: String::new(),
                is_encoding_problem: false,
                is_truncated: false
            }
        );
        let odd = decoded(Some("utf-8\"; x=\"y"), "7bit", b"ab", true);
        assert_eq!(odd.value, "ab");
        assert!(
            mime_header(&text("plain", Some("a\"b"), "7bit", 0))
                .starts_with(b"Content-Type: text/plain; charset=\"a\\\"b\"\r\n")
        );
    }

    proptest! {
        #[test]
        fn a_prefix_of_the_encoded_bytes_decodes_to_a_prefix_of_the_text(
            words in prop::collection::vec("[a-zA-Z\u{e9}\u{fc}\u{4e2d} ]{1,12}", 1..40),
            encoding in prop::sample::select(vec!["7bit", "8bit", "base64", "quoted-printable"]),
            cap in 1u32..4096,
            cut in 0usize..4096,
        ) {
            let plain = words.join("\n");
            let encoded: Vec<u8> = match encoding {
                "base64" => BASE64.encode(plain.as_bytes()).into_bytes(),
                "quoted-printable" => quoted_printable(plain.as_bytes()),
                _ => plain.as_bytes().to_vec(),
            };
            let leaf = text("plain", Some("utf-8"), encoding, u32::try_from(encoded.len()).unwrap());
            let whole = decode(&leaf, &encoded, u32::MAX, true);
            prop_assert_eq!(&whole.value, &plain);
            prop_assert!(!whole.is_truncated && !whole.is_encoding_problem);
            let capped = decode(&leaf, &encoded, cap, true);
            prop_assert!(plain.starts_with(&capped.value));
            prop_assert!(capped.value.len() <= usize::try_from(cap).unwrap());
            prop_assert_eq!(capped.is_truncated, capped.value.len() < plain.len());
            let cut = cut.min(encoded.len());
            let prefix = decode(&leaf, &encoded[..cut], u32::MAX, cut == encoded.len());
            prop_assert!(plain.starts_with(&prefix.value), "{:?}", prefix.value);
            prop_assert!(!prefix.is_encoding_problem);
            prop_assert_eq!(prefix.is_truncated, cut < encoded.len());
        }
    }

    /// Quoted-printable of RFC 2045 section 6.7 over bytes: every space
    /// escaped, since a decoder drops one before a line break.
    fn quoted_printable(bytes: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for byte in bytes {
            match byte {
                b'\n' => out.extend_from_slice(b"\r\n"),
                33..=60 | 62..=126 => out.push(*byte),
                _ => out.extend_from_slice(format!("={byte:02X}").as_bytes()),
            }
        }
        out
    }
}
