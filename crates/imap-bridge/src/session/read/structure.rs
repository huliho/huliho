// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! BODYSTRUCTURE into the bridge's own tree, every field a sender wrote
//! cut under a bound. The guard under the client library bounds its
//! depth.

use async_imap::imap_proto::{
    BodyContentCommon, BodyContentSinglePart, BodyStructure, ContentEncoding,
};

use crate::session::{
    BodyPart, Disposition, Leaf, MAX_PART_FIELD_BYTES, MAX_PART_FIELDS, Multipart,
};

/// The parts kept of one BODYSTRUCTURE; a message past it goes without
/// a structure.
pub(super) const MAX_BODY_PARTS: usize = 1024;

/// The bytes of fields one tree keeps: the guard's structure bound
/// applied to the literals it lets through, so the five hundred trees
/// of a sync batch stay within 32 MiB as their headers do. A message
/// past it goes without a structure.
pub const MAX_TREE_BYTES: usize = 64 * 1024;

/// The longest media type word kept (RFC 6838 section 4.2).
const MAX_MEDIA_TYPE_BYTES: usize = 127;

/// The tree; `None` past `MAX_BODY_PARTS` or `MAX_TREE_BYTES`.
pub(super) fn tree(body: &BodyStructure<'_>) -> Option<BodyPart> {
    let mut room = MAX_BODY_PARTS;
    let tree = part(body, &mut room)?;
    (kept_bytes(&tree) <= MAX_TREE_BYTES).then_some(tree)
}

/// The bytes of every field of a part and of the parts under it.
fn kept_bytes(part: &BodyPart) -> usize {
    let pairs = |parameters: &[(String, String)]| {
        parameters
            .iter()
            .map(|(name, value)| name.len() + value.len())
            .sum::<usize>()
    };
    let field = |field: &Option<String>| field.as_ref().map_or(0, String::len);
    let disposition = |disposition: &Option<Disposition>| {
        disposition
            .as_ref()
            .map_or(0, |found| found.kind.len() + pairs(&found.parameters))
    };
    let language = |language: &Option<Vec<String>>| {
        language
            .as_ref()
            .map_or(0, |tags| tags.iter().map(String::len).sum())
    };
    match part {
        BodyPart::Leaf(leaf) => {
            leaf.media_type.len()
                + leaf.subtype.len()
                + pairs(&leaf.parameters)
                + field(&leaf.id)
                + field(&leaf.description)
                + leaf.encoding.len()
                + disposition(&leaf.disposition)
                + language(&leaf.language)
                + field(&leaf.location)
        }
        BodyPart::Multipart(multipart) => {
            multipart.subtype.len()
                + pairs(&multipart.parameters)
                + disposition(&multipart.disposition)
                + language(&multipart.language)
                + field(&multipart.location)
                + multipart.parts.iter().map(kept_bytes).sum::<usize>()
        }
    }
}

fn part(body: &BodyStructure<'_>, room: &mut usize) -> Option<BodyPart> {
    *room = room.checked_sub(1)?;
    Some(match body {
        BodyStructure::Multipart { common, bodies, .. } => BodyPart::Multipart(Multipart {
            subtype: word(&common.ty.subtype),
            parameters: parameters(common.ty.params.as_deref()),
            disposition: disposition(common),
            language: language(common),
            location: common.location.as_deref().map(cut),
            parts: bodies
                .iter()
                .map(|body| part(body, room))
                .collect::<Option<_>>()?,
        }),
        BodyStructure::Basic { common, other, .. } => leaf(common, other, None),
        BodyStructure::Text {
            common,
            other,
            lines,
            ..
        }
        | BodyStructure::Message {
            common,
            other,
            lines,
            ..
        } => leaf(common, other, Some(*lines)),
    })
}

fn leaf(
    common: &BodyContentCommon<'_>,
    other: &BodyContentSinglePart<'_>,
    lines: Option<u32>,
) -> BodyPart {
    let encoding = match &other.transfer_encoding {
        ContentEncoding::SevenBit => "7bit".to_owned(),
        ContentEncoding::EightBit => "8bit".to_owned(),
        ContentEncoding::Binary => "binary".to_owned(),
        ContentEncoding::Base64 => "base64".to_owned(),
        ContentEncoding::QuotedPrintable => "quoted-printable".to_owned(),
        ContentEncoding::Other(name) => word(name),
    };
    BodyPart::Leaf(Leaf {
        media_type: word(&common.ty.ty),
        subtype: word(&common.ty.subtype),
        parameters: parameters(common.ty.params.as_deref()),
        id: other.id.as_deref().map(cut),
        description: other.description.as_deref().map(cut),
        encoding,
        bytes: other.octets,
        lines,
        disposition: disposition(common),
        language: language(common),
        location: common.location.as_deref().map(cut),
    })
}

