// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

use proptest::prelude::*;

use super::*;
use crate::session::read::MAX_WIRE_NAME_BYTES;

/// The bytes LITERAL- reads without waiting (RFC 7888 section 5).
const LITERAL_MINUS_BYTES: usize = 4096;

fn ask<'a>(message: &'a [u8], flags: &'a [String], literals: Literals) -> AppendAsk<'a> {
    AppendAsk {
        mailbox: "Drafts",
        flags,
        message,
        literals,
        until: None,
    }
}

fn flags() -> Vec<String> {
    vec!["\\Draft".to_owned(), "\\Seen".to_owned()]
}

#[test]
fn the_size_marker_follows_the_literals_the_server_reads_rfc7888_3() {
    let flags = flags();
    let cases = [
        (Literals::Plus, LITERAL_MINUS_BYTES + 1, "+"),
        (Literals::Minus, LITERAL_MINUS_BYTES, "+"),
        (Literals::Minus, LITERAL_MINUS_BYTES + 1, ""),
        (Literals::Synchronizing, 1, ""),
    ];
    for (literals, size, plus) in cases {
        let message = vec![b'x'; size];
        assert_eq!(
            line("APPEND", &ask(&message, &flags, literals)).unwrap(),
            format!("APPEND \"Drafts\" (\\Draft \\Seen) {{{size}{plus}}}")
        );
    }
}

#[test]
fn an_append_without_flags_names_none_rfc3501_6_3_11() {
    assert_eq!(
        line("APPEND", &ask(b"x", &[], Literals::Plus)).unwrap(),
        "APPEND \"Drafts\" {1+}"
    );
}

#[test]
fn a_flag_outside_the_fixed_set_never_reaches_the_line() {
    for flag in [
        "\\Deleted",
        "\\Recent",
        "$custom",
        "\\Seen) {9+}\r\nA2 NOOP",
        "",
    ] {
        let flags = [flag.to_owned()];
        assert!(
            matches!(
                line("APPEND", &ask(b"x", &flags, Literals::Plus)),
                Err(SessionError::Protocol(NO_FLAG))
            ),
            "{flag}"
        );
    }
}

#[test]
fn a_message_holding_a_nul_is_never_sent_rfc3501_9() {
    assert!(matches!(
        line("APPEND", &ask(b"a\0b", &[], Literals::Plus)),
        Err(SessionError::Protocol(NUL))
    ));
}

#[test]
fn a_mailbox_that_cannot_be_quoted_or_a_line_past_the_limit_sends_nothing() {
    let refused = AppendAsk {
        mailbox: "a\r\nA2 NOOP",
        ..ask(b"x", &[], Literals::Plus)
    };
    assert!(line("APPEND", &refused).is_err());
    let long = "\"".repeat(MAX_WIRE_NAME_BYTES);
    let long = AppendAsk {
        mailbox: &long,
        ..ask(b"x", &[], Literals::Plus)
    };
    assert!(line("APPEND", &long).is_err());
}

/// What these lines' codes name.
fn read(lines: &[&str]) -> Result<Option<Appended>, SessionError> {
    let mut appended = None;
    for line in lines {
        let response = Response::from_bytes(line.as_bytes()).unwrap().1;
        note(&mut appended, &response)?;
    }
    Ok(appended)
}

#[test]
fn the_code_counts_on_the_tagged_ok_and_on_an_untagged_one_rfc4315_3_rfc8508_3_2() {
    let named = Some(Appended {
        uid_validity: 38505,
        uid: 3955,
    });
    assert_eq!(
        read(&["A1 OK [APPENDUID 38505 3955] done\r\n"]).unwrap(),
        named
    );
    assert_eq!(
        read(&["* OK [APPENDUID 38505 3955] Replacement Message ready\r\n"]).unwrap(),
        named
    );
    assert_eq!(read(&["A1 OK done\r\n"]).unwrap(), None);
    assert_eq!(read(&["* OK [COPYUID 9 1 2] Moved\r\n"]).unwrap(), None);
}

#[test]
fn a_code_that_names_no_one_message_is_a_protocol_failure() {
    for code in [
        "APPENDUID 0 5",
        "APPENDUID 9 0",
        "APPENDUID 9 5:6",
        "APPENDUID 9 5,6",
    ] {
        let line = format!("A1 OK [{code}] done\r\n");
        assert!(
            matches!(read(&[&line]), Err(SessionError::Protocol(APPENDUID))),
            "{code}"
        );
    }
}

proptest! {
    /// Whatever the message holds, the line carries the count alone.
    #[test]
    fn no_byte_of_a_message_reaches_the_command_line_rfc3501_4_3(
        message in prop::collection::vec(1u8.., 0..256),
    ) {
        let written = line("APPEND", &ask(&message, &[], Literals::Plus)).unwrap();
        prop_assert_eq!(written, format!("APPEND \"Drafts\" {{{}+}}", message.len()));
    }
}
