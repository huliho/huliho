// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Gmail's rules on a name, as its answers read on the test account.

use proptest::prelude::*;

use super::*;

/// The white space a name may hold: Unicode's `White_Space` without its
/// control characters, which every server's check refuses first.
const WHITE_SPACE: [char; 19] = [
    ' ', '\u{a0}', '\u{1680}', '\u{2000}', '\u{2001}', '\u{2002}', '\u{2003}', '\u{2004}',
    '\u{2005}', '\u{2006}', '\u{2007}', '\u{2008}', '\u{2009}', '\u{200a}', '\u{2028}', '\u{2029}',
    '\u{202f}', '\u{205f}', '\u{3000}',
];

/// What looks like a space and is none: Gmail takes these in a row.
const NOT_WHITE_SPACE: [char; 4] = ['\u{180e}', '\u{200b}', '\u{2060}', '\u{feff}'];

/// What Gmail reads as a space between two letters, the full-width forms of
/// two of them and white space among them.
const READ_AS_SPACE: [char; 13] = [
    ' ', '"', '&', '(', ')', '-', '{', '|', '}', '\u{ff0d}', '\u{ff08}', '\u{a0}', '\u{3000}',
];

/// Characters of one, two, three and four bytes of UTF-8.
const WIDTHS: [char; 4] = ['x', '\u{e9}', '\u{6f22}', '\u{1f600}'];

/// The most characters a drawn path holds: past either limit in any width.
const DRAWN_CHARACTERS: usize = 300;

/// Emoji beside `ASCII_BESIDE_EMOJI` letters: a path at the units within the
/// bytes, though its characters stay under the units.
const EMOJI_IN_A_PATH: usize = 14;

/// The letters that fill the units beside `EMOJI_IN_A_PATH` emoji.
const ASCII_BESIDE_EMOJI: usize = 197;

fn white_space() -> impl Strategy<Value = char> {
    prop::sample::select(WHITE_SPACE.to_vec())
}

fn part() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9]{1,8}"
}

/// A path of characters as wide as `WIDTHS` draws them.
fn path() -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(WIDTHS.to_vec()), 1..=DRAWN_CHARACTERS)
        .prop_map(|characters| characters.into_iter().collect())
}

#[test]
fn a_whole_path_reaches_each_limit_where_gmail_answers_its_no() {
    let fits = "x".repeat(MAX_PATH_UNITS);
    assert!(!past_limit(&fits));
    assert!(past_limit(&format!("{fits}x")), "the units");
    let accents = "\u{e9}".repeat(MAX_PATH_BYTES / 2);
    assert!(!past_limit(&utf7::encode(&format!("{accents}x"))));
    assert!(
        past_limit(&utf7::encode(&format!("{accents}xx"))),
        "the bytes"
    );
    let under = format!("Parent/{}", "x".repeat(MAX_PATH_UNITS));
    assert!(past_limit(&under), "the parents count");
    let wide = "\u{1f600}".repeat(EMOJI_IN_A_PATH);
    let at_units = format!("{}{wide}", "x".repeat(ASCII_BESIDE_EMOJI));
    assert!(!past_limit(&utf7::encode(&at_units)));
    let past_units = format!("{}{wide}", "x".repeat(ASCII_BESIDE_EMOJI + 1));
    assert!(past_limit(&utf7::encode(&past_units)), "two units each");
}

proptest! {
    /// Any two white space characters in a row are refused, whatever stands
    /// around them.
    #[test]
    fn white_space_in_a_row_is_crowded_rfc8621_2(
        (left, right) in (part(), part()),
        (first, second) in (white_space(), white_space()),
    ) {
        let name = format!("{left}{first}{second}{right}");
        prop_assert!(crowded(&name));
    }

    /// White space that stands alone is taken however often it comes. What
    /// only looks like a space is taken in a row.
    #[test]
    fn white_space_alone_or_a_look_alike_is_not_crowded_rfc8621_2(
        (left, middle, right) in (part(), part(), part()),
        (first, second) in (white_space(), white_space()),
        alike in prop::sample::select(NOT_WHITE_SPACE.to_vec()),
    ) {
        let alone = format!("{left}{first}{middle}{second}{right}");
        prop_assert!(!crowded(&alone));
        let in_a_row = format!("{left}{alike}{alike}{right}");
        prop_assert!(!crowded(&in_a_row));
        let between = format!("{left} {alike} {right}");
        prop_assert!(!crowded(&between));
    }

    /// A path is past the limit by its characters as the user reads them,
    /// never by the longer form on the wire.
    #[test]
    fn a_path_is_measured_as_the_user_reads_it_not_as_the_wire_spells_it(path in path()) {
        let wanted = path.len() > MAX_PATH_BYTES || path.encode_utf16().count() > MAX_PATH_UNITS;
        prop_assert_eq!(past_limit(&utf7::encode(&path)), wanted);
    }

    /// Everything Gmail reads as a space reads as one name beside the others.
    #[test]
    fn what_gmail_reads_as_a_space_reads_as_one_after_its_no(
        (left, right) in (part(), part()),
        marks in (
            prop::sample::select(READ_AS_SPACE.to_vec()),
            prop::sample::select(READ_AS_SPACE.to_vec()),
        ),
    ) {
        let (first, second) = marks;
        prop_assert_eq!(
            beside(&format!("{left}{first}{right}")),
            beside(&format!("{left}{second}{right}")),
        );
    }

    /// Different letters stay different names, whatever stands between them.
    /// Letters run together stay another name too, since Gmail takes `a-b`
    /// beside `ab`.
    #[test]
    fn different_letters_stay_different_after_its_no(
        (left, right) in (part(), part()),
        mark in prop::sample::select(READ_AS_SPACE.to_vec()),
    ) {
        let name = format!("{left}{mark}{right}");
        prop_assert_ne!(beside(&name), beside(&format!("{name}z")));
        prop_assert_ne!(beside(&name), beside(&format!("{left}{right}")));
    }
}
