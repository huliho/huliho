// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The body of an Email as RFC 8621 section 4.1.4 has it: the tree of
//! `EmailBodyPart` objects over the structure IMAP described, the three
//! part lists and the blob id of every part. No connection is involved.

mod lists;
mod names;

use serde_json::{Map, Value, json};

use super::MethodError;
use super::values::charset;
use crate::blob::part_blob_id;
use crate::session::{BodyPart, Disposition, Leaf, Multipart};

pub(super) use lists::{Node, leaves, lists};
use names::cid;
pub(super) use names::name;

/// The emails one `Email/get` may read from the server, whatever it
/// asks of them; the client asks one message per request and each
/// costs a fetch and its windows on the conversation under one
/// deadline.
pub const MAX_BODIES_IN_GET: usize = 4;

/// The properties of a part served when `bodyProperties` is omitted
/// (RFC 8621 section 4.2).
const DEFAULT_BODY_PROPERTIES: [&str; 10] = [
    "partId",
    "blobId",
    "size",
    "name",
    "type",
    "charset",
    "disposition",
    "cid",
    "language",
    "location",
];

/// Every property of a part the bridge serves; `headers` and the
/// `header:` forms of a part are not among them.
const BODY_PROPERTIES: [&str; 11] = [
    "partId",
    "blobId",
    "size",
    "name",
    "type",
    "charset",
    "disposition",
    "cid",
    "language",
    "location",
    "subParts",
];

/// The part number of the one part of a message that is one part.
const LONE_PART: &str = "1";

/// The section a fetch names for that part (RFC 3501 section 6.4.5).
const LONE_SECTION: &str = "TEXT";

/// The name of the one attachment a message the bridge cannot describe
/// shows: the message itself.
const WHOLE_MESSAGE_NAME: &str = "message.eml";

/// The part properties to render: the default set when none are named,
/// the named ones otherwise; one the bridge does not serve is
/// `invalidArguments`.
pub(super) fn body_properties(named: Option<&[String]>) -> Result<Vec<&'static str>, MethodError> {
    let Some(named) = named else {
        return Ok(DEFAULT_BODY_PROPERTIES.to_vec());
    };
    let mut wanted = Vec::new();
    for name in named {
        let property = BODY_PROPERTIES
            .iter()
            .copied()
            .find(|known| known == name)
            .ok_or(MethodError::InvalidArguments(
                "a body property is not served",
            ))?;
        if !wanted.contains(&property) {
            wanted.push(property);
        }
    }
    Ok(wanted)
}

/// The section a fetch names for a part: `TEXT` for the one part of a
/// message that is one part, the part number otherwise.
pub(super) fn fetch_section(root: &BodyPart, part_id: &str) -> String {
    match root {
        BodyPart::Leaf(_) if part_id == LONE_PART => LONE_SECTION.to_owned(),
        _ => part_id.to_owned(),
    }
}

/// The leaf a part number names, with the section a fetch reads it by;
/// `None` for a number the tree lacks and for one that names a
/// multipart, which has no content of its own.
pub(crate) fn leaf_of<'a>(root: &'a BodyPart, part_id: &str) -> Option<(&'a Leaf, String)> {
    leaves(root)
        .into_iter()
        .find(|node| node.part_id == part_id)
        .map(|node| (node.leaf, fetch_section(root, part_id)))
}

/// The structure of a message the bridge cannot describe: one part over
/// the whole message, sent as an attachment.
pub(super) fn too_complex(size: u32) -> BodyPart {
    BodyPart::Leaf(Leaf {
        media_type: "application".to_owned(),
        subtype: "octet-stream".to_owned(),
        encoding: "binary".to_owned(),
        bytes: size,
        disposition: Some(Disposition {
            kind: "attachment".to_owned(),
            parameters: vec![("FILENAME".to_owned(), WHOLE_MESSAGE_NAME.to_owned())],
        }),
        ..Leaf::default()
    })
}

/// The whole tree as `bodyStructure`, cut to the wanted properties; the
/// root has no number of its own.
pub(super) fn render_tree(root: &BodyPart, email: &str, wanted: &[&str]) -> Value {
    render(root, "", email, wanted)
}

/// One leaf as an `EmailBodyPart`, cut to the wanted properties.
pub(super) fn render_node(node: &Node<'_>, email: &str, wanted: &[&str]) -> Value {
    render(
        &BodyPart::Leaf(node.leaf.clone()),
        &node.part_id,
        email,
        wanted,
    )
}

