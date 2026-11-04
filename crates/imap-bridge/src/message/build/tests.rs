// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

use mail_parser::{HeaderValue, MessageParser, MimeHeaders};
use proptest::prelude::*;

use super::*;
use crate::store::Personal;

/// When the stamp of these tests was made: 3 November 2026.
const DATE: i64 = 1_793_750_400;

const MESSAGE_ID: &str = "2d0e@example.test";

/// What opens an encoded word (RFC 2047 section 2).
const ENCODED_WORD: &str = "=?";

fn stamp() -> Stamp {
    Stamp {
        message_id: MESSAGE_ID.to_owned(),
        date: DATE,
    }
}

fn address(name: Option<&str>, email: &str) -> Address {
    Address {
        name: name.map(str::to_owned),
        email: email.to_owned(),
    }
}

fn text(media_type: &str, value: &str) -> Part {
    Part {
        media_type: media_type.to_owned(),
        parameters: Vec::new(),
        name: None,
        disposition: None,
        cid: None,
        content: Content::Value(value.to_owned()),
    }
}

fn blob(media_type: &str, id: &str) -> Part {
    Part {
        content: Content::Blob(id.to_owned()),
        ..text(media_type, "")
    }
}

fn multipart(media_type: &str, parts: Vec<Part>) -> Part {
    Part {
        content: Content::Parts(parts),
        ..text(media_type, "")
    }
}

fn draft(body: Part) -> Draft {
    Draft {
        mailbox: "m1".to_owned(),
        keywords: vec!["$draft".to_owned()],
        personal: Personal {
            from: Some(vec![address(Some("Sanne"), "sanne@example.test")]),
            ..Personal::default()
        },
        body,
    }
}

/// Every line of the message: none past the limit, every line end a CRLF
/// and no NUL (RFC 5322 sections 2.1.1 and 2.3).
fn well_formed(message: &[u8]) {
    assert!(!message.contains(&0), "a NUL");
    for (index, byte) in message.iter().enumerate() {
        if *byte == b'\n' {
            assert_eq!(
                message.get(index.wrapping_sub(1)),
                Some(&b'\r'),
                "a bare LF"
            );
        }
        if *byte == b'\r' {
            assert_eq!(message.get(index + 1), Some(&b'\n'), "a bare CR");
        }
    }
    for line in message.split(|byte| *byte == b'\n') {
        assert!(line.len() <= MAX_LINE_BYTES + 1, "{}", line.len());
    }
}

fn built(draft: &Draft, blobs: &Blobs) -> Vec<u8> {
    let message = build(draft, &stamp(), blobs).unwrap();
    well_formed(&message);
    message
}

#[test]
fn the_message_carries_the_fields_of_the_object_and_nothing_of_the_instance_rfc5322_3_6() {
    let mut drafted = draft(text("text/plain", "Hallo"));
    drafted.personal.to = Some(vec![
        address(Some("Mo, the first"), "mo@example.test"),
        address(None, "li@example.test"),
    ]);
    drafted.personal.bcc = Some(vec![address(None, "hidden@example.test")]);
    drafted.personal.subject = Some("Caf\u{e9} om drie".to_owned());
    drafted.personal.in_reply_to = Some(vec!["p1@example.test".to_owned()]);
    drafted.personal.references = Some(vec![
        "p0@example.test".to_owned(),
        "p1@example.test".to_owned(),
    ]);
    let message = built(&drafted, &Blobs::new());
    let parsed = MessageParser::default().parse(&message).unwrap();
    assert_eq!(parsed.message_id(), Some(MESSAGE_ID));
    assert_eq!(parsed.subject(), Some("Caf\u{e9} om drie"));
    assert_eq!(parsed.date().unwrap().to_timestamp(), DATE);
    let to: Vec<_> = parsed.to().unwrap().iter().collect();
    assert_eq!(to[0].name.as_deref(), Some("Mo, the first"));
    assert_eq!(to[1].address.as_deref(), Some("li@example.test"));
    assert_eq!(
        parsed.bcc().unwrap().first().unwrap().address.as_deref(),
        Some("hidden@example.test")
    );
    let HeaderValue::Text(replied) = parsed.in_reply_to() else {
        panic!("{:?}", parsed.in_reply_to());
    };
    assert_eq!(replied, "p1@example.test");
    assert_eq!(parsed.references().as_text_list().unwrap().len(), 2);
    let head = String::from_utf8_lossy(&message);
    assert!(head.contains("MIME-Version: 1.0\r\n"), "{head}");
    assert!(!head.contains("User-Agent"), "{head}");
    assert!(!head.contains("Sender:"), "{head}");
}

