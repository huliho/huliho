// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The body path of the scripted server: BODYSTRUCTURE with or without
//! a set of header fields, the fields alone and one window of one
//! section, cut as the partial fetch asks (RFC 3501 section 6.4.5).

use std::fmt::Write as _;

use super::super::messages::Message;

/// What one body fetch asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum BodyAsked {
    /// BODYSTRUCTURE, the named header fields beside it where asked,
    /// cut to `bytes`.
    Structure {
        fields: Option<String>,
        bytes: usize,
    },
    /// The named header fields alone, cut to `bytes`.
    Fields { fields: String, bytes: usize },
    /// `bytes` of one section from `offset`; both ends move to a
    /// character border, since the scripted server carries text and a
    /// part on the wire is base64, quoted-printable or ASCII.
    Window {
        section: String,
        offset: usize,
        bytes: usize,
    },
}

/// Reads the items of one body fetch; `None` for any other shape.
pub(super) fn asked(items: &str) -> Option<BodyAsked> {
    let rest = items.strip_prefix("(UID ")?.strip_suffix(')')?;
    if let Some(after) = rest.strip_prefix("BODYSTRUCTURE") {
        let after = after.trim_start();
        if after.is_empty() {
            return Some(BodyAsked::Structure {
                fields: None,
                bytes: usize::MAX,
            });
        }
        let ((section, offset, bytes), tail) = peek(after)?;
        if !tail.is_empty() || offset != 0 {
            return None;
        }
        return Some(BodyAsked::Structure {
            fields: Some(fields_of(&section)?),
            bytes,
        });
    }
    let ((section, offset, bytes), tail) = peek(rest)?;
    if !tail.is_empty() {
        return None;
    }
    match fields_of(&section) {
        Some(fields) if offset == 0 => Some(BodyAsked::Fields { fields, bytes }),
        Some(_) => None,
        None => Some(BodyAsked::Window {
            section,
            offset,
            bytes,
        }),
    }
}

/// One `BODY.PEEK[<section>]<<offset>.<bytes>>` off the front: the
/// section with its window, then what follows.
fn peek(items: &str) -> Option<((String, usize, usize), &str)> {
    let after = items.strip_prefix("BODY.PEEK[")?;
    let (section, tail) = after.split_once("]<")?;
    let (window, tail) = tail.split_once('>')?;
    let (offset, bytes) = window.split_once('.')?;
    Some((
        (
            section.to_owned(),
            offset.parse().ok()?,
            bytes.parse().ok()?,
        ),
        tail.trim_start(),
    ))
}

/// The names inside `HEADER.FIELDS (...)`; `None` for another section.
fn fields_of(section: &str) -> Option<String> {
    section
        .strip_prefix("HEADER.FIELDS (")?
        .strip_suffix(')')
        .map(str::to_owned)
}

/// The line one message answers; `misplaced` names the origin octet
/// every window answer carries, as a server off RFC 3501 section 7.4.2
/// does.
pub(super) fn line(message: &Message, asked: &BodyAsked, misplaced: Option<usize>) -> String {
    let mut items = format!("UID {}", message.uid);
    match asked {
        BodyAsked::Structure { fields, bytes } => {
            let _ = write!(items, " BODYSTRUCTURE {}", message.structure);
            if let Some(fields) = fields {
                items.push(' ');
                items.push_str(&fields_item(message, fields, *bytes));
            }
        }
        BodyAsked::Fields { fields, bytes } => {
            items.push(' ');
            items.push_str(&fields_item(message, fields, *bytes));
        }
        BodyAsked::Window {
            section,
            offset,
            bytes,
        } => {
            let data = message.section(section).unwrap_or_default();
            let start = data.ceil_char_boundary((*offset).min(data.len()));
            let end = data.floor_char_boundary(start.saturating_add(*bytes).min(data.len()));
            let cut = &data[start..end];
            let origin = misplaced.unwrap_or(*offset);
            let _ = write!(
                items,
                " BODY[{section}]<{origin}> {{{}}}\r\n{cut}",
                cut.len()
            );
        }
    }
    format!("* {} FETCH ({items})\r\n", message.uid)
}

