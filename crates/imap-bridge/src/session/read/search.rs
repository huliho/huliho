// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! UID SEARCH over windows of sequence numbers: the UID list of a folder
//! and a search with criteria (RFC 3501 section 6.4.4). A string of the
//! client reaches the command line only as a literal with its byte count
//! (RFC 3501 section 4.3), never bare or quoted, so no byte of it can end
//! the command or open another. A literal goes without waiting where the
//! server reads it so (RFC 7888), else after its continuation request.

#[cfg(test)]
mod tests;

use std::fmt::{self, Write as _};

use async_imap::imap_proto::{MailboxDatum, Response};
use time::Date;
use tokio::time::timeout;

use super::{ANSWER_LINE_LIMIT, Bounds, Room, Selection, refusal};
use crate::session::imap::command_error;
use crate::session::write::{LINE_LIMIT, wire_label};
use crate::session::{
    Literals, MAX_COMMAND_BYTES, SEARCH_TERM_BYTES, SEARCH_TERMS, SearchAsk, SessionError, io_error,
};

/// The SEARCH line of one window.
const SEARCH_LINES: usize = 1;

/// The sequence numbers one UID SEARCH names. That many UIDs of ten
/// digits take 55 KB, which stays under the structure bound of a
/// response.
const SEARCH_WINDOW: u32 = 5000;

/// The UIDs one folder may hold, 12 MB as a list.
const MAX_FOLDER_UIDS: usize = 3_000_000;

/// The last year an IMAP date can write: it has four digits (RFC 3501
/// section 9).
const MAX_YEAR: i32 = 9999;

/// The months as an IMAP date writes them (RFC 3501 section 9).
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The fixed words of a folder past `MAX_FOLDER_UIDS`.
const UID_LIMIT: &str = "the folder passes the UID limit";

/// The fixed words of a search the bridge does not send.
const TERM_LIMIT: &str = "a search passes the term limit";
const NUL: &str = "a search term holds a NUL";
const NO_KEY: &str = "a search names no key";
const NO_GMAIL: &str = "the server does not search as Gmail does";
const NO_LABEL: &str = "a search label cannot be sent";
const NO_DATE: &str = "a search date cannot be written";

/// One piece of a search: the bridge's own words or a string of the
/// client, which goes as a literal.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Piece {
    Words(String),
    Literal(String),
}

/// One command as it goes out: its opening and, for every literal that
/// waits for the server's continuation request, a segment that opens
/// with the bytes of that literal.
#[derive(Debug, PartialEq, Eq)]
struct Segments {
    first: String,
    later: Vec<String>,
}

impl SearchAsk<'_> {
    /// Whether the ask goes out as it stands. It names one key at least
    /// and at most `SEARCH_TERMS` terms of at most `SEARCH_TERM_BYTES`
    /// each; no term holds a NUL, which no literal carries (RFC 3501
    /// section 9, `CHAR8`); the Gmail form needs a server that has it.
    ///
    /// # Errors
    ///
    /// `Protocol` in fixed words for an ask that cannot go out.
    pub fn check(&self) -> Result<(), SessionError> {
        let found: Vec<&str> = terms(self.text)
            .into_iter()
            .chain(terms(self.from))
            .collect();
        if found.len() > SEARCH_TERMS || found.iter().any(|term| term.len() > SEARCH_TERM_BYTES) {
            return Err(SessionError::Protocol(TERM_LIMIT));
        }
        if found.iter().any(|term| term.contains('\0')) {
            return Err(SessionError::Protocol(NUL));
        }
        if self.gmail.is_some() && !self.server.gmail {
            return Err(SessionError::Protocol(NO_GMAIL));
        }
        let label = self.gmail.is_some_and(|gmail| gmail.label.is_some());
        let plain = self.unseen || self.flagged || self.since.is_some() || self.before.is_some();
        if found.is_empty() && !label && !plain {
            return Err(SessionError::Protocol(NO_KEY));
        }
        Ok(())
    }
}

/// Counts the terms of the text and the sender and says whether a Gmail
/// label is set, printing neither, so an ask in a log line carries no
/// text of the client and no mailbox name.
impl fmt::Debug for SearchAsk<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = self.gmail.is_some_and(|gmail| gmail.label.is_some());
        f.debug_struct("SearchAsk")
            .field("text_terms", &terms(self.text).len())
            .field("from_terms", &terms(self.from).len())
            .field("unseen", &self.unseen)
            .field("flagged", &self.flagged)
            .field("since", &self.since)
            .field("before", &self.before)
            .field("gmail", &self.gmail.is_some())
            .field("label", &label)
            .field("server", &self.server)
            .finish()
    }
}

