// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The three part lists of an Email: `textBody`, `htmlBody` and
//! `attachments`, the walk RFC 8621 section 4.1.4 suggests over the
//! tree, every leaf under its part number.

use super::child_id;
use crate::session::{BodyPart, Leaf};

/// One leaf with the number a fetch and a blob id name it by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::jmap) struct Node<'a> {
    pub part_id: String,
    pub leaf: &'a Leaf,
}

/// The three lists.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(in crate::jmap) struct Lists<'a> {
    pub text: Vec<Node<'a>>,
    pub html: Vec<Node<'a>>,
    pub attachments: Vec<Node<'a>>,
}

/// The lists of a tree, walked as the RFC has it: the root inside a
/// mixed body of its own.
pub(in crate::jmap) fn lists(root: &BodyPart) -> Lists<'_> {
    let mut lists = Lists::default();
    let mut walk = Walk {
        lists: &mut lists,
        add_text: true,
        add_html: true,
    };
    let outer = Frame {
        multipart_type: "mixed",
        in_alternative: false,
    };
    walk.parts(&[(String::new(), root)], outer);
    lists
}

/// Every leaf of a tree in the order of the message.
pub(in crate::jmap) fn leaves(root: &BodyPart) -> Vec<Node<'_>> {
    let mut found = Vec::new();
    collect(root, "", &mut found);
    found
}

fn collect<'a>(part: &'a BodyPart, part_id: &str, found: &mut Vec<Node<'a>>) {
    match part {
        BodyPart::Leaf(leaf) => found.push(Node {
            part_id: if part_id.is_empty() {
                "1".to_owned()
            } else {
                part_id.to_owned()
            },
            leaf,
        }),
        BodyPart::Multipart(multipart) => {
            for (index, child) in multipart.parts.iter().enumerate() {
                collect(child, &child_id(part_id, index), found);
            }
        }
    }
}

/// The walk's state: which lists still take parts while an alternative
/// body chose one of them.
struct Walk<'w, 'a> {
    lists: &'w mut Lists<'a>,
    add_text: bool,
    add_html: bool,
}

/// The multipart a part sits in: its subtype and whether an
/// alternative body encloses it.
#[derive(Clone, Copy)]
struct Frame<'f> {
    multipart_type: &'f str,
    in_alternative: bool,
}

