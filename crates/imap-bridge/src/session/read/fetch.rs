// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! UID FETCH of the header items: one message per FETCH line, every
//! field a server controls read under a bound.

use std::collections::BTreeMap;

use async_imap::imap_proto::{
    AttributeValue, BodyContentCommon, BodyStructure, MessageSection, Response, SectionPath,
};
use time::OffsetDateTime;
use time::format_description::FormatItem;
use time::macros::format_description;

use super::{Room, Selection, modseq};
use crate::dates::utc_date;
use crate::session::{
    BodyPart, FetchedMessage, MAX_FETCH_MESSAGES, MAX_HEADER_BYTES, MESSAGE_LIMIT, SessionError,
    UidRange,
};

/// The header fields the Email object is built from (RFC 8621 section
/// 4.1.2.3).
const HEADER_FIELDS: &str =
    "FROM SENDER REPLY-TO TO CC BCC SUBJECT DATE MESSAGE-ID IN-REPLY-TO REFERENCES";

/// The flags kept per message; a server allows a few dozen keywords per
/// mailbox.
const MAX_FLAGS: usize = 64;

/// The longest flag kept, the keyword length of RFC 8621 section 4.1.1.
const MAX_FLAG_BYTES: usize = 255;

/// The parts kept of one BODYSTRUCTURE; a message past it goes without
/// a structure.
const MAX_BODY_PARTS: usize = 1024;

/// The longest media type word kept (RFC 6838 section 4.2).
const MAX_MEDIA_TYPE_BYTES: usize = 127;

/// INTERNALDATE as RFC 3501 section 9 writes it, the day padded with a
/// space.
const INTERNAL_DATE: &[FormatItem<'static>] = format_description!(
    "[day padding:space]-[month repr:short]-[year] [hour]:[minute]:[second] [offset_hour sign:mandatory][offset_minute]"
);

fn command(range: UidRange, structure: bool) -> String {
    let structure = if structure { " BODYSTRUCTURE" } else { "" };
    format!(
        "UID FETCH {}:{} (UID FLAGS INTERNALDATE RFC822.SIZE{structure} BODY.PEEK[HEADER.FIELDS ({HEADER_FIELDS})])",
        range.low, range.high
    )
}

/// The messages of the range by UID, each once. A FETCH line without
/// the items asked for is a flag update another client caused and is
/// skipped; more than `MAX_FETCH_MESSAGES` messages fail the answer.
pub(in crate::session) async fn uid_fetch(
    selection: &mut Selection<'_>,
    range: UidRange,
    structure: bool,
    room: Room,
) -> Result<Vec<FetchedMessage>, SessionError> {
    selection.messages()?;
    // IMAP reads `5:3` as `3:5`, which the range test below would not.
    if range.low > range.high {
        return Err(SessionError::Protocol("the UID range runs backward"));
    }
    let bounds = room.bounds(MAX_FETCH_MESSAGES);
    let mut messages = BTreeMap::new();
    let visit = |response: &Response<'_>| {
        if let Response::Fetch(_, attributes) = response
            && let Some(message) = message(attributes)?
            && (range.low..=range.high).contains(&message.uid)
            && !messages.contains_key(&message.uid)
        {
            if messages.len() == MAX_FETCH_MESSAGES {
                return Err(SessionError::Protocol(MESSAGE_LIMIT));
            }
            messages.insert(message.uid, message);
        }
        Ok(())
    };
    selection
        .collect(&command(range, structure), bounds, visit)
        .await?;
    Ok(messages.into_values().collect())
}