fn disposition(common: &BodyContentCommon<'_>) -> Option<Disposition> {
    common.disposition.as_ref().map(|disposition| Disposition {
        kind: word(&disposition.ty),
        parameters: parameters(disposition.params.as_deref()),
    })
}

fn language(common: &BodyContentCommon<'_>) -> Option<Vec<String>> {
    common.language.as_ref().map(|tags| {
        tags.iter()
            .take(MAX_PART_FIELDS)
            .map(|tag| cut(tag))
            .collect()
    })
}

/// The first `MAX_PART_FIELDS` parameters, each value cut, the names
/// as written since a name may carry an RFC 2231 suffix.
fn parameters<T: AsRef<str>>(params: Option<&[(T, T)]>) -> Vec<(String, String)> {
    params
        .into_iter()
        .flatten()
        .take(MAX_PART_FIELDS)
        .map(|(name, value)| (cut(name.as_ref()), cut(value.as_ref())))
        .collect()
}

/// A media type word in lower case, cut at `MAX_MEDIA_TYPE_BYTES` on a
/// character border.
fn word(text: &str) -> String {
    cut_at(text, MAX_MEDIA_TYPE_BYTES).to_ascii_lowercase()
}

/// A field as written, cut at `MAX_PART_FIELD_BYTES` on a character
/// border.
fn cut(text: &str) -> String {
    cut_at(text, MAX_PART_FIELD_BYTES)
}

/// The text cut at `bytes` on a character border, without its control
/// characters: a field goes back on a header line for the decoder, so
/// a line break inside one must never end that line.
fn cut_at(text: &str, bytes: usize) -> String {
    let mut end = text.len().min(bytes);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].chars().filter(|c| !c.is_control()).collect()
}

#[cfg(test)]
mod tests {
    use async_imap::imap_proto::{AttributeValue, Response};

    use super::*;

    fn parsed(structure: &str) -> Option<BodyPart> {
        let line = format!("* 1 FETCH (UID 1 BODYSTRUCTURE {structure})\r\n");
        match Response::from_bytes(line.as_bytes()).unwrap().1 {
            Response::Fetch(_, attributes) => attributes.iter().find_map(|item| match item {
                AttributeValue::BodyStructure(body) => Some(tree(body)),
                _ => None,
            }),
            other => panic!("{other:?}"),
        }
        .flatten()
    }

    #[test]
    fn every_field_of_a_part_reads_in_lower_case_words_rfc3501_7_4_2() {
        let html = "(\"TEXT\" \"HTML\" (\"CHARSET\" \"UTF-8\") \"<part1@x>\" \"the body\" \"QUOTED-PRINTABLE\" 120 3 NIL (\"INLINE\" NIL) (\"en\" \"nl\") \"http://x.test/b\")";
        let image = "(\"IMAGE\" \"PNG\" (\"NAME\" \"a.png\") \"<logo@x>\" NIL \"BASE64\" 900 NIL (\"ATTACHMENT\" (\"FILENAME\" \"a.png\")) NIL NIL)";
        let structure = format!("({html}{image} \"RELATED\" (\"TYPE\" \"text/html\") NIL NIL NIL)");
        let BodyPart::Multipart(multipart) = parsed(&structure).unwrap() else {
            panic!("a multipart")
        };
        assert_eq!(multipart.subtype, "related");
        assert_eq!(
            multipart.parameters,
            [("TYPE".to_owned(), "text/html".to_owned())]
        );
        let [BodyPart::Leaf(html), BodyPart::Leaf(image)] = multipart.parts.as_slice() else {
            panic!("two leaves")
        };
        assert_eq!(
            (html.media_type.as_str(), html.subtype.as_str()),
            ("text", "html")
        );
        assert_eq!(
            html.parameters,
            [("CHARSET".to_owned(), "UTF-8".to_owned())]
        );
        assert_eq!(html.id.as_deref(), Some("<part1@x>"));
        assert_eq!(html.description.as_deref(), Some("the body"));
        assert_eq!(html.encoding, "quoted-printable");
        assert_eq!((html.bytes, html.lines), (120, Some(3)));
        assert_eq!(html.disposition.as_ref().unwrap().kind, "inline");
        assert!(!html.is_attachment());
        assert!(html.is_text_body());
        assert_eq!(
            html.language.as_deref(),
            Some(&["en".to_owned(), "nl".to_owned()][..])
        );
        assert_eq!(html.location.as_deref(), Some("http://x.test/b"));
        assert!(image.is_attachment());
        assert_eq!(
            image.disposition.as_ref().unwrap().parameters,
            [("FILENAME".to_owned(), "a.png".to_owned())]
        );
        assert_eq!(image.lines, None);
        assert!(!image.is_text_body());
    }

