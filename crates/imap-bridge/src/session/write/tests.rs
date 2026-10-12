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
    stored_as(StoreItem::Flags, uids, names, add)
}

fn labeled(uids: &[u32], names: &[&str], add: bool) -> Result<String, SessionError> {
    stored_as(StoreItem::Labels, uids, names, add)
}

fn stored_as(
    item: StoreItem,
    uids: &[u32],
    names: &[&str],
    add: bool,
) -> Result<String, SessionError> {
    let flags = flags(names);
    store_command(&StoreAsk {
        uids,
        flags: &flags,
        add,
        item,
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
fn a_label_store_names_a_system_label_as_it_is_and_a_user_label_quoted() {
    assert_eq!(
        labeled(&[4, 5], &["\\Inbox"], false).unwrap(),
        "UID STORE 4:5 -X-GM-LABELS.SILENT (\\Inbox)"
    );
    assert_eq!(
        labeled(&[7], &["\\Starred", "Work/Q3", "Say \"hi\""], true).unwrap(),
        "UID STORE 7 +X-GM-LABELS.SILENT (\\Starred \"Work/Q3\" \"Say \\\"hi\\\"\")"
    );
}

#[test]
fn a_label_gmail_would_read_as_another_system_label_never_goes_out() {
    let long = "l".repeat(4097);
    for label in [
        "",
        "\\All",
        "\\Trash",
        "\\Spam",
        "\\Deleted",
        "\\inbox",
        "\\Flagged",
        "\\Drafts",
        "a\r\nA9 EXPUNGE",
        "a\0b",
        long.as_str(),
    ] {
        assert!(
            matches!(
                labeled(&[1], &[label], true),
                Err(SessionError::Protocol(NO_FLAG))
            ),
            "{label:?}"
        );
    }
    assert!(matches!(
        labeled(&[1], &[], true),
        Err(SessionError::Protocol(NO_FLAG))
    ));
}

#[test]
fn a_label_measures_as_the_command_line_carries_it_rfc9051_4_3() {
    let frame = "UID STORE 7 +X-GM-LABELS.SILENT ()".len();
    for label in ["\\Inbox", "Work/Q3", "Say \"hi\"", "back\\slash"] {
        let command = labeled(&[7], &[label], true).unwrap();
        assert_eq!(label_bytes(label), Some(command.len() - frame), "{label:?}");
    }
    assert_eq!(label_bytes("Say \"hi\""), Some("\"Say \\\"hi\\\"\"".len()));
    for label in ["", "\\All", "a\0b"] {
        assert_eq!(label_bytes(label), None, "{label:?}");
    }
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

/// A flag or a label a client could ask for, the spellings of `\Deleted`
/// and `\All` among them.
fn flag() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("\\Deleted".to_owned()),
        Just("\\deleted".to_owned()),
        Just("\\DELETED".to_owned()),
        Just("\\Seen".to_owned()),
        Just("\\All".to_owned()),
        Just("\\Inbox".to_owned()),
        Just("a\" +FLAGS (\\Deleted".to_owned()),
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

/// The flags or labels of a STORE line as the server reads them: an
/// atom up to the next space, a quoted string up to its closing quote
/// (RFC 9051 section 4.3).
fn stored(command: &str) -> Vec<String> {
    let (_, list) = command.split_once(".SILENT (").unwrap();
    let mut chars = list.strip_suffix(')').unwrap().chars();
    let mut items = Vec::new();
    while let Some(first) = chars.next() {
        let mut item = String::new();
        if first == '"' {
            while let Some(c) = chars.next() {
                match c {
                    '\\' => item.extend(chars.next()),
                    '"' => break,
                    other => item.push(other),
                }
            }
            chars.next();
        } else {
            item.push(first);
            item.extend(chars.by_ref().take_while(|c| *c != ' '));
        }
        items.push(item);
    }
    items
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
        for store in [line(&uids, &names, add), labeled(&uids, &names, add)]
            .into_iter()
            .flatten()
        {
            prop_assert!(store.starts_with("UID STORE "));
            let items = stored(&store);
            prop_assert!(!items.iter().any(|item| item.eq_ignore_ascii_case(DELETED)));
            prop_assert!(!items.iter().any(|item| item.eq_ignore_ascii_case("\\All")));
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