#[test]
fn a_text_part_declares_utf_8_and_the_parameters_of_the_object_rfc3676_4_1() {
    let mut plain = text("text/plain", "Soft \nbreak");
    plain.parameters = vec![
        ("format".to_owned(), "flowed".to_owned()),
        ("delsp".to_owned(), "yes".to_owned()),
    ];
    let message = built(&draft(plain), &Blobs::new());
    let parsed = MessageParser::default().parse(&message).unwrap();
    let content_type = parsed.content_type().unwrap();
    assert_eq!(content_type.attribute("charset"), Some("utf-8"));
    assert_eq!(content_type.attribute("format"), Some("flowed"));
    assert_eq!(content_type.attribute("delsp"), Some("yes"));
    assert_eq!(parsed.body_text(0).unwrap(), "Soft \r\nbreak");
}

#[test]
fn the_tree_comes_out_as_the_object_gives_it_rfc2046_5_1() {
    let mut inline = blob("image/png", "e1-2");
    inline.disposition = Some("inline".to_owned());
    inline.cid = Some("logo@example.test".to_owned());
    inline.name = Some("logo.png".to_owned());
    let mut attached = blob("application/pdf", "e1-3");
    attached.disposition = Some("attachment".to_owned());
    attached.name = Some("rapport caf\u{e9}.pdf".to_owned());
    let tree = multipart(
        "multipart/mixed",
        vec![
            multipart(
                "multipart/related",
                vec![
                    multipart(
                        "multipart/alternative",
                        vec![
                            text("text/plain", "Hallo"),
                            text("text/html", "<p>Hallo</p>"),
                        ],
                    ),
                    inline,
                ],
            ),
            attached,
        ],
    );
    let blobs: Blobs = [
        ("e1-2".to_owned(), vec![0x89, b'P', b'N', b'G', 0]),
        ("e1-3".to_owned(), vec![0xff; 4096]),
    ]
    .into();
    let message = built(&draft(tree), &blobs);
    let parsed = MessageParser::default().parse(&message).unwrap();
    assert_eq!(
        parsed.content_type().unwrap().c_subtype.as_deref(),
        Some("mixed")
    );
    assert_eq!(parsed.body_text(0).unwrap(), "Hallo");
    assert_eq!(parsed.body_html(0).unwrap(), "<p>Hallo</p>");
    let attachments: Vec<_> = parsed.attachments().collect();
    assert_eq!(attachments.len(), 2);
    let image = attachments
        .iter()
        .find(|part| part.content_id().is_some())
        .unwrap();
    assert_eq!(image.content_id(), Some("logo@example.test"));
    assert_eq!(image.contents(), [0x89, b'P', b'N', b'G', 0]);
    let pdf = attachments
        .iter()
        .find(|part| part.content_id().is_none())
        .unwrap();
    assert_eq!(pdf.attachment_name(), Some("rapport caf\u{e9}.pdf"));
    assert_eq!(pdf.content_type().unwrap().attribute("name"), None);
    assert_eq!(pdf.contents(), vec![0xff; 4096]);
    let head = String::from_utf8_lossy(&message);
    assert!(head.contains("filename*"), "{head}");
}

#[test]
fn a_name_without_a_disposition_stands_in_the_type_rfc2045_5_1() {
    let mut part = blob("application/pdf", "e1");
    part.name = Some("a.pdf".to_owned());
    let message = built(&draft(part), &[("e1".to_owned(), vec![1, 2])].into());
    let parsed = MessageParser::default().parse(&message).unwrap();
    assert_eq!(
        parsed.content_type().unwrap().attribute("name"),
        Some("a.pdf")
    );
    assert!(parsed.content_disposition().is_none());
}

