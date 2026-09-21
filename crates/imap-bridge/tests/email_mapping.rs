// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Properties of the message mapping: flags to keywords, the
//! attachment mark, header bytes nobody vetted and names in modified
//! UTF-7.

use huliho_imap_bridge::session::{BodyPart, FetchedMessage};
use huliho_imap_bridge::sync::{headers, mapping};
use huliho_imap_bridge::utf7;
use proptest::prelude::*;

const SYSTEM: [(&str, &str); 4] = [
    ("\\Seen", "$seen"),
    ("\\Flagged", "$flagged"),
    ("\\Answered", "$answered"),
    ("\\Draft", "$draft"),
];

/// The size every leaf of these trees claims.
const PART_BYTES: u32 = 120;

/// The text with each letter's case picked by one bit of `mask`.
fn recased(text: &str, mask: u32) -> String {
    text.chars()
        .enumerate()
        .map(|(index, c)| {
            if mask >> (index % 32) & 1 == 1 {
                c.to_ascii_uppercase()
            } else {
                c.to_ascii_lowercase()
            }
        })
        .collect()
}

/// The keyword alphabet of RFC 8621 section 4.1.1.
fn is_keyword(text: &str) -> bool {
    let forbidden = b"(){]%*\"\\";
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && !forbidden.contains(&byte))
}

fn leaf(media_type: &str, attachment: bool) -> BodyPart {
    typed(media_type, "plain", attachment)
}

fn typed(media_type: &str, subtype: &str, attachment: bool) -> BodyPart {
    BodyPart::Leaf {
        media_type: media_type.to_owned(),
        subtype: subtype.to_owned(),
        attachment,
        bytes: PART_BYTES,
    }
}

/// A tree of multiparts over the given leaves.
fn tree(leaves: impl Strategy<Value = BodyPart> + 'static) -> impl Strategy<Value = BodyPart> {
    leaves.prop_recursive(5, 32, 4, |inner| {
        (
            prop::sample::select(vec!["mixed", "alternative", "related"]),
            prop::collection::vec(inner, 1..4),
        )
            .prop_map(|(subtype, parts)| BodyPart::Multipart {
                subtype: subtype.to_owned(),
                parts,
            })
    })
}

/// Whether any leaf of the tree is sent as an attachment.
fn holds_one(part: &BodyPart) -> bool {
    match part {
        BodyPart::Leaf { attachment, .. } => *attachment,
        BodyPart::Multipart { parts, .. } => parts.iter().any(holds_one),
    }
}

fn message(flags: Vec<String>, header: Vec<u8>) -> FetchedMessage {
    FetchedMessage {
        uid: 1,
        flags,
        received_at: 0,
        size: 0,
        header,
        structure: None,
    }
}

#[test]
fn an_inline_image_counts_outside_a_related_body_only_rfc8621_4_1_4() {
    let under = |subtype: &str| BodyPart::Multipart {
        subtype: subtype.to_owned(),
        parts: vec![leaf("text", false), leaf("image", false)],
    };
    assert!(!mapping::has_attachment(&[], Some(&under("related"))));
    assert!(mapping::has_attachment(&[], Some(&under("mixed"))));
    assert!(!mapping::has_attachment(&[], None));
}

proptest! {
    #[test]
    fn a_system_flag_maps_in_any_case_and_every_other_backslash_flag_drops_rfc8621_4_1_1(
        index in 0..SYSTEM.len(),
        mask in any::<u32>(),
        other in "\\\\[A-Za-z]{1,12}",
    ) {
        let (flag, keyword) = SYSTEM[index];
        let found = mapping::keywords(&[recased(flag, mask)]);
        prop_assert_eq!(found.keys().collect::<Vec<_>>(), [keyword]);
        let known = SYSTEM.iter().any(|(name, _)| name.eq_ignore_ascii_case(&other));
        prop_assert_eq!(mapping::keywords(&[other]).is_empty(), !known);
    }

    #[test]
    fn every_flag_without_a_backslash_lowercases_into_a_keyword_or_drops(
        flag in "[ -\\[\\]-~]{0,40}",
    ) {
        let found = mapping::keywords(std::slice::from_ref(&flag));
        for keyword in found.keys() {
            prop_assert_eq!(keyword, &flag.to_ascii_lowercase());
            prop_assert!(is_keyword(keyword));
            let again = mapping::keywords(std::slice::from_ref(keyword));
            prop_assert_eq!(again.keys().collect::<Vec<_>>(), [keyword]);
        }
    }

    #[test]
    fn a_deleted_message_gives_no_facts_whatever_else_it_carries(
        mask in any::<u32>(),
        others in prop::collection::vec("[a-zA-Z$\\\\]{1,8}", 0..4),
        header in prop::collection::vec(any::<u8>(), 0..64),
    ) {
        let mut flags = others;
        flags.push(recased("\\Deleted", mask));
        prop_assert!(mapping::email(&message(flags, header)).is_none());
    }

    #[test]
    fn text_alone_never_attaches_and_one_attachment_anywhere_always_does(
        texts in tree(Just(leaf("text", false))),
        mixed in tree(prop_oneof![Just(leaf("text", false)), Just(leaf("text", true))]),
    ) {
        prop_assert!(!mapping::has_attachment(&[], Some(&texts)));
        prop_assert_eq!(mapping::has_attachment(&[], Some(&mixed)), holds_one(&mixed));
    }

    #[test]
    fn the_two_flags_of_dovecot_win_over_any_structure(
        body in tree(prop_oneof![Just(leaf("text", false)), Just(leaf("application", true))]),
        mask in any::<u32>(),
    ) {
        let has = [recased("$HasAttachment", mask)];
        let has_not = [recased("$HasNoAttachment", mask)];
        prop_assert!(mapping::has_attachment(&has, Some(&body)));
        prop_assert!(!mapping::has_attachment(&has_not, Some(&body)));
    }

    #[test]
    fn header_bytes_nobody_vetted_read_without_a_panic(
        bytes in prop::collection::vec(any::<u8>(), 0..512),
        field in prop::sample::select(vec!["From", "To", "Subject", "Date", "References"]),
    ) {
        let _ = headers::parse(&bytes);
        let mut framed = format!("{field}: ").into_bytes();
        framed.extend_from_slice(&bytes);
        framed.extend_from_slice(b"\r\n\r\n");
        let facts = mapping::email(&message(Vec::new(), framed));
        prop_assert!(facts.is_some());
    }

    #[test]
    fn any_name_decodes_without_a_panic_and_one_without_a_shift_stays_rfc3501_5_1_3(
        name in ".{0,64}",
    ) {
        let decoded = utf7::decode(&name);
        if !name.contains('&') {
            prop_assert_eq!(decoded, name);
        }
    }
}
