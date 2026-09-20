// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! LIST, LSUB and STATUS: the mailbox names with their attributes and
//! counts, each answer read under its own line limit.

use async_imap::imap_proto::{MailboxDatum, NameAttribute, Response, StatusAttribute};

use super::{Room, Selection, modseq, quoted, unescaped, wire_name};
use crate::session::{ListEntry, ListReturn, Listing, SessionError, StatusEntry, StatusItems};

/// The mailboxes one account may list. A listing past it fails, since
/// a cut listing would destroy the rows of the names it leaves out.
const MAX_MAILBOXES: usize = 10_000;

/// The lines of its own one LIST or LSUB answer may carry: a LIST line
/// and a STATUS line per mailbox (RFC 5819).
const MAX_LIST_LINES: usize = 2 * MAX_MAILBOXES;

/// The lines of its own one STATUS answer carries.
const STATUS_LINES: usize = 1;

/// The attribute words one LIST line keeps: the IANA registry of
/// mailbox name attributes holds under twenty.
const MAX_ATTRIBUTES: usize = 32;

/// The longest attribute word kept: the longest registered one,
/// `\HasNoChildren`, has 14 bytes.
const MAX_ATTRIBUTE_BYTES: usize = 64;

/// The LIST command with its RETURN options.
fn list_command(options: ListReturn) -> String {
    let mut returned = Vec::new();
    if options.subscribed {
        returned.push("SUBSCRIBED".to_owned());
    }
    if options.special_use {
        returned.push("SPECIAL-USE".to_owned());
    }
    if let Some(items) = options.status {
        returned.push(format!("STATUS ({})", status_items(items)));
    }
    if returned.is_empty() {
        return "LIST \"\" \"*\"".to_owned();
    }
    format!("LIST \"\" \"*\" RETURN ({})", returned.join(" "))
}

fn status_items(items: StatusItems) -> &'static str {
    if items.modseq {
        "MESSAGES UNSEEN UIDNEXT UIDVALIDITY HIGHESTMODSEQ"
    } else {
        "MESSAGES UNSEEN UIDNEXT UIDVALIDITY"
    }
}

/// The listing without the names that are not sendable; past
/// `MAX_MAILBOXES` entries or STATUS lines it fails.
pub(in crate::session) async fn list(
    selection: &mut Selection<'_>,
    options: ListReturn,
    room: Room,
) -> Result<Listing, SessionError> {
    let bounds = room.bounds(MAX_LIST_LINES);
    let mut listing = Listing::default();
    selection
        .collect(&list_command(options), bounds, |response| {
            keep(&mut listing, response)
        })
        .await?;
    Ok(listing)
}

/// One line of a LIST answer into the listing: an entry, a STATUS line
/// riding along (RFC 5819) or nothing.
pub(super) fn keep(listing: &mut Listing, response: &Response<'_>) -> Result<(), SessionError> {
    match response {
        Response::MailboxData(MailboxDatum::List {
            name_attributes,
            delimiter,
            name,
        }) => hold(
            &mut listing.entries,
            entry(name_attributes, delimiter.as_deref(), name),
        ),
        Response::MailboxData(MailboxDatum::Status { mailbox, status }) => {
            hold(&mut listing.statuses, status_entry(mailbox, status)?)
        }
        _ => Ok(()),
    }
}

/// Adds what one line gave, if anything; past `MAX_MAILBOXES` held ones
/// it fails.
fn hold<T>(held: &mut Vec<T>, found: Option<T>) -> Result<(), SessionError> {
    if found.is_some() && held.len() == MAX_MAILBOXES {
        return Err(SessionError::Protocol(
            "the listing passes the mailbox limit",
        ));
    }
    held.extend(found);
    Ok(())
}