/// Every UID of the selected mailbox, highest first, each once.
pub(in crate::session) async fn uid_list(
    selection: &mut Selection<'_>,
    room: Room,
) -> Result<Vec<u32>, SessionError> {
    walk(selection, &[], Literals::Synchronizing, room).await
}

/// The UIDs of the selected mailbox the ask matches, highest first, each
/// once. An ask that cannot go out sends nothing.
pub(in crate::session) async fn uid_search(
    selection: &mut Selection<'_>,
    ask: &SearchAsk<'_>,
    room: Room,
) -> Result<Vec<u32>, SessionError> {
    let keys = keys(ask)?;
    walk(selection, &keys, ask.server.literals, room).await
}

/// One `UID SEARCH <low>:<high>` with the keys per window of sequence
/// numbers from the top down. An expunge only moves a message down, so
/// none is missed; one that moved into the next window shows up twice
/// and is dropped.
async fn walk(
    selection: &mut Selection<'_>,
    keys: &[Piece],
    literals: Literals,
    room: Room,
) -> Result<Vec<u32>, SessionError> {
    let mut top = start(selection.messages()?)?;
    let mut uids = Vec::new();
    while let Some((low, high)) = window(top, selection.messages()?) {
        let command = segments(keys, (low, high), literals)?;
        let visit = |response: &Response<'_>| {
            if let Response::MailboxData(MailboxDatum::Search(found)) = response {
                hold(&mut uids, found)?;
            }
            Ok(())
        };
        send(selection, &command, room.bounds(SEARCH_LINES), visit).await?;
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

/// The terms of a text as typed: whitespace apart, a run in double
/// quotes one term with its whitespace, the quotes kept. A term of
/// quotes alone names nothing.
fn terms(text: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut start = None;
    let mut quoted = false;
    for (index, c) in text.char_indices() {
        if c.is_whitespace() && !quoted {
            if let Some(from) = start.take() {
                found.push(&text[from..index]);
            }
            continue;
        }
        if c == '"' {
            quoted = !quoted;
        }
        start.get_or_insert(index);
    }
    found.extend(start.map(|from| &text[from..]));
    found.retain(|term| !bare(term).is_empty());
    found
}

/// A term without its quotes: what a TEXT or a FROM key matches.
fn bare(term: &str) -> String {
    term.replace('"', "")
}

/// The keys of the ask in the order the command carries them: a TEXT or
/// a FROM key per term, on Gmail the text and the sender as one X-GM-RAW
/// string, then the plaintext conditions. A Gmail label goes as a
/// literal, since Gmail reads a system label written as an atom in a
/// SEARCH as no label at all.
fn keys(ask: &SearchAsk<'_>) -> Result<Vec<Piece>, SessionError> {
    ask.check()?;
    let (text, from) = (terms(ask.text), terms(ask.from));
    let mut keys = Vec::new();
    if let Some(gmail) = ask.gmail {
        if !text.is_empty() || !from.is_empty() {
            keys.push(Piece::Words("X-GM-RAW".to_owned()));
            keys.push(Piece::Literal(raw(&text, &from)));
        }
        if let Some(label) = gmail.label {
            if wire_label(label).is_none() {
                return Err(SessionError::Protocol(NO_LABEL));
            }
            keys.push(Piece::Words("X-GM-LABELS".to_owned()));
            keys.push(Piece::Literal(label.to_owned()));
        }
    } else {
        for (key, found) in [("TEXT", &text), ("FROM", &from)] {
            for term in found {
                keys.push(Piece::Words(key.to_owned()));
                keys.push(Piece::Literal(bare(term)));
            }
        }
    }
    for (key, asked) in [("UNSEEN", ask.unseen), ("FLAGGED", ask.flagged)] {
        if asked {
            keys.push(Piece::Words(key.to_owned()));
        }
    }
    for (key, day) in [("SINCE", ask.since), ("BEFORE", ask.before)] {
        if let Some(day) = day {
            let date = imap_date(day).ok_or(SessionError::Protocol(NO_DATE))?;
            keys.push(Piece::Words(format!("{key} {date}")));
        }
    }
    Ok(keys)
}

/// The X-GM-RAW string: the terms of the text as typed, then the sender
/// as one phrase, which Gmail reads as its web search does.
fn raw(text: &[&str], from: &[&str]) -> String {
    let mut raw = text.join(" ");
    if !from.is_empty() {
        let sender: Vec<String> = from.iter().map(|term| bare(term)).collect();
        if !raw.is_empty() {
            raw.push(' ');
        }
        let _ = write!(raw, "from:\"{}\"", sender.join(" "));
    }
    raw
}

/// A day as an IMAP date (RFC 3501 section 9, `date-text`); `None` for
/// a year outside four digits.
fn imap_date(day: Date) -> Option<String> {
    let year = day.year();
    if !(0..=MAX_YEAR).contains(&year) {
        return None;
    }
    let month = MONTHS.get(usize::from(u8::from(day.month())) - 1)?;
    Some(format!("{}-{month}-{year:04}", day.day()))
}

/// The command of one window, cut after every literal that waits for the
/// server's continuation request. `CHARSET UTF-8` comes with a command
/// that carries a literal (RFC 3501 section 6.4.4). Past
/// `MAX_COMMAND_BYTES` nothing goes.
fn segments(
    keys: &[Piece],
    (low, high): (u32, u32),
    literals: Literals,
) -> Result<Segments, SessionError> {
    let charset = keys.iter().any(|piece| matches!(piece, Piece::Literal(_)));
    let mut current = String::from("UID SEARCH");
    if charset {
        current.push_str(" CHARSET UTF-8");
    }
    let _ = write!(current, " {low}:{high}");
    let mut done = Vec::new();
    for piece in keys {
        current.push(' ');
        match piece {
            Piece::Words(words) => current.push_str(words),
            Piece::Literal(string) if !literals.unsynchronized(string.len()) => {
                let _ = write!(current, "{{{}}}", string.len());
                done.push(std::mem::replace(&mut current, string.clone()));
            }
            Piece::Literal(string) => {
                let _ = write!(current, "{{{}+}}\r\n{string}", string.len());
            }
        }
    }
    done.push(current);
    if done.iter().map(String::len).sum::<usize>() > MAX_COMMAND_BYTES {
        return Err(SessionError::Protocol(LINE_LIMIT));
    }
    let first = done.remove(0);
    Ok(Segments { first, later: done })
}

/// Sends one command and reads its answer: the opening under a tag of
/// the client library, every later segment once the server asked for
/// it.
async fn send(
    selection: &mut Selection<'_>,
    command: &Segments,
    bounds: Bounds,
    visit: impl FnMut(&Response<'_>) -> Result<(), SessionError>,
) -> Result<(), SessionError> {
    let id = timeout(bounds.step, selection.session.run_command(&command.first))
        .await
        .map_err(|_elapsed| SessionError::Timeout)?
        .map_err(command_error)?;
    for segment in &command.later {
        continuation(selection, &id.0, bounds).await?;
        timeout(bounds.step, selection.session.run_command_untagged(segment))
            .await
            .map_err(|_elapsed| SessionError::Timeout)?
            .map_err(command_error)?;
    }
    selection.answer(&id.0, bounds, visit).await
}

/// Reads up to the continuation request a literal waits for (RFC 3501
/// section 7.5) under the bounds of one answer, the count following the
/// lines on the way. A tagged answer in its place, an OK included, ends
/// the command as its [`refusal`].
async fn continuation(
    selection: &mut Selection<'_>,
    tag: &str,
    bounds: Bounds,
) -> Result<(), SessionError> {
    for _ in 0..=bounds.max_lines {
        let response = timeout(bounds.step, selection.session.read_response())
            .await
            .map_err(|_elapsed| SessionError::Timeout)?
            .map_err(io_error)?
            .ok_or(SessionError::Closed)?;
        match response.parsed() {
            Response::Continue { .. } => return Ok(()),
            Response::Done {
                tag: found,
                status,
                code,
                ..
            } if found.0 == tag => return Err(refusal(status, code.as_ref())),
            other => {
                if let Some(count) = selection.count.as_deref_mut() {
                    count.note(other);
                }
            }
        }
    }
    Err(SessionError::Protocol(ANSWER_LINE_LIMIT))
}
