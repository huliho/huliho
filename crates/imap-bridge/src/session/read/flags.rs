// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! UID FETCH of the flags alone: what changed since a mod-sequence (RFC
//! 7162 section 3.1.4.1) or every message of a range.

use std::collections::BTreeMap;

use async_imap::imap_proto::{AttributeValue, Response};

use super::fetch::kept_flags;
use super::{Room, Selection, modseq};
use crate::session::{FlagFetch, Flagged, MAX_FLAGGED, MESSAGE_LIMIT, SessionError};

fn command(fetch: FlagFetch) -> Result<String, SessionError> {
    match fetch {
        FlagFetch::ChangedSince(since) => Ok(format!(
            "UID FETCH 1:* (UID FLAGS) (CHANGEDSINCE {})",
            modseq(since)?.max(1)
        )),
        // IMAP reads `5:3` as `3:5`, which the caller does not mean.
        FlagFetch::Range(range) if range.low > range.high => {
            Err(SessionError::Protocol("the UID range runs backward"))
        }
        FlagFetch::Range(range) => Ok(format!(
            "UID FETCH {}:{} (UID FLAGS)",
            range.low, range.high
        )),
    }
}

/// The flags by UID, each message once with the last line the server
/// sent for it; more than `MAX_FLAGGED` messages fail the answer. A
/// server sends a mod-sequence of zero for none, so the command asks
/// from one at least.
pub(in crate::session) async fn uid_flags(
    selection: &mut Selection<'_>,
    fetch: FlagFetch,
    room: Room,
) -> Result<Vec<Flagged>, SessionError> {
    selection.messages()?;
    let command = command(fetch)?;
    let mut flagged = BTreeMap::new();
    let visit = |response: &Response<'_>| {
        if let Response::Fetch(_, attributes) = response
            && let Some(found) = line(attributes)?
        {
            if flagged.len() == MAX_FLAGGED && !flagged.contains_key(&found.uid) {
                return Err(SessionError::Protocol(MESSAGE_LIMIT));
            }
            flagged.insert(found.uid, found);
        }
        Ok(())
    };
    selection
        .collect(&command, room.bounds(MAX_FLAGGED), visit)
        .await?;
    Ok(flagged.into_values().collect())
}

/// One FETCH line; `None` without a UID or without flags.
fn line(attributes: &[AttributeValue<'_>]) -> Result<Option<Flagged>, SessionError> {
    let (mut uid, mut flags) = (None, None);
    for attribute in attributes {
        match attribute {
            AttributeValue::Uid(value) => uid = Some(*value),
            AttributeValue::Flags(found) => flags = Some(kept_flags(found)),
            AttributeValue::ModSeq(value) => {
                modseq(*value)?;
            }
            _ => {}
        }
    }
    Ok(uid.zip(flags).map(|(uid, flags)| Flagged { uid, flags }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::UidRange;

    fn read(line_bytes: &str) -> Result<Option<Flagged>, SessionError> {
        match Response::from_bytes(line_bytes.as_bytes()).unwrap().1 {
            Response::Fetch(_, attributes) => line(&attributes),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_two_commands_ask_for_the_flags_alone_rfc7162_3_1_4_1() {
        assert_eq!(
            command(FlagFetch::ChangedSince(41)).unwrap(),
            "UID FETCH 1:* (UID FLAGS) (CHANGEDSINCE 41)"
        );
        assert_eq!(
            command(FlagFetch::ChangedSince(0)).unwrap(),
            "UID FETCH 1:* (UID FLAGS) (CHANGEDSINCE 1)"
        );
        let range = UidRange { low: 3, high: 9 };
        assert_eq!(
            command(FlagFetch::Range(range)).unwrap(),
            "UID FETCH 3:9 (UID FLAGS)"
        );
        let backward = UidRange { low: 9, high: 3 };
        assert!(command(FlagFetch::Range(backward)).is_err());
        assert!(command(FlagFetch::ChangedSince(u64::MAX)).is_err());
    }

    #[test]
    fn a_line_needs_its_uid_and_its_flags_and_bounds_its_mod_sequence() {
        let found = read("* 2 FETCH (UID 7 FLAGS (\\Seen $Junk) MODSEQ (12))\r\n");
        assert_eq!(
            found.unwrap(),
            Some(Flagged {
                uid: 7,
                flags: vec!["\\Seen".to_owned(), "$Junk".to_owned()]
            })
        );
        assert_eq!(read("* 2 FETCH (FLAGS (\\Seen))\r\n").unwrap(), None);
        assert_eq!(read("* 2 FETCH (UID 7)\r\n").unwrap(), None);
        assert!(matches!(
            read("* 2 FETCH (UID 7 FLAGS () MODSEQ (9223372036854775808))\r\n"),
            Err(SessionError::Protocol("a mod-sequence passes 63 bits"))
        ));
    }
}