/// The subscribed names that are sendable; past `MAX_MAILBOXES` of them
/// it fails.
pub(in crate::session) async fn lsub(
    selection: &mut Selection<'_>,
    room: Room,
) -> Result<Vec<String>, SessionError> {
    let bounds = room.bounds(MAX_LIST_LINES);
    let mut names = Vec::new();
    selection
        .collect("LSUB \"\" \"*\"", bounds, |response| {
            if let Response::MailboxData(MailboxDatum::List { name, .. }) = response {
                return hold(&mut names, wire_name(name));
            }
            Ok(())
        })
        .await?;
    Ok(names)
}

pub(in crate::session) async fn status(
    selection: &mut Selection<'_>,
    mailbox: &str,
    items: StatusItems,
    room: Room,
) -> Result<StatusEntry, SessionError> {
    let command = format!("STATUS {} ({})", quoted(mailbox)?, status_items(items));
    let mut found = None;
    let visit = |response: &Response<'_>| {
        if let Response::MailboxData(MailboxDatum::Status { mailbox, status }) = response
            && let Some(entry) = status_entry(mailbox, status)?
        {
            found = Some(entry);
        }
        Ok(())
    };
    selection
        .collect(&command, room.bounds(STATUS_LINES), visit)
        .await?;
    found.ok_or(SessionError::Protocol("STATUS answered without data"))
}