#[test]
fn a_message_inside_a_message_stays_as_it_is_where_its_lines_allow_rfc2046_5_2_1() {
    let whole = b"Subject: Inner\r\n\r\nBody\r\n".to_vec();
    let message = built(
        &draft(blob("message/rfc822", "e1")),
        &[("e1".to_owned(), whole.clone())].into(),
    );
    let head = String::from_utf8_lossy(&message);
    assert!(
        head.contains("Content-Transfer-Encoding: 7bit\r\n"),
        "{head}"
    );
    assert!(head.ends_with("Subject: Inner\r\n\r\nBody\r\n"), "{head}");
    let bare = b"Subject: Inner\n\nBody\n".to_vec();
    let message = built(
        &draft(blob("message/rfc822", "e1")),
        &[("e1".to_owned(), bare)].into(),
    );
    assert!(String::from_utf8_lossy(&message).contains("base64"));
}

#[test]
fn a_message_inside_a_message_names_its_bytes_and_keeps_to_the_line_limit_rfc2046_5_2_1() {
    let inner = |body: String| {
        let whole = format!("Subject: Inner\r\n\r\n{body}\r\n").into_bytes();
        let message = built(
            &draft(blob("message/rfc822", "e1")),
            &[("e1".to_owned(), whole)].into(),
        );
        String::from_utf8_lossy(&message).into_owned()
    };
    let encoding = |name: &str| format!("Content-Transfer-Encoding: {name}\r\n");
    assert!(inner("Caf\u{e9}".to_owned()).contains(&encoding("8bit")));
    assert!(inner("x".repeat(MAX_LINE_BYTES)).contains(&encoding("7bit")));
    assert!(inner("x".repeat(MAX_LINE_BYTES + 1)).contains(&encoding("base64")));
}

#[test]
fn text_a_literal_cannot_carry_as_it_stands_leaves_in_base64_rfc2045_6_8() {
    for bytes in [&b"a\0b"[..], b"a\rb"] {
        let message = built(
            &draft(blob("text/plain", "e1")),
            &[("e1".to_owned(), bytes.to_vec())].into(),
        );
        let parsed = MessageParser::default().parse(&message).unwrap();
        assert_eq!(parsed.root_part().contents(), bytes);
        assert!(String::from_utf8_lossy(&message).contains("Content-Transfer-Encoding: base64"));
    }
}

#[test]
fn a_body_value_takes_a_crlf_for_every_line_end_rfc5322_2_3() {
    assert_eq!(crlf("a\nb\r\nc\rd"), "a\r\nb\r\nc\r\nd");
    assert_eq!(crlf("a\r\r\nb"), "a\r\n\r\nb");
    assert_eq!(crlf(""), "");
}

#[test]
fn the_row_of_a_draft_says_what_the_sync_would_read_rfc8621_4_1_4() {
    let mut attached = blob("application/pdf", "e1");
    attached.disposition = Some("attachment".to_owned());
    let plain = draft(text("text/plain", "Caf\u{e9}\n\nom   drie uur"));
    assert_eq!(
        plain.stamped(&stamp()).preview.as_deref(),
        Some("Caf\u{e9} om drie uur")
    );
    assert!(!plain.has_attachment());
    let html = draft(text("text/html", "<p>Hallo <b>daar</b></p>"));
    assert_eq!(
        html.stamped(&stamp()).preview.as_deref(),
        Some("Hallo daar")
    );
    let inline = multipart(
        "multipart/related",
        vec![text("text/html", "x"), blob("image/png", "e1")],
    );
    assert!(!draft(inline).has_attachment());
    let mixed = multipart("multipart/mixed", vec![text("text/plain", "x"), attached]);
    assert!(draft(mixed).has_attachment());
    let bare = draft(blob("image/png", "e1")).stamped(&stamp());
    assert_eq!(bare.preview.as_deref(), Some(""));
    let mut sent = text("text/plain", "In the file");
    sent.disposition = Some("attachment".to_owned());
    let beside = multipart("multipart/mixed", vec![sent, text("text/html", "Hallo")]);
    assert_eq!(
        draft(beside).stamped(&stamp()).preview.as_deref(),
        Some("Hallo")
    );
}