fn render(part: &BodyPart, part_id: &str, email: &str, wanted: &[&str]) -> Value {
    let pairs = match part {
        BodyPart::Leaf(leaf) if part_id.is_empty() => leaf_pairs(leaf, LONE_PART, email),
        BodyPart::Leaf(leaf) => leaf_pairs(leaf, part_id, email),
        BodyPart::Multipart(multipart) => multipart_pairs(multipart, part_id, email, wanted),
    };
    pairs
        .into_iter()
        .filter(|(name, _)| wanted.contains(name))
        .map(|(name, value)| (name.to_owned(), value))
        .collect::<Map<_, _>>()
        .into()
}

fn leaf_pairs(leaf: &Leaf, part_id: &str, email: &str) -> Vec<(&'static str, Value)> {
    vec![
        ("partId", json!(part_id)),
        ("blobId", json!(part_blob_id(email, part_id))),
        ("size", json!(leaf.bytes)),
        (
            "name",
            json!(name(leaf.disposition.as_ref(), &leaf.parameters)),
        ),
        (
            "type",
            json!(format!("{}/{}", leaf.media_type, leaf.subtype)),
        ),
        ("charset", json!(charset(leaf))),
        (
            "disposition",
            json!(
                leaf.disposition
                    .as_ref()
                    .map(|disposition| &disposition.kind)
            ),
        ),
        ("cid", json!(leaf.id.as_deref().map(cid))),
        ("language", json!(leaf.language)),
        ("location", json!(leaf.location)),
        ("subParts", Value::Null),
    ]
}

fn multipart_pairs(
    multipart: &Multipart,
    part_id: &str,
    email: &str,
    wanted: &[&str],
) -> Vec<(&'static str, Value)> {
    let children: Vec<Value> = multipart
        .parts
        .iter()
        .enumerate()
        .map(|(index, child)| render(child, &child_id(part_id, index), email, wanted))
        .collect();
    vec![
        ("partId", Value::Null),
        ("blobId", Value::Null),
        (
            "size",
            json!(size_of(&BodyPart::Multipart(multipart.clone()))),
        ),
        (
            "name",
            json!(name(multipart.disposition.as_ref(), &multipart.parameters)),
        ),
        ("type", json!(format!("multipart/{}", multipart.subtype))),
        ("charset", Value::Null),
        (
            "disposition",
            json!(
                multipart
                    .disposition
                    .as_ref()
                    .map(|disposition| &disposition.kind)
            ),
        ),
        ("cid", Value::Null),
        ("language", json!(multipart.language)),
        ("location", json!(multipart.location)),
        ("subParts", Value::Array(children)),
    ]
}

/// The part number of the `index`th child of a part: the children of
/// the root, which has no number, count from `1`; deeper ones append a
/// level (RFC 3501 section 6.4.5).
pub(super) fn child_id(parent: &str, index: usize) -> String {
    if parent.is_empty() {
        return (index + 1).to_string();
    }
    format!("{parent}.{}", index + 1)
}

