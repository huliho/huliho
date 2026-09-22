// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The UID FETCH answers of the scripted server: the header items of
//! the sync, the flags alone and the start of a text part, each cut as
//! a partial fetch asks (RFC 3501 section 6.4.5), the Gmail items where
//! the extension is on.

use std::fmt::Write as _;

use super::folder::Folder;
use super::mailboxes::{Extension, Mailboxes, Refusal, quote};
use super::messages::{Behavior, Message};

/// What one UID FETCH asked for.
enum Asked {
    /// The header items of the sync, BODYSTRUCTURE and the Gmail items
    /// where named.
    Headers {
        fields: String,
        structure: bool,
        gmail: bool,
    },
    /// The flags alone, every message or the ones changed since a
    /// mod-sequence, the labels next to them where named.
    Flags {
        changed_since: Option<u64>,
        labels: bool,
    },
    /// The start of a text part: each section with the bytes asked.
    Preview { sections: Vec<(String, usize)> },
}

/// `<set> (<items>)[ (CHANGEDSINCE n)]`: a line per message in the set
/// the items apply to. `Bad` for a shape the server does not know and
/// for a Gmail item behind an absent extension.
pub(super) fn answer(
    folder: &Folder,
    rest: &str,
    mailboxes: &Mailboxes,
) -> Result<String, Refusal> {
    let (set, items) = rest.split_once(' ').ok_or(Refusal::Bad)?;
    let asked = asked(items).ok_or(Refusal::Bad)?;
    if items.contains("X-GM-") && !mailboxes.has(Extension::Gmail) {
        return Err(Refusal::Bad);
    }
    let behavior = mailboxes.behavior;
    let condstore = mailboxes.has(Extension::Condstore);
    let top = folder.top();
    let found: Vec<&Message> = folder
        .mail
        .iter()
        .filter(|message| in_set(set, message.uid, top))
        .collect();
    let mut lines = String::new();
    match &asked {
        Asked::Headers { .. } => headers(&mut lines, &found, &asked, behavior),
        Asked::Flags {
            changed_since,
            labels,
        } => {
            for message in found {
                if changed_since.is_none_or(|since| message.modseq > since) {
                    lines.push_str(&flag_line(message, (condstore, *labels), behavior));
                }
            }
        }
        Asked::Preview { sections } => {
            for message in found {
                lines.push_str(&preview_line(message, sections));
            }
        }
    }
    Ok(lines)
}

/// Reads the items into what was asked.
fn asked(items: &str) -> Option<Asked> {
    let flags = items
        .strip_prefix("(UID FLAGS)")
        .map(|rest| (false, rest))
        .or_else(|| {
            items
                .strip_prefix("(UID FLAGS X-GM-LABELS)")
                .map(|rest| (true, rest))
        });
    if let Some((labels, rest)) = flags {
        let changed_since = rest
            .trim()
            .strip_prefix("(CHANGEDSINCE ")
            .and_then(|tail| tail.strip_suffix(')'))
            .and_then(|value| value.parse().ok());
        return Some(Asked::Flags {
            changed_since,
            labels,
        });
    }
    let sections = peeks(items);
    if items.contains("INTERNALDATE") {
        let fields = items.split_once("HEADER.FIELDS (")?.1.split_once(')')?.0;
        return Some(Asked::Headers {
            fields: fields.to_owned(),
            structure: items.contains("BODYSTRUCTURE"),
            gmail: items.contains("X-GM-MSGID"),
        });
    }
    (sections.len() == 2).then_some(Asked::Preview { sections })
}

/// Every `BODY.PEEK[<section>]<0.<bytes>>` of the items.
fn peeks(items: &str) -> Vec<(String, usize)> {
    let mut found = Vec::new();
    let mut rest = items;
    while let Some((_, after)) = rest.split_once("BODY.PEEK[") {
        let Some((section, tail)) = after.split_once("]<0.") else {
            break;
        };
        let Some((bytes, tail)) = tail.split_once('>') else {
            break;
        };
        if let Ok(bytes) = bytes.parse() {
            found.push((section.to_owned(), bytes));
        }
        rest = tail;
    }
    found
}

