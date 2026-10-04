// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The write commands on the raw path: STORE of flags on messages of
//! the selected mailbox, silent (RFC 3501 section 6.4.6). A flag reaches
//! the command line only as one of the four system flags a keyword maps
//! to or as an atom.

use super::read::{ATOM_SPECIALS, Room, Selection, sequence_set};
use super::{MAX_STORE_UIDS, SessionError, StoreAsk};

/// The system flags a keyword maps to (RFC 8621 section 4.1.1). No
/// other backslash flag is ever stored, `\Deleted` least of all.
const STORABLE_SYSTEM_FLAGS: [&str; 4] = ["\\Seen", "\\Flagged", "\\Answered", "\\Draft"];

/// The longest command the bridge builds, tag and line end not counted (RFC 7162 section 4).
pub const MAX_COMMAND_BYTES: usize = 8192;

/// The fixed words of a STORE the bridge does not send.
const NO_MESSAGE: &str = "a STORE names no message or too many";
const NO_FLAG: &str = "a STORE names no flag or one that cannot be sent";
const LINE_LIMIT: &str = "the command passes the line limit";

/// Whether a flag goes on a command line as it is: one of the storable
/// system flags or an atom (RFC 3501 section 9).
fn sendable_flag(flag: &str) -> bool {
    STORABLE_SYSTEM_FLAGS.contains(&flag)
        || (!flag.is_empty()
            && flag
                .bytes()
                .all(|byte| byte.is_ascii_graphic() && !ATOM_SPECIALS.contains(&byte)))
}

fn command(ask: &StoreAsk<'_>) -> Result<String, SessionError> {
    // A UID is never zero (RFC 3501 section 2.3.1.1).
    if ask.uids.is_empty() || ask.uids.len() > MAX_STORE_UIDS || ask.uids.contains(&0) {
        return Err(SessionError::Protocol(NO_MESSAGE));
    }
    if ask.flags.is_empty() || !ask.flags.iter().all(|flag| sendable_flag(flag)) {
        return Err(SessionError::Protocol(NO_FLAG));
    }
    let sign = if ask.add { '+' } else { '-' };
    let command = format!(
        "UID STORE {} {sign}FLAGS.SILENT ({})",
        sequence_set(ask.uids),
        ask.flags.join(" ")
    );
    if command.len() > MAX_COMMAND_BYTES {
        return Err(SessionError::Protocol(LINE_LIMIT));
    }
    Ok(command)
}

/// One STORE. A server with CONDSTORE answers a line per message even
/// to a silent one (RFC 7162 section 3.1.3), so the answer has room for
/// as many lines as the ask names messages; none of them is read.
pub(super) async fn uid_store(
    selection: &mut Selection<'_>,
    ask: &StoreAsk<'_>,
    room: Room,
) -> Result<(), SessionError> {
    selection.messages()?;
    let command = command(ask)?;
    selection
        .collect(&command, room.bounds(ask.uids.len()), |_| Ok(()))
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    fn line(uids: &[u32], names: &[&str], add: bool) -> Result<String, SessionError> {
        let flags = flags(names);
        command(&StoreAsk {
            uids,
            flags: &flags,
            add,
        })
    }

    #[test]
    fn a_store_adds_or_removes_silently_over_a_folded_set_rfc3501_6_4_6() {
        assert_eq!(
            line(&[7, 5, 6, 9], &["\\Seen"], true).unwrap(),
            "UID STORE 5:7,9 +FLAGS.SILENT (\\Seen)"
        );
        assert_eq!(
            line(&[3], &["\\Flagged", "$forwarded"], false).unwrap(),
            "UID STORE 3 -FLAGS.SILENT (\\Flagged $forwarded)"
        );
    }

    #[test]
    fn a_flag_that_is_no_atom_never_reaches_the_command_line_rfc3501_9() {
        for flag in [
            "",
            "a b",
            "a\r\nA9 LOGOUT",
            "x)",
            "(x",
            "{3}",
            "a%",
            "a*",
            "a\"b",
            "a]",
            "caf\u{e9}",
            "\\Deleted",
            "\\Recent",
            "\\seen",
            "\\*",
        ] {
            assert!(
                matches!(
                    line(&[1], &[flag], true),
                    Err(SessionError::Protocol(NO_FLAG))
                ),
                "{flag:?}"
            );
        }
        assert!(sendable_flag("$label:1/a~b"));
        assert!(matches!(
            line(&[1], &[], true),
            Err(SessionError::Protocol(NO_FLAG))
        ));
    }

    #[test]
    fn a_store_names_one_message_at_least_and_a_bounded_set() {
        let many: Vec<u32> = (1..=u32::try_from(MAX_STORE_UIDS).unwrap() + 1).collect();
        for uids in [&[][..], &[0], &[4, 0], &many] {
            assert!(matches!(
                line(uids, &["\\Seen"], true),
                Err(SessionError::Protocol(NO_MESSAGE))
            ));
        }
        assert!(line(&many[1..], &["\\Seen"], true).is_ok());
    }

    #[test]
    fn a_command_past_the_line_limit_is_never_sent_rfc7162_4() {
        let long = "k".repeat(MAX_COMMAND_BYTES);
        assert!(matches!(
            line(&[1], &[&long], true),
            Err(SessionError::Protocol(LINE_LIMIT))
        ));
    }
}
