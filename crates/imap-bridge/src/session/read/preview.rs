// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! UID FETCH of the start of one text part: the header that says how it
//! is encoded and the first bytes of its text. Every item is a partial
//! fetch (RFC 3501 section 6.4.5), so what a sender wrote arrives inside
//! literals the ask bounds.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use async_imap::imap_proto::{AttributeValue, MessageSection, Response, SectionPath};

use super::{Room, Selection};
use crate::session::{
    MAX_PREVIEW_TEXT_BYTES, MAX_PREVIEWS, PREVIEW_HEADER_BYTES, PreviewAsk, PreviewBytes,
    SessionError,
};

/// The two fields that say how a message of one part is encoded.
const ENCODING_FIELDS: &str = "CONTENT-TYPE CONTENT-TRANSFER-ENCODING";

/// The UIDs as a sequence set, runs folded into ranges (RFC 3501 section
/// 9).
fn sequence_set(uids: &[u32]) -> String {
    let mut sorted = uids.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let mut set = String::new();
    let mut run: Option<(u32, u32)> = None;
    for uid in sorted {
        run = match run {
            Some((first, last)) if last.checked_add(1) == Some(uid) => Some((first, uid)),
            Some(ended) => {
                push_run(&mut set, ended);
                Some((uid, uid))
            }
            None => Some((uid, uid)),
        };
    }
    if let Some(ended) = run {
        push_run(&mut set, ended);
    }
    set
}

fn push_run(set: &mut String, (first, last): (u32, u32)) {
    if !set.is_empty() {
        set.push(',');
    }
    let _ = write!(set, "{first}");
    if last != first {
        let _ = write!(set, ":{last}");
    }
}

/// A part number is digits with dots between them; anything else never
/// reaches a command line.
fn is_part_number(path: &str) -> bool {
    path.split('.')
        .all(|level| !level.is_empty() && level.bytes().all(|byte| byte.is_ascii_digit()))
}

fn command(ask: &PreviewAsk<'_>) -> Result<String, SessionError> {
    if ask.uids.is_empty() || ask.uids.len() > MAX_PREVIEWS {
        return Err(SessionError::Protocol(
            "a preview fetch names no message or too many",
        ));
    }
    let set = sequence_set(ask.uids);
    let text_bytes = ask.text_bytes.min(MAX_PREVIEW_TEXT_BYTES);
    if ask.path.is_empty() {
        return Ok(format!(
            "UID FETCH {set} (UID BODY.PEEK[HEADER.FIELDS ({ENCODING_FIELDS})]<0.{PREVIEW_HEADER_BYTES}> BODY.PEEK[TEXT]<0.{text_bytes}>)"
        ));
    }
    if !is_part_number(ask.path) {
        return Err(SessionError::Protocol("a part number holds other bytes"));
    }
    let path = ask.path;
    Ok(format!(
        "UID FETCH {set} (UID BODY.PEEK[{path}.MIME]<0.{PREVIEW_HEADER_BYTES}> BODY.PEEK[{path}]<0.{text_bytes}>)"
    ))
}

/// The start of the part for every message asked that answered, each
/// once; lines for other UIDs are skipped.
pub(in crate::session) async fn uid_previews(
    selection: &mut Selection<'_>,
    ask: &PreviewAsk<'_>,
    room: Room,
) -> Result<Vec<PreviewBytes>, SessionError> {
    selection.messages()?;
    let command = command(ask)?;
    let mut found = BTreeMap::new();
    let visit = |response: &Response<'_>| {
        if let Response::Fetch(_, attributes) = response
            && let Some(bytes) = line(attributes)
            && ask.uids.contains(&bytes.uid)
        {
            found.entry(bytes.uid).or_insert(bytes);
        }
        Ok(())
    };
    selection
        .collect(&command, room.bounds(MAX_PREVIEWS), visit)
        .await?;
    Ok(found.into_values().collect())
}