impl<'a> Walk<'_, 'a> {
    /// The parts of one multipart.
    fn parts(&mut self, parts: &[(String, &'a BodyPart)], frame: Frame<'_>) {
        let text_length = self.add_text.then_some(self.lists.text.len());
        let html_length = self.add_html.then_some(self.lists.html.len());
        for (index, (part_id, part)) in parts.iter().enumerate() {
            match part {
                BodyPart::Multipart(multipart) => {
                    let children: Vec<(String, &'a BodyPart)> = multipart
                        .parts
                        .iter()
                        .enumerate()
                        .map(|(child, part)| (child_id(part_id, child), part))
                        .collect();
                    let (add_text, add_html) = (self.add_text, self.add_html);
                    let inner = Frame {
                        multipart_type: &multipart.subtype,
                        in_alternative: frame.in_alternative || multipart.subtype == "alternative",
                    };
                    self.parts(&children, inner);
                    self.add_text = add_text;
                    self.add_html = add_html;
                }
                BodyPart::Leaf(leaf) => {
                    let node = Node {
                        part_id: if part_id.is_empty() {
                            "1".to_owned()
                        } else {
                            part_id.clone()
                        },
                        leaf,
                    };
                    self.leaf(node, index, frame);
                }
            }
        }
        if frame.multipart_type == "alternative" && self.add_text && self.add_html {
            self.settle_alternative(text_length, html_length);
        }
    }

    /// One leaf: a body part goes to the lists still open for it, an
    /// inline image beside a chosen alternative and everything else to
    /// the attachments.
    fn leaf(&mut self, node: Node<'a>, index: usize, frame: Frame<'_>) {
        let leaf = node.leaf;
        let media = is_inline_media(leaf);
        let is_inline = !leaf.is_attachment()
            && (is_text(leaf) || media)
            && (index == 0 || (frame.multipart_type != "related" && (media || !has_name(leaf))));
        if !is_inline {
            self.lists.attachments.push(node);
            return;
        }
        if frame.multipart_type == "alternative" {
            match (leaf.media_type.as_str(), leaf.subtype.as_str()) {
                ("text", "plain") => self.lists.text.push(node),
                ("text", "html") => self.lists.html.push(node),
                _ => self.lists.attachments.push(node),
            }
            return;
        }
        if frame.in_alternative {
            if leaf.subtype == "plain" && leaf.media_type == "text" {
                self.add_html = false;
            }
            if leaf.subtype == "html" && leaf.media_type == "text" {
                self.add_text = false;
            }
        }
        if self.add_text {
            self.lists.text.push(node.clone());
        }
        if self.add_html {
            self.lists.html.push(node.clone());
        }
        if (!self.add_text || !self.add_html) && media {
            self.lists.attachments.push(node);
        }
    }

    /// An alternative body that offered one kind alone serves it in
    /// both lists.
    fn settle_alternative(&mut self, text_length: Option<usize>, html_length: Option<usize>) {
        let (Some(text_length), Some(html_length)) = (text_length, html_length) else {
            return;
        };
        if text_length == self.lists.text.len() && html_length != self.lists.html.len() {
            let found = self.lists.html[html_length..].to_vec();
            self.lists.text.extend(found);
        }
        if html_length == self.lists.html.len() && text_length != self.lists.text.len() {
            let found = self.lists.text[text_length..].to_vec();
            self.lists.html.extend(found);
        }
    }
}

fn is_text(leaf: &Leaf) -> bool {
    leaf.media_type == "text" && (leaf.subtype == "plain" || leaf.subtype == "html")
}

fn is_inline_media(leaf: &Leaf) -> bool {
    matches!(leaf.media_type.as_str(), "image" | "audio" | "video")
}

/// Whether the part carries a name in either header, an RFC 2231 form
/// included.
fn has_name(leaf: &Leaf) -> bool {
    let named = |parameters: &[(String, String)], name: &str| {
        parameters.iter().any(|(found, _)| {
            found.eq_ignore_ascii_case(name)
                || found
                    .get(..name.len() + 1)
                    .is_some_and(|head| head.eq_ignore_ascii_case(&format!("{name}*")))
        })
    };
    leaf.disposition
        .as_ref()
        .is_some_and(|disposition| named(&disposition.parameters, "filename"))
        || named(&leaf.parameters, "name")
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::session::{Disposition, Multipart};

    /// A leaf of one of the kinds the walk tells apart.
    fn any_leaf() -> impl Strategy<Value = BodyPart> {
        prop::sample::select(vec![
            ("text", "plain", None),
            ("text", "html", None),
            ("text", "plain", Some("attachment")),
            ("text", "calendar", None),
            ("image", "png", None),
            ("image", "png", Some("attachment")),
            ("application", "pdf", None),
            ("message", "rfc822", None),
        ])
        .prop_map(|(media_type, subtype, disposition)| leaf(media_type, subtype, disposition, None))
    }

    /// A tree of the three multipart kinds over such leaves.
    fn any_tree() -> impl Strategy<Value = BodyPart> {
        any_leaf().prop_recursive(4, 24, 3, |inner| {
            (
                prop::sample::select(vec!["mixed", "alternative", "related"]),
                prop::collection::vec(inner, 1..4),
            )
                .prop_map(|(subtype, parts)| multipart(subtype, parts))
        })
    }

    proptest! {
        #[test]
        fn every_list_holds_leaves_of_the_tree_in_order_and_an_attachment_lands_in_attachments_alone(
            root in any_tree(),
        ) {
            let leaves = leaves(&root);
            let found = lists(&root);
            for list in [&found.text, &found.html, &found.attachments] {
                let positions: Vec<usize> = list
                    .iter()
                    .map(|node| leaves.iter().position(|leaf| leaf.part_id == node.part_id))
                    .map(|position| position.expect("a leaf of the tree"))
                    .collect();
                prop_assert!(positions.windows(2).all(|pair| pair[0] < pair[1]), "{positions:?}");
            }
            let holds = |list: &[Node<'_>], id: &str| list.iter().any(|node| node.part_id == id);
            for node in &leaves {
                let inline_kind = is_text(node.leaf) || is_inline_media(node.leaf);
                if node.leaf.is_attachment() || !inline_kind {
                    prop_assert!(holds(&found.attachments, &node.part_id), "{}", node.part_id);
                    prop_assert!(!holds(&found.text, &node.part_id), "{}", node.part_id);
                    prop_assert!(!holds(&found.html, &node.part_id), "{}", node.part_id);
                }
            }
        }
    }

    fn leaf(
        media_type: &str,
        subtype: &str,
        disposition: Option<&str>,
        name: Option<&str>,
    ) -> BodyPart {
        BodyPart::Leaf(Leaf {
            media_type: media_type.to_owned(),
            subtype: subtype.to_owned(),
            parameters: name
                .map(|name| vec![("NAME".to_owned(), name.to_owned())])
                .unwrap_or_default(),
            disposition: disposition.map(|kind| Disposition {
                kind: kind.to_owned(),
                parameters: Vec::new(),
            }),
            ..Leaf::default()
        })
    }

    fn multipart(subtype: &str, parts: Vec<BodyPart>) -> BodyPart {
        BodyPart::Multipart(Multipart {
            subtype: subtype.to_owned(),
            parts,
            ..Multipart::default()
        })
    }

    fn ids<'n>(nodes: &'n [Node<'_>]) -> Vec<&'n str> {
        nodes.iter().map(|node| node.part_id.as_str()).collect()
    }

    /// The example of RFC 8621 section 4.1.4, its parts named by letter
    /// through their numbers.
    #[test]
    fn the_example_of_the_rfc_decomposes_as_the_rfc_says_rfc8621_4_1_4() {
        let tree = multipart(
            "mixed",
            vec![
                leaf("text", "plain", Some("inline"), None),
                multipart(
                    "mixed",
                    vec![
                        multipart(
                            "alternative",
                            vec![
                                multipart(
                                    "mixed",
                                    vec![
                                        leaf("text", "plain", Some("inline"), None),
                                        leaf("image", "jpeg", Some("inline"), None),
                                        leaf("text", "plain", Some("inline"), None),
                                    ],
                                ),
                                multipart(
                                    "related",
                                    vec![
                                        leaf("text", "html", None, None),
                                        leaf("image", "jpeg", None, None),
                                    ],
                                ),
                            ],
                        ),
                        leaf("image", "jpeg", Some("attachment"), None),
                        leaf("application", "x-excel", None, None),
                        leaf("message", "rfc822", None, None),
                    ],
                ),
                leaf("text", "plain", Some("inline"), None),
            ],
        );
        let found = lists(&tree);
        assert_eq!(
            ids(&found.text),
            ["1", "2.1.1.1", "2.1.1.2", "2.1.1.3", "3"]
        );
        assert_eq!(ids(&found.html), ["1", "2.1.2.1", "3"]);
        assert_eq!(
            ids(&found.attachments),
            ["2.1.1.2", "2.1.2.2", "2.2", "2.3", "2.4"]
        );
        assert_eq!(leaves(&tree).len(), 10);
        assert_eq!(leaves(&tree)[0].part_id, "1");
        assert_eq!(leaves(&tree)[9].part_id, "3");
    }

    #[test]
    fn a_message_of_one_part_is_part_one_in_both_lists() {
        let plain = leaf("text", "plain", None, None);
        let found = lists(&plain);
        assert_eq!(ids(&found.text), ["1"]);
        assert_eq!(ids(&found.html), ["1"]);
        assert!(found.attachments.is_empty());
        assert_eq!(ids(&leaves(&plain)), ["1"]);
        let attached = leaf("application", "pdf", Some("attachment"), Some("a.pdf"));
        let found = lists(&attached);
        assert!(found.text.is_empty() && found.html.is_empty());
        assert_eq!(ids(&found.attachments), ["1"]);
    }

    #[test]
    fn an_alternative_with_one_kind_serves_it_in_both_lists_and_a_named_text_part_attaches() {
        let html_only = multipart("alternative", vec![leaf("text", "html", None, None)]);
        let found = lists(&html_only);
        assert_eq!(ids(&found.text), ["1"]);
        assert_eq!(ids(&found.html), ["1"]);
        let named = multipart(
            "mixed",
            vec![
                leaf("text", "plain", None, None),
                leaf("text", "plain", None, Some("notes.txt")),
                leaf("text", "calendar", None, None),
            ],
        );
        let found = lists(&named);
        assert_eq!(ids(&found.text), ["1"]);
        assert_eq!(ids(&found.attachments), ["2", "3"]);
        let related = multipart(
            "related",
            vec![
                leaf("text", "html", None, None),
                leaf("image", "png", None, None),
            ],
        );
        let found = lists(&related);
        assert_eq!(ids(&found.html), ["1"]);
        assert_eq!(ids(&found.attachments), ["2"]);
    }
}
