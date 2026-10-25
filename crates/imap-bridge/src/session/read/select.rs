// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! EXAMINE and SELECT: a mailbox opened read-only or for writing.

use async_imap::imap_proto::{MailboxDatum, Response, ResponseCode, Status};

use super::{Room, Selection, modseq, quoted};
use crate::session::{Selected, SessionError, Writable};

/// The lines of its own an EXAMINE or a SELECT answer carries: FLAGS,
/// EXISTS, RECENT and the OK lines with UNSEEN, PERMANENTFLAGS, UIDNEXT,
/// UIDVALIDITY and HIGHESTMODSEQ (RFC 3501 section 6.3.1, RFC 7162).
const OPEN_LINES: usize = 8;

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
    read_only: bool,
}

impl Opened {
    /// Keeps what one line says about the mailbox. The permanent flags
    /// count for a SELECT alone: the read path passes them over, whatever
    /// their number. `[READ-ONLY]` belongs on the tagged OK (RFC 3501
    /// section 6.3.1); an untagged OK that carries it counts as well.
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
                ResponseCode::ReadOnly => self.read_only = true,
                _ => {}
            },
            Response::Done {
                status: Status::Ok,
                code: Some(ResponseCode::ReadOnly),
                ..
            } => self.read_only = true,
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
            read_only: self.read_only,
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_select_answered_with_read_only_reads_as_read_only_rfc3501_6_3_1() {
        let validity = "* OK [UIDVALIDITY 7] UIDs valid\r\n";
        let read_only = |lines: &[&str]| selected(lines).unwrap().read_only;
        assert!(!read_only(&[validity, "A1 OK [READ-WRITE] Selected\r\n"]));
        assert!(!read_only(&[validity, "A1 OK Selected\r\n"]));
        assert!(read_only(&[validity, "A1 OK [READ-ONLY] Selected\r\n"]));
        assert!(read_only(&["* OK [READ-ONLY] Shared\r\n", validity]));
        // Only an OK counts; a NO never reaches the reader.
        assert!(!read_only(&[validity, "A1 NO [READ-ONLY] Refused\r\n"]));
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
