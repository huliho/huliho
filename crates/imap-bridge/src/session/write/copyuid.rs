// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The `COPYUID` code of a move or a copy (RFC 4315 section 3): the
//! target's UIDVALIDITY and two UID sets that pair one to one, each read
//! under the bound the ask sets before any of it is expanded.

use async_imap::imap_proto::{Response, ResponseCode, Status, UidSetMember};

use crate::session::{Moved, SessionError};

const UNPAIRED: &str = "COPYUID does not pair with the UIDs asked";
const ZERO: &str = "COPYUID names a zero";

/// Keeps the pairs one line's `COPYUID` names. A move carries it on an
/// untagged OK ahead of its expunges (RFC 6851 section 4.3), a copy on
/// the tagged OK; both count, and a second code adds its pairs to the
/// first. Every source UID is one the ask named and each comes once.
pub(super) fn note(
    moved: &mut Option<Moved>,
    response: &Response<'_>,
    asked: &[u32],
) -> Result<(), SessionError> {
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
    let ResponseCode::CopyUid(uid_validity, source, target) = code else {
        return Ok(());
    };
    let held = moved.get_or_insert(Moved {
        uid_validity: *uid_validity,
        uids: Vec::new(),
    });
    let room = asked.len().saturating_sub(held.uids.len());
    let (source, target) = (expanded(source, room)?, expanded(target, room)?);
    if *uid_validity == 0 || source.contains(&0) || target.contains(&0) {
        return Err(SessionError::Protocol(ZERO));
    }
    if held.uid_validity != *uid_validity || source.len() != target.len() {
        return Err(SessionError::Protocol(UNPAIRED));
    }
    for pair in source.into_iter().zip(target) {
        let taken = held
            .uids
            .iter()
            .any(|(from, to)| *from == pair.0 || *to == pair.1);
        if !asked.contains(&pair.0) || taken {
            return Err(SessionError::Protocol(UNPAIRED));
        }
        held.uids.push(pair);
    }
    Ok(())
}

/// The UIDs of a set in the order it names them, a range from its low
/// end (RFC 4315 section 4 reads `4:2` as `2:4`). Past `room` UIDs it
/// fails before anything is allocated.
fn expanded(set: &[UidSetMember], room: usize) -> Result<Vec<u32>, SessionError> {
    let room = u64::try_from(room).unwrap_or(u64::MAX);
    let mut total: u64 = 0;
    for member in set {
        total += match member {
            UidSetMember::Uid(_) => 1,
            UidSetMember::UidRange(range) => u64::from(range.end() - range.start()) + 1,
        };
        if total > room {
            return Err(SessionError::Protocol(UNPAIRED));
        }
    }
    Ok(set
        .iter()
        .flat_map(|member| match member {
            UidSetMember::Uid(uid) => *uid..=*uid,
            UidSetMember::UidRange(range) => range.clone(),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What these lines' codes name for a move of `asked`.
    fn read(lines: &[&str], asked: &[u32]) -> Result<Option<Moved>, SessionError> {
        let mut moved = None;
        for line in lines {
            let response = Response::from_bytes(line.as_bytes()).unwrap().1;
            note(&mut moved, &response, asked)?;
        }
        Ok(moved)
    }

    fn pairs(lines: &[&str], asked: &[u32]) -> Vec<(u32, u32)> {
        read(lines, asked).unwrap().unwrap().uids
    }

    #[test]
    fn the_two_sets_pair_in_order_on_either_line_rfc4315_3() {
        let moved = read(
            &["* OK [COPYUID 38505 304,319:320 3956:3958] Moved\r\n"],
            &[320, 304, 319],
        )
        .unwrap()
        .unwrap();
        assert_eq!(moved.uid_validity, 38505);
        assert_eq!(moved.uids, [(304, 3956), (319, 3957), (320, 3958)]);
        let copied = pairs(&["A4 OK [COPYUID 9 7 1] Copied\r\n"], &[7]);
        assert_eq!(copied, [(7, 1)]);
        assert_eq!(read(&["A4 OK Copied\r\n"], &[7]).unwrap(), None);
    }

    #[test]
    fn a_uid_that_names_no_message_has_no_pair_rfc3501_6_4_8() {
        let found = pairs(&["* OK [COPYUID 9 2,4 5:6] Moved\r\n"], &[2, 3, 4]);
        assert_eq!(found, [(2, 5), (4, 6)]);
    }

    #[test]
    fn two_codes_of_one_answer_add_up() {
        let lines = [
            "* OK [COPYUID 9 2 5] Moved\r\n",
            "A1 OK [COPYUID 9 3 6] Done\r\n",
        ];
        assert_eq!(pairs(&lines, &[2, 3]), [(2, 5), (3, 6)]);
    }

    #[test]
    fn a_code_that_does_not_pair_with_the_ask_is_a_protocol_failure() {
        for (code, asked) in [
            // Fewer UIDs in the target than in the source, then more.
            ("COPYUID 9 2:3 5", &[2, 3][..]),
            ("COPYUID 9 2 5:6", &[2, 3]),
            // A source UID the ask never named.
            ("COPYUID 9 2,8 5:6", &[2, 3]),
            // One source or one target UID twice.
            ("COPYUID 9 2,2 5:6", &[2, 3]),
            ("COPYUID 9 2:3 5,5", &[2, 3]),
            // More pairs than the ask has messages.
            ("COPYUID 9 2:4 5:7", &[2, 3]),
        ] {
            let line = format!("* OK [{code}] Moved\r\n");
            assert!(
                matches!(read(&[&line], asked), Err(SessionError::Protocol(UNPAIRED))),
                "{code}"
            );
        }
        let lines = [
            "* OK [COPYUID 9 2 5] Moved\r\n",
            "A1 OK [COPYUID 8 3 6] Done\r\n",
        ];
        assert!(matches!(
            read(&lines, &[2, 3]),
            Err(SessionError::Protocol(UNPAIRED))
        ));
        let again = [
            "* OK [COPYUID 9 2 5] Moved\r\n",
            "A1 OK [COPYUID 9 2 5] Done\r\n",
        ];
        assert!(read(&again, &[2, 3]).is_err());
    }

    #[test]
    fn a_zero_is_a_protocol_failure_rfc3501_2_3_1_1() {
        for code in ["COPYUID 9 2 0", "COPYUID 9 0:2 4:6", "COPYUID 0 2 5"] {
            let line = format!("* OK [{code}] Moved\r\n");
            assert!(
                matches!(
                    read(&[&line], &[0, 1, 2]),
                    Err(SessionError::Protocol(ZERO))
                ),
                "{code}"
            );
        }
    }

    #[test]
    fn a_range_past_the_ask_fails_before_it_is_expanded() {
        let line = "* OK [COPYUID 9 1:4294967295 1:4294967295] Moved\r\n";
        assert!(matches!(
            read(&[line], &[1]),
            Err(SessionError::Protocol(UNPAIRED))
        ));
    }
}
