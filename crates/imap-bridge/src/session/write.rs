// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The write commands on the raw path, each on a mailbox a SELECT opened
//! for writing. A STORE changes flags silently (RFC 3501 section 6.4.6).
//! A MOVE (RFC 6851 section 3.2) or a COPY (RFC 3501 section 6.4.7)
//! takes messages to another mailbox and reads their new UIDs from
//! `COPYUID`. An expunge removes the messages it names and no other (RFC
//! 4315 section 2.1). A flag reaches the command line only as one of the
//! four system flags a keyword maps to or as an atom; `\Deleted` is
//! stored by the expunge alone, which never sends a plain EXPUNGE.

mod copyuid;

use async_imap::imap_proto::Response;

use super::read::{ATOM_SPECIALS, Room, Selection, quoted, sequence_set};
use super::{MAX_STORE_UIDS, MoveAsk, Moved, SessionError, StoreAsk};

/// The system flags a keyword maps to (RFC 8621 section 4.1.1). No
/// other backslash flag is stored through a STORE ask.
const STORABLE_SYSTEM_FLAGS: [&str; 4] = ["\\Seen", "\\Flagged", "\\Answered", "\\Draft"];

/// The flag the expunge stores and nothing else does.
const DELETED: &str = "\\Deleted";

/// The longest command the bridge builds, tag and line end not counted (RFC 7162 section 4).
pub const MAX_COMMAND_BYTES: usize = 8192;

/// The fixed words of a write the bridge does not send.
const NO_MESSAGE: &str = "a write names no message or too many";
const NO_FLAG: &str = "a STORE names no flag or one that cannot be sent";
const LINE_LIMIT: &str = "the command passes the line limit";

/// Which of the two commands takes messages to another mailbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Transfer {
    Move,
    Copy,
}

/// Whether a flag goes on a command line as it is: one of the storable
/// system flags or an atom (RFC 3501 section 9).
fn sendable_flag(flag: &str) -> bool {
    STORABLE_SYSTEM_FLAGS.contains(&flag)
        || (!flag.is_empty()
            && flag
                .bytes()
                .all(|byte| byte.is_ascii_graphic() && !ATOM_SPECIALS.contains(&byte)))
}

/// The set of one write: a message at least, `MAX_STORE_UIDS` at most,
/// never a UID of zero (RFC 3501 section 2.3.1.1).
fn uid_set(uids: &[u32]) -> Result<String, SessionError> {
    if uids.is_empty() || uids.len() > MAX_STORE_UIDS || uids.contains(&0) {
        return Err(SessionError::Protocol(NO_MESSAGE));
    }
    Ok(sequence_set(uids))
}

/// The command as built; past `MAX_COMMAND_BYTES` it is never sent.
fn fitting(command: String) -> Result<String, SessionError> {
    if command.len() > MAX_COMMAND_BYTES {
        return Err(SessionError::Protocol(LINE_LIMIT));
    }
    Ok(command)
}

fn store_command(ask: &StoreAsk<'_>) -> Result<String, SessionError> {
    let set = uid_set(ask.uids)?;
    if ask.flags.is_empty() || !ask.flags.iter().all(|flag| sendable_flag(flag)) {
        return Err(SessionError::Protocol(NO_FLAG));
    }
    let sign = if ask.add { '+' } else { '-' };
    let flags = ask.flags.join(" ");
    fitting(format!("UID STORE {set} {sign}FLAGS.SILENT ({flags})"))
}

fn transfer_command(transfer: Transfer, ask: &MoveAsk<'_>) -> Result<String, SessionError> {
    let verb = match transfer {
        Transfer::Move => "MOVE",
        Transfer::Copy => "COPY",
    };
    let set = uid_set(ask.uids)?;
    fitting(format!("UID {verb} {set} {}", quoted(ask.target)?))
}

/// The two commands of an expunge over one set: `\Deleted` stored on
/// its messages, then `UID EXPUNGE` of the same set, which leaves a
/// message another client marked where it is.
fn expunge_commands(uids: &[u32]) -> Result<[String; 2], SessionError> {
    let set = uid_set(uids)?;
    Ok([
        fitting(format!("UID STORE {set} +FLAGS.SILENT ({DELETED})"))?,
        fitting(format!("UID EXPUNGE {set}"))?,
    ])
}

/// One STORE. A server with CONDSTORE answers a line per message even
/// to a silent one (RFC 7162 section 3.1.3), so the answer has room for
/// as many lines as the ask names messages; none of them is read.
pub(super) async fn uid_store(
    selection: &mut Selection<'_>,
    ask: &StoreAsk<'_>,
    room: Room,
) -> Result<(), SessionError> {
    selection.writable()?;
    let command = store_command(ask)?;
    selection
        .collect(&command, room.bounds(ask.uids.len()), |_| Ok(()))
        .await
}

/// One `UID MOVE`; the answer has room for an EXPUNGE per message and
/// the line that carries `COPYUID`.
pub(super) async fn uid_move(
    selection: &mut Selection<'_>,
    ask: &MoveAsk<'_>,
    room: Room,
) -> Result<Option<Moved>, SessionError> {
    transfer(selection, Transfer::Move, ask, room).await
}

/// One `UID COPY`, whose `COPYUID` rides on the tagged OK.
pub(super) async fn uid_copy(
    selection: &mut Selection<'_>,
    ask: &MoveAsk<'_>,
    room: Room,
) -> Result<Option<Moved>, SessionError> {
    transfer(selection, Transfer::Copy, ask, room).await
}

/// A move or a copy and the pairs its `COPYUID` names; `None` when the
/// answer carries no code, as from a server without UIDPLUS.
async fn transfer(
    selection: &mut Selection<'_>,
    transfer: Transfer,
    ask: &MoveAsk<'_>,
    room: Room,
) -> Result<Option<Moved>, SessionError> {
    selection.writable()?;
    let command = transfer_command(transfer, ask)?;
    let mut moved = None;
    let visit = |response: &Response<'_>| copyuid::note(&mut moved, response, ask.uids);
    selection
        .collect(&command, room.bounds(ask.uids.len() + 1), visit)
        .await?;
    Ok(moved)
}

/// `\Deleted` on the messages, then `UID EXPUNGE` of the same set; when
/// the STORE fails the expunge is never sent. Each answer has room for a
/// line per message.
pub(super) async fn uid_expunge(
    selection: &mut Selection<'_>,
    uids: &[u32],
    room: Room,
) -> Result<(), SessionError> {
    selection.writable()?;
    let [store, expunge] = expunge_commands(uids)?;
    let bounds = room.bounds(uids.len());
    selection.collect(&store, bounds, |_| Ok(())).await?;
    selection.collect(&expunge, bounds, |_| Ok(())).await
}

/// One line through the `COPYUID` reader, for the fuzz targets.
#[cfg(feature = "test-support")]
pub(super) fn line(bytes: &[u8]) {
    if let Ok((_, response)) = Response::from_bytes(bytes) {
        let _ = copyuid::note(&mut None, &response, &[1, 2, 3]);
    }
}

#[cfg(test)]
mod tests;