/// Whether a UID is in a sequence set of ranges and single numbers;
/// `*` is the highest UID the folder holds (RFC 3501 section 9).
fn in_set(set: &str, uid: u32, top: u32) -> bool {
    set.split(',').any(|part| {
        let number = |text: &str| {
            if text == "*" {
                Some(top)
            } else {
                text.parse().ok()
            }
        };
        match part.split_once(':') {
            Some((low, high)) => match (number(low), number(high)) {
                (Some(low), Some(high)) => (low.min(high)..=low.max(high)).contains(&uid),
                _ => false,
            },
            None => number(part) == Some(uid),
        }
    })
}

/// The header items with the lines a misbehaving server adds.
fn headers(lines: &mut String, found: &[&Message], asked: &Asked, behavior: Behavior) {
    let first = found.first().filter(|_| behavior.volunteered > 0);
    if let Some(first) = first {
        for _ in 0..behavior.volunteered {
            lines.push_str("* 9 EXISTS\r\n");
        }
        let _ = write!(
            lines,
            "* 3 EXPUNGE\r\n* 1 FETCH (UID {} FLAGS (\\Flagged))\r\n",
            first.uid
        );
        let outside =
            Message::new(found.iter().map(|message| message.uid).max().unwrap_or(0) + 1000);
        lines.push_str(&header_line(&outside, asked, behavior));
    }
    for message in found {
        lines.push_str(&header_line(message, asked, behavior));
    }
    if let Some(first) = first {
        let again = (*first).clone().flagged(&["\\Flagged"]);
        lines.push_str(&header_line(&again, asked, behavior));
    }
}

fn header_line(message: &Message, asked: &Asked, behavior: Behavior) -> String {
    let Asked::Headers {
        fields,
        structure,
        gmail,
    } = asked
    else {
        return String::new();
    };
    let mut items = format!(
        "UID {} FLAGS ({}) INTERNALDATE \"{}\" RFC822.SIZE {}",
        message.uid,
        message.flags.join(" "),
        message.internal_date,
        message.size
    );
    if let Some(modseq) = behavior.fetch_modseq {
        let _ = write!(items, " MODSEQ ({modseq})");
    }
    if *structure {
        let _ = write!(items, " BODYSTRUCTURE {}", message.structure);
    }
    if *gmail || behavior.gmail_items {
        let _ = write!(
            items,
            " {} X-GM-MSGID {} X-GM-THRID {}",
            labels_item(message),
            message.msgid,
            message.thrid
        );
    }
    format!(
        "* {} FETCH ({items} BODY[HEADER.FIELDS ({fields})] {{{}}}\r\n{})\r\n",
        message.uid,
        message.header.len(),
        message.header
    )
}

/// The flags of one message, with its mod-sequence where CONDSTORE is
/// on (RFC 7162 section 3.1.4.1) and its labels where asked or where
/// the behavior volunteers them.
fn flag_line(message: &Message, (condstore, labels): (bool, bool), behavior: Behavior) -> String {
    let mut items = format!("UID {} FLAGS ({})", message.uid, message.flags.join(" "));
    if condstore {
        let modseq = behavior.fetch_modseq.unwrap_or(message.modseq);
        let _ = write!(items, " MODSEQ ({modseq})");
    }
    if labels || behavior.gmail_items {
        let _ = write!(items, " {}", labels_item(message));
    }
    format!("* {} FETCH ({items})\r\n", message.uid)
}

/// `X-GM-LABELS (...)`: a system label as an atom, a user label as a
/// quoted string, as Gmail sends them.
fn labels_item(message: &Message) -> String {
    let labels: Vec<String> = message
        .labels
        .iter()
        .map(|label| {
            if label.starts_with('\\') {
                label.clone()
            } else {
                quote(label)
            }
        })
        .collect();
    format!("X-GM-LABELS ({})", labels.join(" "))
}