/// One FETCH line; `None` without a UID or without the text. What a
/// server sent past the ask is cut.
fn line(attributes: &[AttributeValue<'_>]) -> Option<PreviewBytes> {
    let (mut uid, mut header, mut text) = (None, Vec::new(), None);
    for attribute in attributes {
        match attribute {
            AttributeValue::Uid(value) => uid = Some(*value),
            AttributeValue::BodySection { section, data, .. } => {
                let data = data.as_deref().unwrap_or_default();
                match section {
                    Some(
                        SectionPath::Full(MessageSection::Header)
                        | SectionPath::Part(_, Some(MessageSection::Mime)),
                    ) => header = cut(data, PREVIEW_HEADER_BYTES),
                    Some(SectionPath::Full(MessageSection::Text) | SectionPath::Part(_, None)) => {
                        text = Some(cut(data, MAX_PREVIEW_TEXT_BYTES));
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    Some(PreviewBytes {
        uid: uid?,
        header,
        text: text?,
    })
}

fn cut(data: &[u8], bytes: u32) -> Vec<u8> {
    let keep = usize::try_from(bytes).unwrap_or(usize::MAX);
    data[..data.len().min(keep)].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask<'a>(uids: &'a [u32], path: &'a str) -> PreviewAsk<'a> {
        PreviewAsk {
            uids,
            path,
            text_bytes: 2048,
        }
    }

    #[test]
    fn runs_of_uids_fold_into_ranges_rfc3501_9() {
        assert_eq!(sequence_set(&[9, 5, 12, 13, 14, 5]), "5,9,12:14");
        assert_eq!(sequence_set(&[7]), "7");
        assert_eq!(
            sequence_set(&[u32::MAX - 1, u32::MAX]),
            "4294967294:4294967295"
        );
    }

    #[test]
    fn every_item_of_the_command_is_a_partial_fetch_rfc3501_6_4_5() {
        assert_eq!(
            command(&ask(&[3, 4], "1.2")).unwrap(),
            "UID FETCH 3:4 (UID BODY.PEEK[1.2.MIME]<0.4096> BODY.PEEK[1.2]<0.2048>)"
        );
        assert_eq!(
            command(&ask(&[3], "")).unwrap(),
            "UID FETCH 3 (UID BODY.PEEK[HEADER.FIELDS (CONTENT-TYPE CONTENT-TRANSFER-ENCODING)]<0.4096> BODY.PEEK[TEXT]<0.2048>)"
        );
        let greedy = PreviewAsk {
            text_bytes: u32::MAX,
            ..ask(&[3], "1")
        };
        assert!(
            command(&greedy)
                .unwrap()
                .ends_with("BODY.PEEK[1]<0.65536>)")
        );
    }

    #[test]
    fn a_part_number_with_other_bytes_and_a_set_out_of_bounds_are_refused() {
        for path in ["1.", ".1", "1..2", "1 NOOP", "1]\r\nA9 NOOP", "TEXT"] {
            assert!(command(&ask(&[1], path)).is_err(), "{path}");
        }
        assert!(command(&ask(&[], "1")).is_err());
        let many: Vec<u32> = (1..=u32::try_from(MAX_PREVIEWS).unwrap() + 1).collect();
        assert!(command(&ask(&many, "1")).is_err());
    }

    #[test]
    fn a_line_reads_both_sections_in_either_form_and_cuts_what_passes_the_ask() {
        let read = |bytes: &str| match Response::from_bytes(bytes.as_bytes()).unwrap().1 {
            Response::Fetch(_, attributes) => line(&attributes),
            other => panic!("{other:?}"),
        };
        let part = read("* 1 FETCH (UID 7 BODY[1.MIME]<0> {4}\r\nA: b BODY[1]<0> {2}\r\nhi)\r\n");
        assert_eq!(
            part,
            Some(PreviewBytes {
                uid: 7,
                header: b"A: b".to_vec(),
                text: b"hi".to_vec()
            })
        );
        let whole = read(
            "* 1 FETCH (UID 8 BODY[HEADER.FIELDS (CONTENT-TYPE)]<0> {4}\r\nA: b BODY[TEXT]<0> {2}\r\nhi)\r\n",
        );
        assert_eq!(whole.map(|bytes| bytes.uid), Some(8));
        assert_eq!(
            read("* 1 FETCH (UID 7 BODY[1.MIME]<0> {4}\r\nA: b)\r\n"),
            None
        );
        assert_eq!(cut(&[0; 9], 4).len(), 4);
    }
}