/// One FETCH line; `None` when the UID, the date, the size or the
/// header is missing.
pub(super) fn message(
    attributes: &[AttributeValue<'_>],
) -> Result<Option<FetchedMessage>, SessionError> {
    let (mut uid, mut received_at, mut size, mut header) = (None, None, None, None);
    let mut flags = Vec::new();
    let mut structure = None;
    for attribute in attributes {
        match attribute {
            AttributeValue::Uid(value) => uid = Some(*value),
            AttributeValue::Flags(found) => flags = kept_flags(found),
            AttributeValue::InternalDate(text) => received_at = Some(internal_date(text)?),
            AttributeValue::Rfc822Size(value) => size = Some(*value),
            AttributeValue::BodyStructure(body) => structure = body_tree(body),
            AttributeValue::BodySection {
                section: Some(SectionPath::Full(MessageSection::Header)),
                data,
                ..
            } => header = Some(cut(data.as_deref().unwrap_or_default())),
            // No value is kept yet; the bound holds for every one that arrives.
            AttributeValue::ModSeq(value) => {
                modseq(*value)?;
            }
            _ => {}
        }
    }
    let (Some(uid), Some(received_at), Some(size), Some(header)) = (uid, received_at, size, header)
    else {
        return Ok(None);
    };
    Ok(Some(FetchedMessage {
        uid,
        flags,
        received_at,
        size,
        header,
        structure,
    }))
}

fn cut(header: &[u8]) -> Vec<u8> {
    header[..header.len().min(MAX_HEADER_BYTES)].to_vec()
}

pub(super) fn kept_flags<T: AsRef<str>>(flags: &[T]) -> Vec<String> {
    flags
        .iter()
        .map(AsRef::as_ref)
        .filter(|flag| flag.len() <= MAX_FLAG_BYTES)
        .take(MAX_FLAGS)
        .map(str::to_owned)
        .collect()
}

/// The date in seconds. One the format refuses fails the line and so
/// does one whose UTC instant `receivedAt` could not render, since the
/// offset can carry a date past either end of the calendar.
fn internal_date(text: &str) -> Result<i64, SessionError> {
    OffsetDateTime::parse(text, INTERNAL_DATE)
        .ok()
        .map(OffsetDateTime::unix_timestamp)
        .filter(|timestamp| utc_date(*timestamp).is_some())
        .ok_or(SessionError::Protocol(
            "a message carries no valid INTERNALDATE",
        ))
}

/// The tree in the bridge's own type; `None` past `MAX_BODY_PARTS`. The
/// guard under the client library bounds its depth.
fn body_tree(body: &BodyStructure<'_>) -> Option<BodyPart> {
    let mut room = MAX_BODY_PARTS;
    part(body, &mut room)
}

fn part(body: &BodyStructure<'_>, room: &mut usize) -> Option<BodyPart> {
    *room = room.checked_sub(1)?;
    Some(match body {
        BodyStructure::Multipart { common, bodies, .. } => BodyPart::Multipart {
            subtype: word(&common.ty.subtype),
            parts: bodies
                .iter()
                .map(|body| part(body, room))
                .collect::<Option<_>>()?,
        },
        BodyStructure::Basic { common, other, .. }
        | BodyStructure::Text { common, other, .. }
        | BodyStructure::Message { common, other, .. } => leaf(common, other.octets),
    })
}

fn leaf(common: &BodyContentCommon<'_>, bytes: u32) -> BodyPart {
    BodyPart::Leaf {
        media_type: word(&common.ty.ty),
        subtype: word(&common.ty.subtype),
        attachment: common
            .disposition
            .as_ref()
            .is_some_and(|disposition| disposition.ty.eq_ignore_ascii_case("attachment")),
        bytes,
    }
}

/// A media type word in lower case, cut at `MAX_MEDIA_TYPE_BYTES` on a
/// character border.
fn word(text: &str) -> String {
    let mut end = text.len().min(MAX_MEDIA_TYPE_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "Subject: hi\r\n\r\n";

    fn line(items: &str) -> String {
        format!(
            "* 1 FETCH ({items} BODY[HEADER.FIELDS (SUBJECT)] {{{}}}\r\n{HEADER})\r\n",
            HEADER.len()
        )
    }

    fn read(line: &str) -> Result<Option<FetchedMessage>, SessionError> {
        match Response::from_bytes(line.as_bytes()).unwrap().1 {
            Response::Fetch(_, attributes) => message(&attributes),
            other => panic!("{other:?}"),
        }
    }

    const ITEMS: &str = "UID 7 FLAGS (\\Seen $Forwarded) INTERNALDATE \" 7-Jul-1996 02:44:25 -0700\" RFC822.SIZE 4286";

    #[test]
    fn a_fetch_line_reads_into_a_message_rfc3501_7_4_2() {
        let structure = "BODYSTRUCTURE ((\"TEXT\" \"PLAIN\" NIL NIL NIL \"7BIT\" 1 1)(\"APPLICATION\" \"PDF\" NIL NIL NIL \"BASE64\" 9 NIL (\"ATTACHMENT\" NIL)) \"MIXED\")";
        let message = read(&line(&format!("{ITEMS} {structure}")))
            .unwrap()
            .unwrap();
        assert_eq!(message.uid, 7);
        assert_eq!(message.flags, ["\\Seen", "$Forwarded"]);
        assert_eq!(message.received_at, 836_732_665);
        assert_eq!(message.size, 4286);
        assert_eq!(message.header, HEADER.as_bytes());
        assert_eq!(
            message.structure,
            Some(BodyPart::Multipart {
                subtype: "mixed".to_owned(),
                parts: vec![
                    BodyPart::Leaf {
                        media_type: "text".to_owned(),
                        subtype: "plain".to_owned(),
                        attachment: false,
                        bytes: 1,
                    },
                    BodyPart::Leaf {
                        media_type: "application".to_owned(),
                        subtype: "pdf".to_owned(),
                        attachment: true,
                        bytes: 9,
                    },
                ],
            })
        );
    }

    #[test]
    fn a_flag_update_another_client_caused_is_no_message() {
        assert_eq!(read("* 3 FETCH (UID 7 FLAGS (\\Seen))\r\n").unwrap(), None);
        assert_eq!(read("* 3 FETCH (FLAGS (\\Seen))\r\n").unwrap(), None);
    }

    #[test]
    fn a_mod_sequence_past_63_bits_fails_the_line_rfc7162_3_1() {
        let top = line(&format!("{ITEMS} MODSEQ (9223372036854775807)"));
        assert!(read(&top).unwrap().is_some());
        let past = line(&format!("{ITEMS} MODSEQ (9223372036854775808)"));
        assert!(matches!(
            read(&past),
            Err(SessionError::Protocol("a mod-sequence passes 63 bits"))
        ));
    }

    #[test]
    fn a_media_type_word_is_cut_by_bytes_on_a_character_border() {
        let cut = word(&"\u{e9}".repeat(MAX_MEDIA_TYPE_BYTES));
        assert_eq!(cut.len(), MAX_MEDIA_TYPE_BYTES - 1);
        assert_eq!(word("TEXT"), "text");
    }

    #[test]
    fn a_date_the_format_refuses_or_no_utc_date_can_render_fails_the_line() {
        for date in [
            "31-Feb-1996 02:44:25 -0700",
            "31-Dec-9999 23:59:59 -0100",
            " 1-Jan-0000 00:00:00 +0100",
        ] {
            let items = ITEMS.replace(" 7-Jul-1996 02:44:25 -0700", date);
            assert!(
                matches!(
                    read(&line(&items)),
                    Err(SessionError::Protocol(
                        "a message carries no valid INTERNALDATE"
                    ))
                ),
                "{date}"
            );
        }
    }

    #[test]
    fn the_header_is_cut_and_long_or_many_flags_are_dropped() {
        assert_eq!(
            cut(&vec![b'x'; MAX_HEADER_BYTES + 9]).len(),
            MAX_HEADER_BYTES
        );
        let long = "k".repeat(MAX_FLAG_BYTES + 1);
        let mut flags: Vec<String> = (0..MAX_FLAGS + 8).map(|n| format!("k{n}")).collect();
        flags[1] = long;
        let kept = kept_flags(&flags);
        assert_eq!(kept.len(), MAX_FLAGS);
        assert_eq!(kept[1], "k2");
    }

    #[test]
    fn a_structure_past_the_part_limit_is_dropped_whole() {
        let leaf = "(\"TEXT\" \"PLAIN\" NIL NIL NIL \"7BIT\" 1 1)";
        let structure =
            |leaves: usize| format!("BODYSTRUCTURE ({} \"MIXED\")", leaf.repeat(leaves));
        let inside = read(&line(&format!("{ITEMS} {}", structure(MAX_BODY_PARTS - 1))));
        assert!(inside.unwrap().unwrap().structure.is_some());
        let past = read(&line(&format!("{ITEMS} {}", structure(MAX_BODY_PARTS))));
        assert_eq!(past.unwrap().unwrap().structure, None);
    }
}
