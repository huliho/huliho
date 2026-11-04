// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The message commands on the raw path: APPEND (RFC 3501 section
//! 6.3.11) and `UID REPLACE` (RFC 8508). A message goes as one literal
//! behind its byte count (RFC 3501 section 4.3), so no byte of it can end
//! the command or open another. It goes without waiting where the server
//! reads it so (RFC 7888), else after the continuation request. The
//! command line holds the mailbox through `quoted`, flags from the fixed
//! set and the size marker, and nothing of the message.

use std::future::Future;

use async_imap::imap_proto::{Response, ResponseCode, Status, UidSetMember};
use tokio::io::AsyncWriteExt;
use tokio::time::{Instant, timeout};

use super::imap::command_error;
use super::read::{Bounds, Room, Selection, continuation, quoted};
use super::write::{STORABLE_SYSTEM_FLAGS, fitting};
use super::{Literals, SessionError, io_error};

/// The lines of its own one answer may carry: the untagged OK with
/// `APPENDUID`, the EXISTS and the EXPUNGE of a replace (RFC 8508
/// section 3.2).
const ANSWER_LINES: usize = 3;

/// The fixed words of a message the bridge does not send.
const NO_FLAG: &str = "an APPEND names a flag that cannot be sent";
const NUL: &str = "a message holds a NUL";
const NO_UID: &str = "a REPLACE names no message";
const APPENDUID: &str = "APPENDUID does not name one message";

/// The two commands that put a message in a mailbox. The session's seam
/// has them beside itself, as a supertrait.
pub trait Appends: Send {
    /// APPEND of one message to a mailbox by its wire name, read as
    /// [`crate::session::Session::list`] is: the UID its `APPENDUID` names,
    /// `None` when the answer carries none. The server stamps the
    /// arrival.
    ///
    /// # Errors
    ///
    /// `Refused` for a NO, before or after the literal. Before anything
    /// is sent, `Protocol` for a flag outside the four storable system
    /// flags, a message holding a NUL, a name that cannot be sent or a
    /// command past `MAX_COMMAND_BYTES`.
    fn append(
        &mut self,
        ask: &AppendAsk<'_>,
    ) -> impl Future<Output = Result<Option<Appended>, SessionError>> + Send;

    /// `UID REPLACE` of a message of the selected mailbox (RFC 8508
    /// section 3.3): the new message appended and the old one removed as
    /// one action. The caller asks it where REPLACE is advertised.
    ///
    /// # Errors
    ///
    /// As [`Appends::append`]; `Protocol` as well, before anything is
    /// sent, when no mailbox is open for writing or the UID is zero.
    fn replace(
        &mut self,
        uid: u32,
        ask: &AppendAsk<'_>,
    ) -> impl Future<Output = Result<Option<Appended>, SessionError>> + Send;
}

/// One message on its way to a mailbox. It has no debug form, since the
/// bytes are what a sender wrote.
#[derive(Clone, Copy)]
pub struct AppendAsk<'a> {
    /// The wire name of the mailbox.
    pub mailbox: &'a str,
    /// The flags as the wire carries them, each one of the four storable
    /// system flags.
    pub flags: &'a [String],
    pub message: &'a [u8],
    /// Which literals the server reads without a continuation request.
    pub literals: Literals,
    /// When the command must be over: writing the literal and each read
    /// of its answer wait until then in place of the session's step.
    pub until: Option<Instant>,
}

/// What `APPENDUID` names (RFC 4315 section 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Appended {
    pub uid_validity: u32,
    pub uid: u32,
}

/// The command line: the verb, the mailbox, the flags and the size
/// marker. An ask that cannot go out sends nothing.
fn line(verb: &str, ask: &AppendAsk<'_>) -> Result<String, SessionError> {
    let flags = ask.flags.iter().map(String::as_str);
    if !flags
        .into_iter()
        .all(|flag| STORABLE_SYSTEM_FLAGS.contains(&flag))
    {
        return Err(SessionError::Protocol(NO_FLAG));
    }
    if ask.message.contains(&0) {
        return Err(SessionError::Protocol(NUL));
    }
    let mailbox = quoted(ask.mailbox)?;
    let flags = if ask.flags.is_empty() {
        String::new()
    } else {
        format!("({}) ", ask.flags.join(" "))
    };
    let size = ask.message.len();
    let plus = if ask.literals.unsynchronized(size) {
        "+"
    } else {
        ""
    };
    fitting(format!("{verb} {mailbox} {flags}{{{size}{plus}}}"))
}

