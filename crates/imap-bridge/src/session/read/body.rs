// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The body path of one message: its structure with a set of header
//! fields, the fields alone and one window of one part (RFC 3501
//! section 6.4.5). Every item is a partial fetch, so what a sender wrote
//! arrives inside literals the ask bounds.

use async_imap::imap_proto::{AttributeValue, MessageSection, Response, SectionPath};

use super::preview::is_part_number;
use super::structure::tree;
use super::{Room, Selection};
use crate::session::{
    BODY_WINDOW_BYTES, MAX_HEADER_BYTES, PartAsk, PartWindow, SessionError, Structure,
};

/// The lines of its own an answer for one message carries.
const ONE_LINE: usize = 1;

/// The bytes asked of the header fields, the bound the header sync
/// keeps of a header.
const FIELDS_BYTES: usize = MAX_HEADER_BYTES;

/// The section of a message that is one part.
const TEXT_SECTION: &str = "TEXT";

/// The bytes an IMAP atom may not hold (RFC 3501 section 9) beside the
/// controls and the space; a field name holding one never reaches a
/// command line.
const ATOM_SPECIALS: &[u8] = b"(){%*\"\\]";

/// Whether a header field name goes on a command line as it is: printable
/// ASCII without the colon (RFC 5322 section 3.6.8) and without an atom
/// special.
pub(crate) fn sendable_field(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && byte != b':' && !ATOM_SPECIALS.contains(&byte))
}

/// The HEADER.FIELDS item for the names; `None` for none.
fn fields_item(fields: &[String]) -> Result<Option<String>, SessionError> {
    if fields.is_empty() {
        return Ok(None);
    }
    if !fields.iter().all(|field| sendable_field(field)) {
        return Err(SessionError::Protocol("a field name cannot be sent"));
    }
    Ok(Some(format!(
        "BODY.PEEK[HEADER.FIELDS ({})]<0.{FIELDS_BYTES}>",
        fields.join(" ")
    )))
}

/// BODYSTRUCTURE and the named header fields of one message in one
/// FETCH; `None` when no line came back for the UID.
pub(in crate::session) async fn uid_structure(
    selection: &mut Selection<'_>,
    uid: u32,
    fields: &[String],
    room: Room,
) -> Result<Option<Structure>, SessionError> {
    selection.messages()?;
    let fields = fields_item(fields)?.map_or(String::new(), |item| format!(" {item}"));
    let command = format!("UID FETCH {uid} (UID BODYSTRUCTURE{fields})");
    let mut found = None;
    let visit = |response: &Response<'_>| {
        if let Response::Fetch(_, attributes) = response
            && found.is_none()
            && uid_of(attributes) == Some(uid)
        {
            found = structure(attributes);
        }
        Ok(())
    };
    selection
        .collect(&command, room.bounds(ONE_LINE), visit)
        .await?;
    Ok(found)
}

/// The named header fields of one message; `None` when no line came
/// back for the UID.
pub(in crate::session) async fn uid_header_fields(
    selection: &mut Selection<'_>,
    uid: u32,
    fields: &[String],
    room: Room,
) -> Result<Option<Vec<u8>>, SessionError> {
    selection.messages()?;
    let Some(item) = fields_item(fields)? else {
        return Err(SessionError::Protocol("a field fetch names no field"));
    };
    let command = format!("UID FETCH {uid} (UID {item})");
    let mut found = None;
    let visit = |response: &Response<'_>| {
        if let Response::Fetch(_, attributes) = response
            && found.is_none()
            && uid_of(attributes) == Some(uid)
        {
            found = header_section(attributes);
        }
        Ok(())
    };
    selection
        .collect(&command, room.bounds(ONE_LINE), visit)
        .await?;
    Ok(found)
}

