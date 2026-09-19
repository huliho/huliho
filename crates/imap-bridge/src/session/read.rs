// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The read commands on the raw path: one tagged command, every
//! untagged line parsed here up to the tagged answer, so nothing the
//! server volunteers is lost.

mod list;

use std::time::Duration;

use async_imap::Connection;
use async_imap::imap_proto::{Response, Status};
use tokio::time::timeout;

use super::imap::{Stream, Wire, command_error};
use super::{SessionError, io_error};

pub(super) use list::{list, lsub, status};

type Signed = async_imap::Session<Stream>;

/// Room in one answer for the lines a server volunteers, alerts for one.
const SPARE_LINES: usize = 8;

/// The longest name the bridge keeps and sends back on a command line,
/// far above the few hundred bytes a server lets a name grow to.
const MAX_WIRE_NAME_BYTES: usize = 4096;

/// The longest raw name worth unescaping: the fold at most halves a
/// name, so a longer one stays above `MAX_WIRE_NAME_BYTES`.
const MAX_RAW_NAME_BYTES: usize = 2 * MAX_WIRE_NAME_BYTES;

/// What bounds one answer: the time one read may take and the lines
/// that may come before the tagged one.
#[derive(Clone, Copy)]
pub(super) struct Bounds {
    pub(super) step: Duration,
    pub(super) max_lines: usize,
}

/// Runs `command` and hands every untagged answer to `visit` until the
/// tagged OK, as [`answer`] reads it.
async fn collect(
    session: &mut Signed,
    command: &str,
    bounds: Bounds,
    visit: impl FnMut(&Response<'_>) -> Result<(), SessionError>,
) -> Result<(), SessionError> {
    let id = timeout(bounds.step, session.run_command(command))
        .await
        .map_err(|_elapsed| SessionError::Timeout)?
        .map_err(command_error)?;
    answer(session, &id.0, bounds, visit).await
}

/// Hands every untagged answer to `visit` until the one tagged `tag`: an
/// OK ends the read, a NO is `Refused`, a BAD a protocol failure in
/// fixed words. An answer of more lines than the bounds allow fails the
/// same way, so one command takes a bounded number of reads. Each
/// response is dropped before the next one is parsed.
pub(super) async fn answer<T: Wire>(
    connection: &mut Connection<T>,
    tag: &str,
    bounds: Bounds,
    mut visit: impl FnMut(&Response<'_>) -> Result<(), SessionError>,
) -> Result<(), SessionError> {
    let mut lines = 0;
    loop {
        let response = timeout(bounds.step, connection.read_response())
            .await
            .map_err(|_elapsed| SessionError::Timeout)?
            .map_err(io_error)?
            .ok_or(SessionError::Closed)?;
        match response.parsed() {
            Response::Done {
                tag: found, status, ..
            } if found.0 == tag => {
                return match status {
                    Status::Ok => Ok(()),
                    Status::No => Err(SessionError::Refused),
                    Status::Bad => Err(SessionError::Protocol("the server answered BAD")),
                    Status::PreAuth | Status::Bye => Err(SessionError::Closed),
                };
            }
            _ if lines == bounds.max_lines => {
                return Err(SessionError::Protocol("the answer passes the line limit"));
            }
            Response::Done { .. } => {}
            other => visit(other)?,
        }
        lines += 1;
    }
}

/// Whether the bridge puts the name on a command line. Any control
/// character rules a name out, which keeps CR, LF and NUL off the line.
/// So does a length above `MAX_WIRE_NAME_BYTES`.
fn sendable(name: &str) -> bool {
    name.len() <= MAX_WIRE_NAME_BYTES && !name.chars().any(char::is_control)
}

/// A mailbox name as a quoted string (RFC 9051 section 4.3), `\` and
/// `"` escaped. A name that is not sendable is refused, so none reaches
/// a command line.
fn quoted(name: &str) -> Result<String, SessionError> {
    if !sendable(name) {
        return Err(SessionError::Protocol("the mailbox name cannot be sent"));
    }
    let mut out = String::with_capacity(name.len() + 2);
    out.push('"');
    for c in name.chars() {
        if matches!(c, '"' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    Ok(out)
}

/// A name as the server spells it, `None` when it is not sendable: a
/// mailbox the bridge can never name has no place in what it reads. A
/// raw name above `MAX_RAW_NAME_BYTES` is never copied.
fn wire_name(name: &str) -> Option<String> {
    if name.len() > MAX_RAW_NAME_BYTES {
        return None;
    }
    Some(unescaped(name)).filter(|name| sendable(name))
}

/// Folds `\"` and `\\` back to one character, since the parser keeps
/// the escapes a quoted string arrives with (RFC 9051 section 4.3). A
/// name delivered as a literal gets the same fold, so a literal holding
/// `\"` or `\\` loses one character.
fn unescaped(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\'
            && let Some(escaped) = chars.next_if(|next| matches!(next, '"' | '\\'))
        {
            out.push(escaped);
            continue;
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_quoted_with_its_specials_escaped_rfc9051_4_3() {
        assert_eq!(quoted("INBOX").unwrap(), "\"INBOX\"");
        assert_eq!(quoted("a\"b\\c").unwrap(), "\"a\\\"b\\\\c\"");
        assert_eq!(quoted("&U,BTFw-").unwrap(), "\"&U,BTFw-\"");
    }

    #[test]
    fn a_name_with_a_control_character_or_past_the_byte_limit_is_never_quoted() {
        let long = "a".repeat(MAX_WIRE_NAME_BYTES + 1);
        for name in [
            "a\r\nA9 NOOP",
            "a\nb",
            "a\0b",
            "a\u{7f}b",
            "a\u{85}b",
            &long,
        ] {
            assert!(!sendable(name));
            assert!(matches!(
                quoted(name),
                Err(SessionError::Protocol("the mailbox name cannot be sent"))
            ));
        }
        assert!(sendable(&long[1..]));
        assert!(sendable("&U,BTFw- \"x\\y\""));
    }

    #[test]
    fn a_raw_name_of_escaped_pairs_is_kept_up_to_twice_the_byte_limit() {
        let pairs = "\\\\".repeat(MAX_WIRE_NAME_BYTES);
        assert_eq!(pairs.len(), MAX_RAW_NAME_BYTES);
        assert_eq!(wire_name(&pairs), Some("\\".repeat(MAX_WIRE_NAME_BYTES)));
        assert_eq!(wire_name(&format!("{pairs}a")), None);
    }
}