/// One APPEND.
pub(super) async fn append(
    selection: &mut Selection<'_>,
    ask: &AppendAsk<'_>,
    room: Room,
) -> Result<Option<Appended>, SessionError> {
    let command = line("APPEND", ask)?;
    send(
        selection,
        &command,
        ask,
        room.until(ask.until)?.bounds(ANSWER_LINES),
    )
    .await
}

/// One `UID REPLACE` on the mailbox a SELECT opened.
pub(super) async fn replace(
    selection: &mut Selection<'_>,
    (uid, ask): (u32, &AppendAsk<'_>),
    room: Room,
) -> Result<Option<Appended>, SessionError> {
    selection.writable()?;
    if uid == 0 {
        return Err(SessionError::Protocol(NO_UID));
    }
    let command = line(&format!("UID REPLACE {uid}"), ask)?;
    send(
        selection,
        &command,
        ask,
        room.until(ask.until)?.bounds(ANSWER_LINES),
    )
    .await
}

/// The command under a tag of the client library, the literal when the
/// server takes it and the answer with the code it carries.
async fn send(
    selection: &mut Selection<'_>,
    command: &str,
    ask: &AppendAsk<'_>,
    bounds: Bounds,
) -> Result<Option<Appended>, SessionError> {
    let id = timeout(bounds.step, selection.session.run_command(command))
        .await
        .map_err(|_elapsed| SessionError::Timeout)?
        .map_err(command_error)?;
    if !ask.literals.unsynchronized(ask.message.len()) {
        continuation(selection, &id.0, bounds).await?;
    }
    let written = async {
        let stream = selection.session.get_mut();
        stream.write_all(ask.message).await?;
        stream.write_all(b"\r\n").await?;
        stream.flush().await
    };
    timeout(bounds.step, written)
        .await
        .map_err(|_elapsed| SessionError::Timeout)?
        .map_err(io_error)?;
    let mut appended = None;
    let visit = |response: &Response<'_>| note(&mut appended, response);
    selection.answer(&id.0, bounds, visit).await?;
    Ok(appended)
}

/// Keeps what one line's `APPENDUID` names. An APPEND carries it on the
/// tagged OK (RFC 4315 section 3), a replace on an untagged OK ahead of
/// its expunge (RFC 8508 section 3.2); both count.
fn note(appended: &mut Option<Appended>, response: &Response<'_>) -> Result<(), SessionError> {
    let (Response::Data {
        status: Status::Ok,
        code: Some(code),
        ..
    }
    | Response::Done {
        status: Status::Ok,
        code: Some(code),
        ..
    }) = response
    else {
        return Ok(());
    };
    let ResponseCode::AppendUid(uid_validity, uids) = code else {
        return Ok(());
    };
    let uid = match uids.as_slice() {
        [UidSetMember::Uid(uid)] => *uid,
        [UidSetMember::UidRange(range)] if range.start() == range.end() => *range.start(),
        _ => return Err(SessionError::Protocol(APPENDUID)),
    };
    if *uid_validity == 0 || uid == 0 {
        return Err(SessionError::Protocol(APPENDUID));
    }
    *appended = Some(Appended {
        uid_validity: *uid_validity,
        uid,
    });
    Ok(())
}

/// One line through the `APPENDUID` reader, for the fuzz targets.
#[cfg(feature = "test-support")]
pub(super) fn line_read(bytes: &[u8]) {
    if let Ok((_, response)) = Response::from_bytes(bytes) {
        let _ = note(&mut None, &response);
    }
}

#[cfg(test)]
mod tests;
