// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! EXAMINE, SELECT and the UID list: a mailbox opened read-only or for
//! writing and the UIDs it holds, asked for in windows of sequence
//! numbers so no answer grows with the folder.

use async_imap::imap_proto::{MailboxDatum, Response, ResponseCode, Status};

use super::{Room, Selection, modseq, quoted};
use crate::session::{Selected, SessionError, Writable};

/// The lines of its own an EXAMINE or a SELECT answer carries: FLAGS,
/// EXISTS, RECENT and the OK lines with UNSEEN, PERMANENTFLAGS, UIDNEXT,
/// UIDVALIDITY and HIGHESTMODSEQ (RFC 3501 section 6.3.1, RFC 7162).
const OPEN_LINES: usize = 8;

/// The SEARCH line of one window.
const SEARCH_LINES: usize = 1;

/// The sequence numbers one UID SEARCH names. That many UIDs of ten
/// digits take 55 KB, which stays under the structure bound of a
/// response.
const SEARCH_WINDOW: u32 = 5000;

/// The UIDs one folder may hold, 12 MB as a list.
const MAX_FOLDER_UIDS: usize = 3_000_000;

/// The fixed words of a folder past `MAX_FOLDER_UIDS`.
const UID_LIMIT: &str = "the folder passes the UID limit";

/// The flags one PERMANENTFLAGS code may name; a mailbox carries a few
/// dozen keywords.
const MAX_PERMANENT_FLAGS: usize = 1024;

/// How a mailbox is opened. The read path examines, so it never clears
/// `\Recent` and never holds write access; the write path selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::session) enum Access {
    ReadOnly,
    ReadWrite,
}

impl Access {
    fn verb(self) -> &'static str {
        match self {
            Self::ReadOnly => "EXAMINE",
            Self::ReadWrite => "SELECT",
        }
    }
}

/// What an EXAMINE or a SELECT answered, item by item.
#[derive(Debug, Default)]
pub(in crate::session) struct Opened {
    uid_validity: Option<u32>,
    highest_modseq: Option<u64>,
    uid_next: Option<u32>,
    messages: Option<u32>,
    permanent: Option<Vec<String>>,
}

impl Opened {
    /// Keeps what one untagged line says about the mailbox. The
    /// permanent flags count for a SELECT alone: the read path passes
    /// them over, whatever their number.
    pub(super) fn note(
        &mut self,
        response: &Response<'_>,
        access: Access,
    ) -> Result<(), SessionError> {
        match response {
            Response::Data {
                status: Status::Ok,
                code: Some(code),
                ..
            } => match code {
                ResponseCode::UidValidity(value) => self.uid_validity = Some(*value),
                ResponseCode::UidNext(value) => self.uid_next = Some(*value),
                ResponseCode::HighestModSeq(value) => self.highest_modseq = Some(modseq(*value)?),
                ResponseCode::PermanentFlags(flags) if access == Access::ReadWrite => {
                    self.permanent = Some(permanent(flags)?);
                }
                _ => {}
            },
            Response::MailboxData(MailboxDatum::Exists(count)) => self.messages = Some(*count),
            _ => {}
        }
        Ok(())
    }

    /// The mailbox as the read path takes it from an EXAMINE.
    pub(in crate::session) fn selected(&self) -> Result<Selected, SessionError> {
        let messages = self
            .messages
            .ok_or(SessionError::Protocol("EXAMINE answered without EXISTS"))?;
        let uid_validity = self.uid_validity.ok_or(SessionError::Protocol(
            "EXAMINE answered without UIDVALIDITY",
        ))?;
        Ok(Selected {
            uid_validity,
            highest_modseq: self.highest_modseq,
            uid_next: self.uid_next,
            messages,
        })
    }

    /// The mailbox as the write path takes it from a SELECT.
    pub(in crate::session) fn writable(self) -> Result<Writable, SessionError> {
        let uid_validity = self.uid_validity.ok_or(SessionError::Protocol(
            "SELECT answered without UIDVALIDITY",
        ))?;
        Ok(Writable {
            uid_validity,
            permanent: self.permanent,
        })
    }
}