/// The two sections of a preview ask, each cut to the bytes asked: a
/// MIME header or the header fields, then the text.
fn preview_line(message: &Message, sections: &[(String, usize)]) -> String {
    let mut items = format!("UID {}", message.uid);
    for (section, bytes) in sections {
        let mime = section
            .rsplit_once('.')
            .is_some_and(|(_, tail)| tail.eq_ignore_ascii_case("MIME"));
        let data = if mime || section.starts_with("HEADER.FIELDS") {
            message.mime_header()
        } else {
            message.body.clone()
        };
        let end = data.len().min(*bytes);
        let cut = &data[..end];
        let _ = write!(items, " BODY[{section}]<0> {{{}}}\r\n{cut}", cut.len());
    }
    format!("* {} FETCH ({items})\r\n", message.uid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sequence_set_reads_ranges_numbers_and_the_star_rfc3501_9() {
        assert!(in_set("5,9,12:14", 13, 20));
        assert!(in_set("5,9,12:14", 5, 20));
        assert!(!in_set("5,9,12:14", 6, 20));
        assert!(in_set("1:*", 20, 20));
        assert!(!in_set("1:*", 21, 20));
        assert!(in_set("9:3", 4, 20));
    }

    #[test]
    fn the_items_tell_the_three_asks_apart() {
        assert!(matches!(
            asked("(UID FLAGS) (CHANGEDSINCE 41)"),
            Some(Asked::Flags {
                changed_since: Some(41),
                labels: false,
            })
        ));
        assert!(matches!(
            asked("(UID FLAGS X-GM-LABELS) (CHANGEDSINCE 41)"),
            Some(Asked::Flags {
                changed_since: Some(41),
                labels: true,
            })
        ));
        assert!(matches!(
            asked("(UID FLAGS)"),
            Some(Asked::Flags {
                changed_since: None,
                labels: false,
            })
        ));
        let sync =
            "(UID FLAGS INTERNALDATE RFC822.SIZE BODYSTRUCTURE BODY.PEEK[HEADER.FIELDS (FROM TO)])";
        assert!(matches!(
            asked(sync),
            Some(Asked::Headers {
                structure: true,
                gmail: false,
                ..
            })
        ));
        let gmail = "(UID FLAGS INTERNALDATE RFC822.SIZE X-GM-LABELS X-GM-MSGID X-GM-THRID BODY.PEEK[HEADER.FIELDS (FROM TO)])";
        assert!(matches!(
            asked(gmail),
            Some(Asked::Headers {
                structure: false,
                gmail: true,
                ..
            })
        ));
        let preview = "(UID BODY.PEEK[1.MIME]<0.4096> BODY.PEEK[1]<0.2048>)";
        match asked(preview) {
            Some(Asked::Preview { sections }) => assert_eq!(
                sections,
                [("1.MIME".to_owned(), 4096), ("1".to_owned(), 2048)]
            ),
            _ => panic!("a preview ask"),
        }
        assert!(asked("(UID)").is_none());
    }

    #[test]
    fn a_preview_line_cuts_each_section_to_the_bytes_asked_rfc3501_6_4_5() {
        let message = Message::new(7);
        let line = preview_line(&message, &[("TEXT".to_owned(), 4)]);
        assert_eq!(line, "* 7 FETCH (UID 7 BODY[TEXT]<0> {4}\r\nBody)\r\n");
    }

    #[test]
    fn the_labels_item_writes_a_system_label_as_an_atom_and_a_user_label_quoted() {
        let message = Message::new(7).labeled(&["\\Inbox", "Work/Q3", "Say \"hi\""]);
        assert_eq!(
            labels_item(&message),
            "X-GM-LABELS (\\Inbox \"Work/Q3\" \"Say \\\"hi\\\"\")"
        );
        assert_eq!(labels_item(&Message::new(8).labeled(&[])), "X-GM-LABELS ()");
    }
}