/// One window of one part of one message, cut at the bytes asked;
/// `None` when no line came back for the UID.
pub(in crate::session) async fn uid_part(
    selection: &mut Selection<'_>,
    ask: &PartAsk<'_>,
    room: Room,
) -> Result<Option<Vec<u8>>, SessionError> {
    selection.messages()?;
    let (uid, section, window) = (ask.uid, ask.section, ask.window);
    if section != TEXT_SECTION && !is_part_number(section) {
        return Err(SessionError::Protocol("a part number holds other bytes"));
    }
    if window.bytes > BODY_WINDOW_BYTES {
        return Err(SessionError::Protocol("a window passes its bound"));
    }
    let command = format!(
        "UID FETCH {uid} (UID BODY.PEEK[{section}]<{}.{}>)",
        window.offset, window.bytes
    );
    let mut found = None;
    let visit = |response: &Response<'_>| {
        if let Response::Fetch(_, attributes) = response
            && found.is_none()
            && uid_of(attributes) == Some(uid)
        {
            found = part_of(attributes, section, window)?;
        }
        Ok(())
    };
    selection
        .collect(&command, room.bounds(ONE_LINE), visit)
        .await?;
    Ok(found)
}

fn uid_of(attributes: &[AttributeValue<'_>]) -> Option<u32> {
    attributes.iter().find_map(|attribute| match attribute {
        AttributeValue::Uid(value) => Some(*value),
        _ => None,
    })
}

/// The structure of one line; `None` when the line carries none.
pub(super) fn structure(attributes: &[AttributeValue<'_>]) -> Option<Structure> {
    let body = attributes.iter().find_map(|attribute| match attribute {
        AttributeValue::BodyStructure(body) => Some(body),
        _ => None,
    })?;
    Some(Structure {
        tree: tree(body),
        header: header_of(attributes),
    })
}

/// The header fields of one line, cut at `FIELDS_BYTES`; empty when the
/// line carries none.
pub(super) fn header_of(attributes: &[AttributeValue<'_>]) -> Vec<u8> {
    header_section(attributes).unwrap_or_default()
}

/// The header fields of one line; `None` when the line carries none,
/// as a flag line another client caused does.
fn header_section(attributes: &[AttributeValue<'_>]) -> Option<Vec<u8>> {
    attributes.iter().find_map(|attribute| match attribute {
        AttributeValue::BodySection {
            section: Some(SectionPath::Full(MessageSection::Header)),
            data,
            ..
        } => Some(cut(data.as_deref().unwrap_or_default(), FIELDS_BYTES)),
        _ => None,
    })
}

/// The bytes of the window on one line: the section asked, cut at the
/// bytes asked; `None` when the line carries no such section. A line
/// without an origin octet answers the first window alone (RFC 3501
/// section 7.4.2), so a server that ignores the window never splices
/// its first bytes in further on; a section at another offset is a
/// protocol failure rather than a message that left.
pub(super) fn part_of(
    attributes: &[AttributeValue<'_>],
    section: &str,
    window: PartWindow,
) -> Result<Option<Vec<u8>>, SessionError> {
    let found = attributes.iter().find_map(|attribute| match attribute {
        AttributeValue::BodySection {
            section: Some(path),
            index,
            data,
        } if names(path, section) => Some((*index, data)),
        _ => None,
    });
    let Some((index, data)) = found else {
        return Ok(None);
    };
    if !index.map_or(window.offset == 0, |offset| offset == window.offset) {
        return Err(SessionError::Protocol("the answer names another window"));
    }
    let keep = usize::try_from(window.bytes).unwrap_or(usize::MAX);
    Ok(Some(cut(data.as_deref().unwrap_or_default(), keep)))
}

/// Whether the section on the line is the one asked: `TEXT` or the
/// part number written with its dots.
fn names(path: &SectionPath, section: &str) -> bool {
    match path {
        SectionPath::Full(MessageSection::Text) => section == TEXT_SECTION,
        SectionPath::Part(numbers, None) => {
            let written: Vec<String> = numbers.iter().map(u32::to_string).collect();
            written.join(".") == section
        }
        _ => false,
    }
}

fn cut(data: &[u8], keep: usize) -> Vec<u8> {
    data[..data.len().min(keep)].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::BodyPart;

    fn read(line: &str) -> Vec<AttributeValue<'static>> {
        match Response::from_bytes(line.as_bytes()).unwrap().1 {
            Response::Fetch(_, attributes) => attributes
                .into_iter()
                .map(|attribute| match attribute {
                    AttributeValue::Uid(uid) => AttributeValue::Uid(uid),
                    AttributeValue::BodySection {
                        section,
                        index,
                        data,
                    } => AttributeValue::BodySection {
                        section,
                        index,
                        data: data.map(|data| data.into_owned().into()),
                    },
                    _ => AttributeValue::Rfc822Size(0),
                })
                .collect(),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_field_name_goes_on_the_line_as_written_and_a_special_one_never_rfc3501_9() {
        assert_eq!(
            fields_item(&["Authentication-Results".to_owned(), "X-Spam".to_owned()]).unwrap(),
            Some(format!(
                "BODY.PEEK[HEADER.FIELDS (Authentication-Results X-Spam)]<0.{FIELDS_BYTES}>"
            ))
        );
        assert_eq!(fields_item(&[]).unwrap(), None);
        for name in [
            "",
            "a b",
            "a:b",
            "a(b",
            "a\"b",
            "a\\b",
            "a]b",
            "a*b",
            "a%b",
            "a\r\nA9 NOOP",
            "caf\u{e9}",
        ] {
            assert!(fields_item(&[name.to_owned()]).is_err(), "{name:?}");
        }
    }

    #[test]
    fn a_window_line_reads_the_text_section_and_a_numbered_part_at_the_offset_asked() {
        let window = PartWindow {
            offset: 4,
            bytes: 2,
        };
        let text = read("* 1 FETCH (UID 7 BODY[TEXT]<4> {3}\r\nabc)\r\n");
        assert_eq!(
            part_of(&text, "TEXT", window).unwrap(),
            Some(b"ab".to_vec())
        );
        assert_eq!(part_of(&text, "1", window).unwrap(), None);
        let part = read("* 1 FETCH (UID 7 BODY[1.2]<4> {2}\r\nab)\r\n");
        assert_eq!(part_of(&part, "1.2", window).unwrap(), Some(b"ab".to_vec()));
        assert_eq!(part_of(&part, "1", window).unwrap(), None);
        assert_eq!(part_of(&part, "TEXT", window).unwrap(), None);
        let elsewhere = read("* 1 FETCH (UID 7 BODY[1.2]<9> {2}\r\nab)\r\n");
        assert!(part_of(&elsewhere, "1.2", window).is_err());
        let empty = read("* 1 FETCH (UID 7 BODY[TEXT]<4> \"\")\r\n");
        assert_eq!(part_of(&empty, "TEXT", window).unwrap(), Some(Vec::new()));
        let unmarked = read("* 1 FETCH (UID 7 BODY[1.2] {3}\r\nabc)\r\n");
        assert!(part_of(&unmarked, "1.2", window).is_err());
        let first = PartWindow {
            offset: 0,
            bytes: 2,
        };
        assert_eq!(
            part_of(&unmarked, "1.2", first).unwrap(),
            Some(b"ab".to_vec())
        );
        let flags = read("* 1 FETCH (UID 7 FLAGS (\\Seen))\r\n");
        assert_eq!(part_of(&flags, "1.2", window).unwrap(), None);
        assert_eq!(header_section(&flags), None);
        assert_eq!(uid_of(&text), Some(7));
    }

    #[test]
    fn a_structure_line_reads_the_tree_and_the_fields_beside_it() {
        let line = "* 1 FETCH (UID 7 BODYSTRUCTURE (\"TEXT\" \"PLAIN\" NIL NIL NIL \"7BIT\" 5 1) BODY[HEADER.FIELDS (X-A)]<0> {10}\r\nX-A: 1\r\n\r\n)\r\n";
        let (_, response) = Response::from_bytes(line.as_bytes()).unwrap();
        let Response::Fetch(_, attributes) = response else {
            panic!("a fetch line")
        };
        let found = structure(&attributes).unwrap();
        assert!(matches!(found.tree, Some(BodyPart::Leaf(_))));
        assert_eq!(found.header, b"X-A: 1\r\n\r\n");
        let bare = read("* 1 FETCH (UID 7 FLAGS (\\Seen))\r\n");
        assert_eq!(header_of(&bare), Vec::<u8>::new());
    }
}