    #[test]
    fn a_message_inside_a_message_is_a_leaf_rfc8621_4_1_4() {
        let inner = "(\"TEXT\" \"PLAIN\" NIL NIL NIL \"7BIT\" 5 1)";
        let envelope = "(NIL \"inner\" NIL NIL NIL NIL NIL NIL NIL NIL)";
        let message =
            format!("(\"MESSAGE\" \"RFC822\" NIL NIL NIL \"7BIT\" 200 {envelope} {inner} 9)");
        let BodyPart::Leaf(leaf) = parsed(&message).unwrap() else {
            panic!("a leaf")
        };
        assert_eq!(
            (leaf.media_type.as_str(), leaf.subtype.as_str()),
            ("message", "rfc822")
        );
        assert_eq!((leaf.bytes, leaf.lines), (200, Some(9)));
    }

    #[test]
    fn an_unknown_encoding_keeps_its_word_and_long_fields_are_cut() {
        let long = "x".repeat(MAX_PART_FIELD_BYTES + 5);
        let structure = format!(
            "(\"APPLICATION\" \"OCTET-STREAM\" (\"NAME\" \"{long}\") NIL \"{long}\" \"X-UUENCODE\" 7 NIL NIL NIL \"{long}\")"
        );
        let BodyPart::Leaf(leaf) = parsed(&structure).unwrap() else {
            panic!("a leaf")
        };
        assert_eq!(leaf.encoding, "x-uuencode");
        assert_eq!(leaf.parameters[0].1.len(), MAX_PART_FIELD_BYTES);
        assert_eq!(
            leaf.description.as_ref().unwrap().len(),
            MAX_PART_FIELD_BYTES
        );
        assert_eq!(leaf.location.as_ref().unwrap().len(), MAX_PART_FIELD_BYTES);
        let cut = word(&"\u{e9}".repeat(MAX_MEDIA_TYPE_BYTES));
        assert_eq!(cut.len(), MAX_MEDIA_TYPE_BYTES - 1);
        assert_eq!(word("TEXT"), "text");
    }

    #[test]
    fn a_line_break_inside_a_literal_field_never_reaches_the_field() {
        let broken = "(\"TEXT\" \"PLAIN\" ({9}\r\nCHAR\r\nSET {8}\r\nutf\r\n-8x) {5}\r\na\r\n@b NIL \"7BIT\" 1 1)";
        let BodyPart::Leaf(leaf) = parsed(broken).unwrap() else {
            panic!("a leaf")
        };
        assert_eq!(
            leaf.parameters,
            [("CHARSET".to_owned(), "utf-8x".to_owned())]
        );
        assert_eq!(leaf.id.as_deref(), Some("a@b"));
    }

    #[test]
    fn a_structure_whose_fields_pass_the_tree_bound_is_dropped_whole() {
        let value = "v".repeat(MAX_PART_FIELD_BYTES);
        let structure = |fields: usize| {
            let list: Vec<String> = (0..fields)
                .map(|index| format!("\"P{index}\" \"{value}\""))
                .collect();
            format!(
                "(\"TEXT\" \"PLAIN\" ({}) NIL NIL \"7BIT\" 1 1)",
                list.join(" ")
            )
        };
        let under = MAX_TREE_BYTES / MAX_PART_FIELD_BYTES - 1;
        assert!(parsed(&structure(under)).is_some());
        assert_eq!(parsed(&structure(under + 2)), None);
    }

    #[test]
    fn a_structure_past_the_part_limit_is_dropped_whole() {
        let leaf = "(\"TEXT\" \"PLAIN\" NIL NIL NIL \"7BIT\" 1 1)";
        let structure = |leaves: usize| format!("({} \"MIXED\")", leaf.repeat(leaves));
        assert!(parsed(&structure(MAX_BODY_PARTS - 1)).is_some());
        assert_eq!(parsed(&structure(MAX_BODY_PARTS)), None);
    }
}