/// The octets of every leaf under a part.
fn size_of(part: &BodyPart) -> u32 {
    match part {
        BodyPart::Leaf(leaf) => leaf.bytes,
        BodyPart::Multipart(multipart) => multipart
            .parts
            .iter()
            .fold(0, |sum, part| sum.saturating_add(size_of(part))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(media_type: &str, subtype: &str, bytes: u32) -> Leaf {
        Leaf {
            media_type: media_type.to_owned(),
            subtype: subtype.to_owned(),
            encoding: "7bit".to_owned(),
            bytes,
            ..Leaf::default()
        }
    }

    #[test]
    fn the_body_properties_default_to_the_ten_of_the_rfc_and_refuse_one_not_served() {
        assert_eq!(body_properties(None).unwrap(), DEFAULT_BODY_PROPERTIES);
        let named = ["size".to_owned(), "subParts".to_owned(), "size".to_owned()];
        assert_eq!(body_properties(Some(&named)).unwrap(), ["size", "subParts"]);
        for property in ["headers", "header:Content-Type", "value"] {
            assert!(
                matches!(
                    body_properties(Some(&[property.to_owned()])),
                    Err(MethodError::InvalidArguments(_))
                ),
                "{property}"
            );
        }
    }

    #[test]
    fn a_part_number_follows_the_section_and_the_lone_part_reads_as_text() {
        assert_eq!(child_id("", 0), "1");
        assert_eq!(child_id("", 1), "2");
        assert_eq!(child_id("1", 0), "1.1");
        assert_eq!(child_id("2.1", 2), "2.1.3");
        let lone = BodyPart::Leaf(leaf("text", "plain", 5));
        assert_eq!(fetch_section(&lone, "1"), "TEXT");
        let mixed = Multipart::of("mixed", vec![lone.clone()]);
        assert_eq!(fetch_section(&mixed, "1"), "1");
        assert_eq!(fetch_section(&mixed, "1.2"), "1.2");
    }

    #[test]
    fn a_part_number_names_its_leaf_with_the_section_and_a_multipart_names_none() {
        let lone = BodyPart::Leaf(leaf("text", "plain", 5));
        let (found, section) = leaf_of(&lone, "1").unwrap();
        assert_eq!((found.bytes, section.as_str()), (5, "TEXT"));
        assert_eq!(leaf_of(&lone, "2"), None);
        let alternative = Multipart::of(
            "alternative",
            vec![lone, BodyPart::Leaf(leaf("text", "html", 9))],
        );
        let tree = Multipart::of(
            "mixed",
            vec![alternative, BodyPart::Leaf(leaf("image", "png", 30))],
        );
        let (found, section) = leaf_of(&tree, "1.2").unwrap();
        assert_eq!((found.bytes, section.as_str()), (9, "1.2"));
        let (found, section) = leaf_of(&tree, "2").unwrap();
        assert_eq!((found.bytes, section.as_str()), (30, "2"));
        for absent in ["1", "", "3", "1.3", "1.2.1", "01"] {
            assert_eq!(leaf_of(&tree, absent), None, "{absent}");
        }
    }

    #[test]
    fn a_leaf_renders_every_property_of_the_rfc_rfc8621_4_1_4() {
        let mut part = leaf("text", "html", 120);
        part.parameters = vec![("charset".to_owned(), "UTF-8".to_owned())];
        part.id = Some("<part1@x>".to_owned());
        part.disposition = Some(Disposition {
            kind: "inline".to_owned(),
            parameters: Vec::new(),
        });
        part.language = Some(vec!["en".to_owned()]);
        part.location = Some("http://x.test/b".to_owned());
        let rendered = render(&BodyPart::Leaf(part), "1.2", "e1", &BODY_PROPERTIES);
        assert_eq!(
            rendered,
            json!({
                "partId": "1.2",
                "blobId": "e1-1_2",
                "size": 120,
                "name": null,
                "type": "text/html",
                "charset": "UTF-8",
                "disposition": "inline",
                "cid": "part1@x",
                "language": ["en"],
                "location": "http://x.test/b",
                "subParts": null,
            })
        );
        let cut = render(
            &BodyPart::Leaf(leaf("image", "png", 9)),
            "2",
            "e1",
            &["type", "charset"],
        );
        assert_eq!(cut, json!({ "type": "image/png", "charset": null }));
    }

    #[test]
    fn a_multipart_renders_its_children_under_their_numbers_and_the_octets_beneath_it() {
        let alternative = Multipart::of(
            "alternative",
            vec![
                BodyPart::Leaf(leaf("text", "plain", 10)),
                BodyPart::Leaf(leaf("text", "html", 20)),
            ],
        );
        let tree = Multipart::of(
            "mixed",
            vec![alternative, BodyPart::Leaf(leaf("application", "pdf", 30))],
        );
        let rendered = render_tree(&tree, "e1", &["partId", "type", "size", "subParts"]);
        assert_eq!(
            rendered,
            json!({
                "partId": null,
                "type": "multipart/mixed",
                "size": 60,
                "subParts": [
                    {
                        "partId": null,
                        "type": "multipart/alternative",
                        "size": 30,
                        "subParts": [
                            { "partId": "1.1", "type": "text/plain", "size": 10, "subParts": null },
                            { "partId": "1.2", "type": "text/html", "size": 20, "subParts": null },
                        ],
                    },
                    { "partId": "2", "type": "application/pdf", "size": 30, "subParts": null },
                ],
            })
        );
    }

    #[test]
    fn a_message_the_bridge_cannot_describe_is_one_attachment_over_the_whole_message() {
        let rendered = render_tree(&too_complex(4321), "e1", &DEFAULT_BODY_PROPERTIES);
        assert_eq!(rendered["type"], "application/octet-stream");
        assert_eq!(rendered["name"], WHOLE_MESSAGE_NAME);
        assert_eq!(rendered["size"], 4321);
        assert_eq!(rendered["disposition"], "attachment");
        assert_eq!(rendered["charset"], Value::Null);
    }
}