/// `BODY[HEADER.FIELDS (...)]` with the named fields of the message's
/// header, whole with their folded lines, then the blank line, cut to
/// `bytes` as the partial fetch asks.
fn fields_item(message: &Message, fields: &str, bytes: usize) -> String {
    let wanted: Vec<&str> = fields.split_whitespace().collect();
    let mut kept = String::new();
    let mut keeping = false;
    for line in message.header.split_inclusive("\r\n") {
        if line.starts_with([' ', '\t']) {
            if keeping {
                kept.push_str(line);
            }
            continue;
        }
        let name = line
            .split_once(':')
            .map(|(name, _)| name)
            .unwrap_or_default();
        keeping = !name.is_empty() && wanted.iter().any(|field| field.eq_ignore_ascii_case(name));
        if keeping {
            kept.push_str(line);
        }
    }
    kept.push_str("\r\n");
    kept.truncate(kept.floor_char_boundary(bytes));
    format!(
        "BODY[HEADER.FIELDS ({fields})] {{{}}}\r\n{kept}",
        kept.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::parts::{CORPUS_AUTHENTICATION, corpus};

    #[test]
    fn the_items_tell_the_three_body_asks_apart_and_leave_the_others() {
        assert_eq!(
            asked("(UID BODYSTRUCTURE)"),
            Some(BodyAsked::Structure {
                fields: None,
                bytes: usize::MAX
            })
        );
        assert_eq!(
            asked("(UID BODYSTRUCTURE BODY.PEEK[HEADER.FIELDS (X-A X-B)]<0.65536>)"),
            Some(BodyAsked::Structure {
                fields: Some("X-A X-B".to_owned()),
                bytes: 65536
            })
        );
        assert_eq!(
            asked("(UID BODY.PEEK[HEADER.FIELDS (X-A)]<0.65536>)"),
            Some(BodyAsked::Fields {
                fields: "X-A".to_owned(),
                bytes: 65536
            })
        );
        assert_eq!(asked("(UID BODY.PEEK[HEADER.FIELDS (X-A)]<8.65536>)"), None);
        assert_eq!(
            asked("(UID BODY.PEEK[1.2]<524288.524288>)"),
            Some(BodyAsked::Window {
                section: "1.2".to_owned(),
                offset: 524_288,
                bytes: 524_288,
            })
        );
        assert_eq!(
            asked("(UID BODY.PEEK[1.MIME]<0.4096> BODY.PEEK[1]<0.2048>)"),
            None
        );
        assert_eq!(asked("(UID FLAGS)"), None);
    }

    #[test]
    fn a_window_cuts_the_section_and_the_fields_keep_the_named_lines_whole() {
        let message = corpus(7);
        let window = BodyAsked::Window {
            section: "1.1".to_owned(),
            offset: 6,
            bytes: 3,
        };
        assert_eq!(
            line(&message, &window, None),
            "* 7 FETCH (UID 7 BODY[1.1]<6> {3}\r\nthe)\r\n"
        );
        assert_eq!(
            line(&message, &window, Some(0)),
            "* 7 FETCH (UID 7 BODY[1.1]<0> {3}\r\nthe)\r\n"
        );
        let past = BodyAsked::Window {
            section: "9".to_owned(),
            offset: 0,
            bytes: 3,
        };
        assert_eq!(
            line(&message, &past, None),
            "* 7 FETCH (UID 7 BODY[9]<0> {0}\r\n)\r\n"
        );
        let fields = line(
            &message,
            &BodyAsked::Fields {
                fields: "authentication-results".to_owned(),
                bytes: 65536,
            },
            None,
        );
        let expected = format!("Authentication-Results: {CORPUS_AUTHENTICATION}\r\n\r\n");
        assert_eq!(
            fields,
            format!(
                "* 7 FETCH (UID 7 BODY[HEADER.FIELDS (authentication-results)] {{{}}}\r\n{expected})\r\n",
                expected.len()
            )
        );
        let cut = line(
            &message,
            &BodyAsked::Fields {
                fields: "authentication-results".to_owned(),
                bytes: 10,
            },
            None,
        );
        assert_eq!(
            cut,
            "* 7 FETCH (UID 7 BODY[HEADER.FIELDS (authentication-results)] {10}\r\nAuthentica)\r\n"
        );
        let structure = line(
            &message,
            &BodyAsked::Structure {
                fields: None,
                bytes: usize::MAX,
            },
            None,
        );
        assert!(structure.starts_with("* 7 FETCH (UID 7 BODYSTRUCTURE ((("));
    }
}
