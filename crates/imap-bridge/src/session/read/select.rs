// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! EXAMINE and the UID list: a mailbox opened read-only and the UIDs it
//! holds, asked for in windows of sequence numbers so no answer grows
//! with the folder.

use async_imap::imap_proto::{MailboxDatum, Response, ResponseCode, Status};

use super::{Room, Selection, quoted};
use crate::session::{Selected, SessionError};

/// The lines of its own an EXAMINE answer carries: FLAGS, EXISTS,
/// RECENT and the OK lines with UNSEEN, PERMANENTFLAGS, UIDNEXT,
/// UIDVALIDITY and HIGHESTMODSEQ (RFC 3501 section 6.3.1, RFC 7162).
const EXAMINE_LINES: usize = 8;

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

/// EXAMINE, never SELECT: the read path must not clear `\Recent` or
/// hold write access.
pub(in crate::session) async fn examine(
    selection: &mut Selection<'_>,
    mailbox: &str,
    room: Room,
) -> Result<Selected, SessionError> {
    let command = format!("EXAMINE {}", quoted(mailbox)?);
    let (mut uid_validity, mut counted) = (None, false);
    let bounds = room.bounds(EXAMINE_LINES);
    let visit = |response: &Response<'_>| {
        match response {
            Response::Data {
                status: Status::Ok,
                code: Some(ResponseCode::UidValidity(value)),
                ..
            } => uid_validity = Some(*value),
            Response::MailboxData(MailboxDatum::Exists(_)) => counted = true,
            _ => {}
        }
        Ok(())
    };
    selection.collect(&command, bounds, visit).await?;
    if !counted {
        return Err(SessionError::Protocol("EXAMINE answered without EXISTS"));
    }
    let uid_validity = uid_validity.ok_or(SessionError::Protocol(
        "EXAMINE answered without UIDVALIDITY",
    ))?;
    Ok(Selected { uid_validity })
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
}