#[test]
fn a_blob_named_twice_is_read_once_and_weighs_twice() {
    let parts = ["e1", "e2", "e1"].map(|id| blob("image/png", id));
    let mut leaves = parts.to_vec();
    leaves.push(text("text/plain", "abc"));
    let tree = multipart("multipart/mixed", leaves);
    let (one, other) = (vec![0; 5], vec![0; 7]);
    let weighed = 2 * one.len() + other.len() + "abc".len();
    let blobs: Blobs = [("e1".to_owned(), one), ("e2".to_owned(), other)].into();
    assert_eq!(weight(&tree, &blobs), u64::try_from(weighed).unwrap());
    assert_eq!(draft(tree).blob_ids(), ["e1", "e2"]);
}

#[test]
fn a_fresh_id_stands_at_the_domain_of_the_sender_rfc5322_3_6_4() {
    let stamped = Stamp::now(&draft(text("text/plain", "x")));
    assert!(
        stamped.message_id.ends_with("@example.test"),
        "{}",
        stamped.message_id
    );
    let mut anonymous = draft(text("text/plain", "x"));
    anonymous.personal.from = None;
    assert!(
        Stamp::now(&anonymous)
            .message_id
            .ends_with("@draft.invalid")
    );
    assert_ne!(
        stamped.message_id,
        Stamp::now(&draft(text("text/plain", "x"))).message_id
    );
}

/// Text with what a hostile value holds: line breaks in a body, marks
/// outside ASCII, long runs without a space.
fn hostile_text() -> impl Strategy<Value = String> {
    let pieces = prop_oneof![
        Just("\r\n".to_owned()),
        Just("\n".to_owned()),
        Just("\r".to_owned()),
        Just(".\r\n".to_owned()),
        Just("From ".to_owned()),
        Just("=?utf-8?Q?x?=".to_owned()),
        Just("Caf\u{e9} \u{1f600} ".to_owned()),
        Just("x".repeat(2000)),
        "[ -~]{0,40}",
    ];
    prop::collection::vec(pieces, 0..8).prop_map(|pieces| pieces.concat())
}

/// A header value: the same without its line breaks.
fn hostile_header() -> impl Strategy<Value = String> {
    hostile_text().prop_map(|text| text.replace(['\r', '\n'], ""))
}

proptest! {
    /// CRLF, a line past 998 octets and a non-ASCII subject reach the
    /// parse as a well-formed message with their value whole (RFC 5322
    /// section 2.1.1, RFC 2047).
    #[test]
    fn a_hostile_draft_stays_well_formed_and_reads_back_whole(
        subject in hostile_header(),
        name in hostile_header(),
        body in hostile_text(),
    ) {
        let mut drafted = draft(text("text/plain", &body));
        drafted.personal.subject = Some(subject.clone());
        drafted.personal.to = Some(vec![address(Some(&name), "mo@example.test")]);
        let message = built(&drafted, &Blobs::new());
        let parsed = MessageParser::default().parse(&message).unwrap();
        // Text that reads as an encoded word reads as what it encodes.
        if !subject.contains(ENCODED_WORD) {
            prop_assert_eq!(parsed.subject().unwrap_or_default().trim(), subject.trim());
        }
        let to = parsed.to().unwrap().first().unwrap();
        prop_assert_eq!(to.address.as_deref(), Some("mo@example.test"));
        let read = parsed.body_text(0).unwrap_or_default().replace("\r\n", "\n");
        let wrote = crlf(&body).replace("\r\n", "\n");
        prop_assert_eq!(read.trim_end(), wrote.trim_end());
    }

    /// Bytes of any kind in a part come back as they went (RFC 2045
    /// section 6.8).
    #[test]
    fn any_blob_reads_back_whole(bytes in prop::collection::vec(any::<u8>(), 0..2048)) {
        for media_type in ["application/octet-stream", "image/png"] {
            let message = built(
                &draft(blob(media_type, "e1")),
                &[("e1".to_owned(), bytes.clone())].into(),
            );
            let parsed = MessageParser::default().parse(&message).unwrap();
            prop_assert_eq!(parsed.root_part().contents(), bytes.as_slice());
        }
    }
}