/// The flags of a PERMANENTFLAGS code, `MAX_PERMANENT_FLAGS` at most.
fn permanent<T: ToString>(flags: &[T]) -> Result<Vec<String>, SessionError> {
    if flags.len() > MAX_PERMANENT_FLAGS {
        return Err(SessionError::Protocol(
            "the mailbox names too many permanent flags",
        ));
    }
    Ok(flags.iter().map(ToString::to_string).collect())
}

/// EXAMINE or SELECT of one mailbox by its wire name (RFC 3501 sections
/// 6.3.1 and 6.3.2).
pub(in crate::session) async fn open(
    selection: &mut Selection<'_>,
    access: Access,
    mailbox: &str,
    room: Room,
) -> Result<Opened, SessionError> {
    let command = format!("{} {}", access.verb(), quoted(mailbox)?);
    let mut opened = Opened::default();
    let visit = |response: &Response<'_>| opened.note(response, access);
    selection
        .collect(&command, room.bounds(OPEN_LINES), visit)
        .await?;
    Ok(opened)
}

/// Every UID of the selected mailbox, highest first, each once: one
/// `UID SEARCH <low>:<high>` per window of sequence numbers from the top
/// down. An expunge only moves a message down, so none is missed; one
/// that moved into the next window shows up twice and is dropped.
pub(in crate::session) async fn uid_list(
    selection: &mut Selection<'_>,
    room: Room,
) -> Result<Vec<u32>, SessionError> {
    let mut top = start(selection.messages()?)?;
    let mut uids = Vec::new();
    while let Some((low, high)) = window(top, selection.messages()?) {
        let command = format!("UID SEARCH {low}:{high}");
        let bounds = room.bounds(SEARCH_LINES);
        let visit = |response: &Response<'_>| {
            if let Response::MailboxData(MailboxDatum::Search(found)) = response {
                hold(&mut uids, found)?;
            }
            Ok(())
        };
        selection.collect(&command, bounds, visit).await?;
        top = low - 1;
    }
    uids.sort_unstable_by(|a, b| b.cmp(a));
    uids.dedup();
    Ok(uids)
}

/// Where the walk starts: at the count, which may not pass the UID
/// limit, so a server cannot buy commands with a large EXISTS.
fn start(messages: u32) -> Result<u32, SessionError> {
    if usize::try_from(messages).is_ok_and(|messages| messages <= MAX_FOLDER_UIDS) {
        Ok(messages)
    } else {
        Err(SessionError::Protocol(UID_LIMIT))
    }
}

/// The window below `top`, `None` once nothing is left. It never names
/// a sequence number above the count, which would earn a BAD (RFC 3501
/// section 9).
fn window(top: u32, messages: u32) -> Option<(u32, u32)> {
    let high = top.min(messages);
    (high > 0).then(|| (high.saturating_sub(SEARCH_WINDOW - 1).max(1), high))
}

