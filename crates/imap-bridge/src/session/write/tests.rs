// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The command builders of the write path against what a client or a
//! server could hand them.

use proptest::prelude::*;

use super::*;

fn flags(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

fn line(uids: &[u32], names: &[&str], add: bool) -> Result<String, SessionError> {
    let flags = flags(names);
    store_command(&StoreAsk {
        uids,
        flags: &flags,
        add,
    })
}

fn moving(transfer: Transfer, uids: &[u32], target: &str) -> Result<String, SessionError> {
    transfer_command(transfer, &MoveAsk { uids, target })
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
fn a_write_names_one_message_at_least_and_a_bounded_set() {
    let many: Vec<u32> = (1..=u32::try_from(MAX_STORE_UIDS).unwrap() + 1).collect();
    for uids in [&[][..], &[0], &[4, 0], &many] {
        assert!(matches!(
            line(uids, &["\\Seen"], true),
            Err(SessionError::Protocol(NO_MESSAGE))
        ));
        assert!(matches!(
            moving(Transfer::Move, uids, "Archive"),
            Err(SessionError::Protocol(NO_MESSAGE))
        ));
        assert!(matches!(
            expunge_commands(uids),
            Err(SessionError::Protocol(NO_MESSAGE))
        ));
    }
    assert!(line(&many[1..], &["\\Seen"], true).is_ok());
    assert!(moving(Transfer::Copy, &many[1..], "Archive").is_ok());
}

#[test]
fn a_command_past_the_line_limit_is_never_sent_rfc7162_4() {
    let long = "k".repeat(MAX_COMMAND_BYTES);
    assert!(matches!(
        line(&[1], &[&long], true),
        Err(SessionError::Protocol(LINE_LIMIT))
    ));
    // Every quote of a name sendable at its full length is escaped.
    let quotes = "\"".repeat(4096);
    assert!(matches!(
        moving(Transfer::Move, &[1], &quotes),
        Err(SessionError::Protocol(LINE_LIMIT))
    ));
}

#[test]
fn a_move_and_a_copy_name_the_target_as_a_quoted_string_rfc6851_3_2() {
    assert_eq!(
        moving(Transfer::Move, &[9, 4, 5], "Archive").unwrap(),
        "UID MOVE 4:5,9 \"Archive\""
    );
    assert_eq!(
        moving(Transfer::Copy, &[3], "Say \"hi\"").unwrap(),
        "UID COPY 3 \"Say \\\"hi\\\"\""
    );
    for target in ["a\r\nA9 EXPUNGE", "a\0b", &"a".repeat(4097)] {
        assert!(moving(Transfer::Move, &[1], target).is_err(), "{target:?}");
    }
}

#[test]
fn an_expunge_stores_deleted_and_expunges_the_same_set_alone_rfc4315_2_1() {
    assert_eq!(
        expunge_commands(&[12, 10, 11]).unwrap(),
        [
            "UID STORE 10:12 +FLAGS.SILENT (\\Deleted)".to_owned(),
            "UID EXPUNGE 10:12".to_owned()
        ]
    );
}

/// A flag a client could ask for, the spellings of `\Deleted` among them.
fn flag() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("\\Deleted".to_owned()),
        Just("\\deleted".to_owned()),
        Just("\\DELETED".to_owned()),
        Just("\\Seen".to_owned()),
        "[ -~]{0,12}",
        any::<String>(),
    ]
}

/// A mailbox name a server could list, the shapes of a smuggled command
/// among them.
fn target() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("EXPUNGE".to_owned()),
        Just("\\Deleted".to_owned()),
        Just("a\r\nA9 EXPUNGE".to_owned()),
        Just("a\" +FLAGS (\\Deleted)".to_owned()),
        "[ -~]{0,24}",
        any::<String>(),
    ]
}

/// The flags of a STORE line.
fn stored(command: &str) -> Vec<&str> {
    let (_, list) = command.split_once("FLAGS.SILENT (").unwrap();
    list.trim_end_matches(')').split(' ').collect()
}

proptest! {
    #[test]
    fn deleted_is_stored_by_the_expunge_alone_and_no_plain_expunge_is_built(
        uids in prop::collection::vec(any::<u32>(), 0..=MAX_STORE_UIDS + 1),
        flags in prop::collection::vec(flag(), 0..4),
        target in target(),
        add in any::<bool>(),
    ) {
        let names: Vec<&str> = flags.iter().map(String::as_str).collect();
        let mut built = Vec::new();
        if let Ok(store) = line(&uids, &names, add) {
            prop_assert!(store.starts_with("UID STORE "));
            prop_assert!(!stored(&store).iter().any(|flag| flag.eq_ignore_ascii_case(DELETED)));
            built.push(store);
        }
        for transfer in [Transfer::Move, Transfer::Copy] {
            if let Ok(command) = moving(transfer, &uids, &target) {
                let verb = if transfer == Transfer::Move { "UID MOVE " } else { "UID COPY " };
                prop_assert!(command.starts_with(verb));
                built.push(command);
            }
        }
        if let Ok([store, expunge]) = expunge_commands(&uids) {
            let set = sequence_set(&uids);
            prop_assert_eq!(&store, &format!("UID STORE {set} +FLAGS.SILENT (\\Deleted)"));
            prop_assert_eq!(&expunge, &format!("UID EXPUNGE {set}"));
            built.extend([store, expunge]);
        }
        for command in &built {
            prop_assert!(!command.contains(['\r', '\n', '\0']), "{command:?}");
            prop_assert!(!command.to_ascii_uppercase().starts_with("EXPUNGE"));
            prop_assert!(command.len() <= MAX_COMMAND_BYTES);
        }
    }
}