/// One LIST or LSUB line, `None` when its name is not sendable. Of its
/// attribute words the first `MAX_ATTRIBUTES` count.
fn entry(
    attributes: &[NameAttribute<'_>],
    delimiter: Option<&str>,
    name: &str,
) -> Option<ListEntry> {
    Some(ListEntry {
        name: wire_name(name)?,
        delimiter: delimiter.and_then(|delimiter| unescaped(delimiter).chars().next()),
        attributes: attributes
            .iter()
            .take(MAX_ATTRIBUTES)
            .filter_map(attribute_name)
            .collect(),
    })
}

/// Every attribute as the wire spells it, upper-cased, so the mapping
/// treats the standard ones and the extensions alike; `None` for a
/// kind the parser knows and this list does not and for a word above
/// `MAX_ATTRIBUTE_BYTES`, which is never copied.
fn attribute_name(attribute: &NameAttribute<'_>) -> Option<String> {
    let name = match attribute {
        NameAttribute::NoInferiors => "\\NOINFERIORS",
        NameAttribute::NoSelect => "\\NOSELECT",
        NameAttribute::Marked => "\\MARKED",
        NameAttribute::Unmarked => "\\UNMARKED",
        NameAttribute::All => "\\ALL",
        NameAttribute::Archive => "\\ARCHIVE",
        NameAttribute::Drafts => "\\DRAFTS",
        NameAttribute::Flagged => "\\FLAGGED",
        NameAttribute::Junk => "\\JUNK",
        NameAttribute::Sent => "\\SENT",
        NameAttribute::Trash => "\\TRASH",
        NameAttribute::Extension(other) => {
            return (other.len() <= MAX_ATTRIBUTE_BYTES).then(|| other.to_ascii_uppercase());
        }
        _ => return None,
    };
    Some(name.to_owned())
}

/// One STATUS line, `None` when its name is not sendable; a
/// HIGHESTMODSEQ past 63 bits fails the answer (RFC 7162 section 3.1).
fn status_entry(
    mailbox: &str,
    items: &[StatusAttribute],
) -> Result<Option<StatusEntry>, SessionError> {
    let Some(mailbox) = wire_name(mailbox) else {
        return Ok(None);
    };
    let mut entry = StatusEntry {
        mailbox,
        ..StatusEntry::default()
    };
    for item in items {
        match item {
            StatusAttribute::Messages(count) => entry.messages = Some(*count),
            StatusAttribute::Unseen(count) => entry.unseen = Some(*count),
            StatusAttribute::UidNext(uid) => entry.uid_next = Some(*uid),
            StatusAttribute::UidValidity(validity) => entry.uid_validity = Some(*validity),
            StatusAttribute::HighestModSeq(value) => entry.highest_modseq = Some(modseq(*value)?),
            _ => {}
        }
    }
    Ok(Some(entry))
}

#[cfg(test)]
mod tests {
    use super::super::MAX_WIRE_NAME_BYTES;
    use super::*;

    /// One LIST or LSUB line through the parser, then through the
    /// reader; `None` when the reader leaves it out.
    fn listed(line: &[u8]) -> Option<ListEntry> {
        match Response::from_bytes(line).unwrap().1 {
            Response::MailboxData(MailboxDatum::List {
                name_attributes,
                delimiter,
                name,
            }) => entry(&name_attributes, delimiter.as_deref(), &name),
            other => panic!("{other:?}"),
        }
    }

    /// The same for one STATUS line.
    fn counted(line: &[u8]) -> Option<StatusEntry> {
        match Response::from_bytes(line).unwrap().1 {
            Response::MailboxData(MailboxDatum::Status { mailbox, status }) => {
                status_entry(&mailbox, &status).unwrap()
            }
            other => panic!("{other:?}"),
        }
    }

    /// A name as a literal, the one form that carries any byte.
    fn literal(name: &str) -> String {
        format!("{{{}}}\r\n{name}", name.len())
    }

    #[test]
    fn the_list_command_carries_only_the_options_asked_for_rfc5258_3() {
        assert_eq!(list_command(ListReturn::default()), "LIST \"\" \"*\"");
        let all = ListReturn {
            subscribed: true,
            special_use: true,
            status: Some(StatusItems { modseq: true }),
        };
        assert_eq!(
            list_command(all),
            "LIST \"\" \"*\" RETURN (SUBSCRIBED SPECIAL-USE STATUS (MESSAGES UNSEEN UIDNEXT UIDVALIDITY HIGHESTMODSEQ))"
        );
        let plain_status = ListReturn {
            status: Some(StatusItems { modseq: false }),
            ..ListReturn::default()
        };
        assert_eq!(
            list_command(plain_status),
            "LIST \"\" \"*\" RETURN (STATUS (MESSAGES UNSEEN UIDNEXT UIDVALIDITY))"
        );
    }

    #[test]
    fn a_line_whose_name_is_not_sendable_is_left_out() {
        let long = "a".repeat(MAX_WIRE_NAME_BYTES + 1);
        for name in ["a\r\nA9 NOOP", "a\nb", &long] {
            let list = format!("* LIST () \"/\" {}\r\n", literal(name));
            assert_eq!(listed(list.as_bytes()), None);
            let lsub = format!("* LSUB () \"/\" {}\r\n", literal(name));
            assert_eq!(listed(lsub.as_bytes()), None);
            let status = format!("* STATUS {} (MESSAGES 1)\r\n", literal(name));
            assert_eq!(counted(status.as_bytes()), None);
        }
        let quoted_long = format!("* LIST () \"/\" \"{long}\"\r\n");
        assert_eq!(listed(quoted_long.as_bytes()), None);
    }

    #[test]
    fn a_status_line_past_the_mailbox_limit_fails_the_listing() {
        let mut listing = Listing::default();
        let mut kept = |number: usize| {
            let line = format!("* STATUS \"F{number}\" (MESSAGES 1)\r\n");
            keep(
                &mut listing,
                &Response::from_bytes(line.as_bytes()).unwrap().1,
            )
        };
        for number in 0..MAX_MAILBOXES {
            kept(number).unwrap();
        }
        assert!(matches!(
            kept(MAX_MAILBOXES),
            Err(SessionError::Protocol(
                "the listing passes the mailbox limit"
            ))
        ));
    }

    #[test]
    fn a_list_line_keeps_its_first_words_up_to_the_limit_and_none_past_the_byte_bound() {
        let mut words: Vec<String> = (0..MAX_ATTRIBUTES + 8)
            .map(|number| format!("\\X{number}"))
            .collect();
        words[1] = format!("\\{}", "Y".repeat(MAX_ATTRIBUTE_BYTES));
        words[2] = format!("\\{}", "Z".repeat(MAX_ATTRIBUTE_BYTES - 1));
        let line = format!("* LIST ({}) \"/\" \"INBOX\"\r\n", words.join(" "));
        let entry = listed(line.as_bytes()).unwrap();
        words.truncate(MAX_ATTRIBUTES);
        words.remove(1);
        assert_eq!(entry.attributes, words);
    }

    #[test]
    fn a_list_line_keeps_every_attribute_as_an_upper_case_word() {
        let entry =
            listed(b"* LIST (\\HasNoChildren \\Subscribed \\Sent) \"/\" \"Sent Items\"\r\n")
                .unwrap();
        assert_eq!(entry.name, "Sent Items");
        assert_eq!(entry.delimiter, Some('/'));
        assert_eq!(
            entry.attributes,
            ["\\HASNOCHILDREN", "\\SUBSCRIBED", "\\SENT"]
        );
        let flat = listed(b"* LIST (\\Noselect) NIL \"[Gmail]\"\r\n").unwrap();
        assert_eq!(flat.delimiter, None);
        assert_eq!(flat.attributes, ["\\NOSELECT"]);
        let lsub = listed(b"* LSUB () \".\" \"INBOX.Work\"\r\n").unwrap();
        assert_eq!(
            (lsub.name.as_str(), lsub.delimiter),
            ("INBOX.Work", Some('.'))
        );
        let escaped = listed(b"* LIST () \"\\\\\" \"a\\\"b\\\\c\"\r\n").unwrap();
        assert_eq!(
            (escaped.name.as_str(), escaped.delimiter),
            ("a\"b\\c", Some('\\'))
        );
        let folded = listed(b"* LIST () \"/\" {3}\r\na\\b\r\n").unwrap();
        assert_eq!(folded.name, "a\\b");
        let pairs = listed(b"* LIST () \"/\" \"a\\\\\\\\b\"\r\n").unwrap();
        assert_eq!(pairs.name, "a\\\\b");
    }

    #[test]
    fn a_status_line_reads_every_item_and_leaves_the_rest_none_rfc7162_3_1_7() {
        let status = counted(
            b"* STATUS \"&U,BTFw-\" (MESSAGES 17 UNSEEN 3 UIDNEXT 18 UIDVALIDITY 5 HIGHESTMODSEQ 9)\r\n",
        )
        .unwrap();
        assert_eq!(
            status,
            StatusEntry {
                mailbox: "&U,BTFw-".to_owned(),
                messages: Some(17),
                unseen: Some(3),
                uid_next: Some(18),
                uid_validity: Some(5),
                highest_modseq: Some(9),
            }
        );
        let partial = counted(b"* STATUS INBOX (MESSAGES 2)\r\n").unwrap();
        assert_eq!(partial.messages, Some(2));
        assert_eq!(partial.unseen, None);
        let escaped = counted(b"* STATUS \"a\\\"b\" (MESSAGES 1)\r\n").unwrap();
        assert_eq!(escaped.mailbox, "a\"b");
    }

    #[test]
    fn a_highestmodseq_past_63_bits_fails_the_line_rfc7162_3_1() {
        let line = |value: u64| format!("* STATUS INBOX (HIGHESTMODSEQ {value})\r\n");
        let read = |line: String| match Response::from_bytes(line.as_bytes()).unwrap().1 {
            Response::MailboxData(MailboxDatum::Status { mailbox, status }) => {
                status_entry(&mailbox, &status)
            }
            other => panic!("{other:?}"),
        };
        let top = read(line(i64::MAX.unsigned_abs())).unwrap().unwrap();
        assert_eq!(top.highest_modseq, Some(i64::MAX.unsigned_abs()));
        assert!(matches!(
            read(line(i64::MAX.unsigned_abs() + 1)),
            Err(SessionError::Protocol("a mod-sequence passes 63 bits"))
        ));
    }
}