/// Adds the UIDs of one SEARCH line; past `MAX_FOLDER_UIDS` it fails. A
/// UID is never zero (RFC 3501 section 2.3.1.1), so a zero is dropped
/// and no range ever starts at it.
fn hold(uids: &mut Vec<u32>, found: &[u32]) -> Result<(), SessionError> {
    if uids.len() + found.len() > MAX_FOLDER_UIDS {
        return Err(SessionError::Protocol(UID_LIMIT));
    }
    uids.extend(found.iter().filter(|uid| **uid != 0));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::MAX_STRUCTURED_BYTES;

    /// The bytes one UID takes in a SEARCH answer at most: a space and
    /// ten digits.
    const UID_WIRE_BYTES: usize = 11;

    #[test]
    fn a_window_of_full_width_uids_stays_under_the_structure_bound() {
        let window = usize::try_from(SEARCH_WINDOW).unwrap();
        let answer = "* SEARCH\r\n".len() + window * UID_WIRE_BYTES;
        assert!(answer <= MAX_STRUCTURED_BYTES, "{answer}");
    }

    #[test]
    fn the_windows_walk_down_from_the_top_and_never_pass_the_count_rfc3501_9() {
        assert_eq!(window(12_000, 12_000), Some((7001, 12_000)));
        assert_eq!(window(7000, 12_000), Some((2001, 7000)));
        assert_eq!(window(2000, 12_000), Some((1, 2000)));
        assert_eq!(window(7000, 300), Some((1, 300)));
        assert_eq!(window(0, 12_000), None);
        assert_eq!(window(7000, 0), None);
    }

    #[test]
    fn a_count_past_the_uid_limit_starts_no_walk() {
        let limit = u32::try_from(MAX_FOLDER_UIDS).unwrap();
        assert_eq!(start(limit).unwrap(), limit);
        for messages in [limit + 1, u32::MAX] {
            assert!(matches!(
                start(messages),
                Err(SessionError::Protocol("the folder passes the UID limit"))
            ));
        }
    }

    #[test]
    fn a_list_past_the_uid_limit_fails() {
        let mut uids = vec![1; MAX_FOLDER_UIDS - 1];
        hold(&mut uids, &[2]).unwrap();
        assert!(matches!(
            hold(&mut uids, &[3]),
            Err(SessionError::Protocol("the folder passes the UID limit"))
        ));
    }

    #[test]
    fn a_uid_of_zero_never_enters_the_list_rfc3501_2_3_1_1() {
        let mut uids = Vec::new();
        hold(&mut uids, &[0, 7, 0, 9]).unwrap();
        assert_eq!(uids, [7, 9]);
    }

    /// What these lines open under that access.
    fn opened(lines: &[&str], access: Access) -> Result<Opened, SessionError> {
        let mut opened = Opened::default();
        for line in lines {
            let response = Response::from_bytes(line.as_bytes()).unwrap().1;
            opened.note(&response, access)?;
        }
        Ok(opened)
    }

    /// What a SELECT answering these lines opens.
    fn selected(lines: &[&str]) -> Result<Writable, SessionError> {
        opened(lines, Access::ReadWrite)?.writable()
    }

    #[test]
    fn a_select_names_the_flags_a_store_changes_for_good_rfc3501_7_1() {
        let validity = "* OK [UIDVALIDITY 7] UIDs valid\r\n";
        let named = selected(&[
            "* OK [PERMANENTFLAGS (\\Answered \\Seen $Forwarded)] Limited\r\n",
            validity,
        ])
        .unwrap();
        assert_eq!(named.uid_validity, 7);
        assert!(named.keeps("\\Seen") && named.keeps("\\seen"));
        assert!(named.keeps("$forwarded"));
        assert!(!named.keeps("\\Flagged") && !named.keeps("work"));
        let any = selected(&["* OK [PERMANENTFLAGS (\\Seen \\*)] Flags\r\n", validity]).unwrap();
        assert!(any.keeps("work") && any.keeps("\\Seen"));
        assert!(!any.keeps("\\Flagged"), "the star covers keywords alone");
        let none = selected(&["* OK [PERMANENTFLAGS ()] Read-only\r\n", validity]).unwrap();
        assert!(!none.keeps("\\Seen") && !none.keeps("work"));
        let silent = selected(&[validity]).unwrap();
        assert_eq!(silent.permanent, None);
        assert!(silent.keeps("\\Seen") && silent.keeps("work"));
    }

    #[test]
    fn a_select_without_uidvalidity_or_past_the_flag_bound_fails() {
        assert!(matches!(
            selected(&["* 3 EXISTS\r\n"]),
            Err(SessionError::Protocol(
                "SELECT answered without UIDVALIDITY"
            ))
        ));
        let flags = vec!["k"; MAX_PERMANENT_FLAGS + 1].join(" ");
        let line = format!("* OK [PERMANENTFLAGS ({flags})] Many\r\n");
        assert!(matches!(
            selected(&[&line]),
            Err(SessionError::Protocol(
                "the mailbox names too many permanent flags"
            ))
        ));
        // The read path passes the code over, so an EXAMINE stands.
        let lines = [&line, "* 3 EXISTS\r\n", "* OK [UIDVALIDITY 7] UIDs\r\n"];
        let examined = opened(&lines, Access::ReadOnly).unwrap();
        assert_eq!(examined.permanent, None);
        assert_eq!(examined.selected().unwrap().messages, 3);
    }
}
