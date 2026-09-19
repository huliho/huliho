// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The bound on the bytes of a response outside its literals, against
//! the shapes the parser pays most for.

use super::super::lexer::Lexer;
use super::super::{Limit, MAX_STRUCTURED_BYTES};
use super::{LEAF, fed};

/// A literal the size of a message body.
const BODY_BYTES: usize = 8 * 1024 * 1024;

/// The shapes the parser pays most for, each as an opening, the unit it
/// repeats and a closing.
const COSTLY: [(&str, &str, &str); 8] = [
    ("* 1 FETCH (BODYSTRUCTURE (", LEAF, " \"MIXED\"))\r\n"),
    ("* 1 FETCH (UID 1", " UID 1", ")\r\n"),
    ("* 1 FETCH (FLAGS (a", " a", "))\r\n"),
    ("* CAPABILITY IMAP4rev1", " a", "\r\n"),
    ("* LIST (\\a", " \\a", ") \"/\" x\r\n"),
    ("* OK [PERMANENTFLAGS (a", " a", ")] x\r\n"),
    ("A1 OK [CAPABILITY IMAP4rev1", " a", "] x\r\n"),
    ("+ ", "a", "\r\n"),
];

/// A response of exactly `bytes` bytes: the unit as often as fits, `x`
/// for the rest.
fn filled((opening, unit, closing): (&str, &str, &str), bytes: usize) -> String {
    let room = bytes - opening.len() - closing.len();
    let units = unit.repeat(room / unit.len());
    let rest = "x".repeat(room % unit.len());
    format!("{opening}{units}{rest}{closing}")
}

#[test]
fn structure_at_the_bound_passes_response_after_response_and_one_byte_more_trips() {
    let mut lexer = Lexer::default();
    for shape in COSTLY {
        let line = filled(shape, MAX_STRUCTURED_BYTES);
        assert_eq!(line.len(), MAX_STRUCTURED_BYTES);
        assert_eq!(lexer.feed(line.as_bytes()), Ok(()), "{shape:?}");
    }
    for shape in COSTLY {
        let line = filled(shape, MAX_STRUCTURED_BYTES + 1);
        assert_eq!(fed(line.as_bytes()), Err(Limit::Structure), "{shape:?}");
    }
}

#[test]
fn a_literal_weighs_nothing_toward_the_structure_bound() {
    let mut lexer = Lexer::default();
    let opening = format!("* 1 FETCH (UID 1 BODY[] {{{BODY_BYTES}}}\r\n");
    assert_eq!(lexer.feed(opening.as_bytes()), Ok(()));
    assert_eq!(lexer.feed(&vec![b'x'; BODY_BYTES]), Ok(()));
    assert_eq!(lexer.feed(b")\r\n"), Ok(()));
}

#[test]
fn the_structure_on_both_sides_of_a_literal_adds_up() {
    let payload = "abc";
    let line = |filling: usize| {
        let before = "x".repeat(filling / 2);
        let after = "x".repeat(filling - filling / 2);
        let size = payload.len();
        format!("* X {before} {{{size}}}\r\n{payload} {after}\r\n")
    };
    let filling = MAX_STRUCTURED_BYTES - (line(0).len() - payload.len());
    assert_eq!(fed(line(filling).as_bytes()), Ok(()));
    assert_eq!(fed(line(filling + 1).as_bytes()), Err(Limit::Structure));
}

#[test]
fn literals_inside_a_response_code_cannot_carry_a_response_past_the_bounds() {
    let small = "{1}\r\na ".repeat(MAX_STRUCTURED_BYTES);
    for opening in [
        "* OK [BADCHARSET (",
        "A1 NO [BADCHARSET (",
        "+ [BADCHARSET (",
    ] {
        let line = format!("{opening}{small})] x\r\n");
        assert_eq!(fed(line.as_bytes()), Err(Limit::Literal), "{opening}");
    }
}
