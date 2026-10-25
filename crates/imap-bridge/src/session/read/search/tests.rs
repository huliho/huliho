// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The windows, the terms and the key builder of a search; what the
//! scripted server reads off a built command whatever a client typed.

use proptest::prelude::*;
use time::{Date, Month};

use super::*;
use crate::session::{GmailSearch, MAX_STRUCTURED_BYTES, Searching};
use crate::testing::command::{self, Command, Continuation, Reading};
use crate::testing::messages::search::{Key, parse};

/// The bytes one UID takes in a SEARCH answer at most: a space and ten
/// digits.
const UID_WIRE_BYTES: usize = 11;

/// The window every built command below names.
const WINDOW: (u32, u32) = (1, 3);

fn ask<'a>(text: &'a str, from: &'a str) -> SearchAsk<'a> {
    SearchAsk {
        text,
        from,
        unseen: false,
        flagged: false,
        since: None,
        before: None,
        gmail: None,
        server: Searching {
            literals: Literals::Plus,
            gmail: true,
        },
        until: None,
    }
}

fn built(ask: &SearchAsk<'_>) -> Result<Segments, SessionError> {
    segments(&keys(ask)?, WINDOW, ask.server.literals)
}

/// The command as a server receives it under a tag, each later segment
/// after the continuation request it asked with.
fn wire(command: &Segments) -> String {
    let mut segments = vec![command.first.as_str()];
    segments.extend(command.later.iter().map(String::as_str));
    format!("A1 {}\r\n", segments.join("\r\n"))
}

/// What the scripted server reads off the wire as one command it takes;
/// the read uses the bytes whole, so nothing is left for a second one.
fn served(wire: &str, literals: Literals) -> Command {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let reading = Reading {
        literals,
        continuation: Continuation::Asks,
    };
    runtime.block_on(async {
        let mut reader = tokio::io::BufReader::new(wire.as_bytes());
        let mut asked = Vec::new();
        let read = command::read(&mut reader, &mut asked, reading, |_| {}).await;
        let mut rest = Vec::new();
        tokio::io::AsyncReadExt::read_to_end(&mut reader, &mut rest)
            .await
            .unwrap();
        assert!(rest.is_empty(), "{rest:?}");
        read.unwrap()
    })
}

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
            Err(SessionError::Protocol(UID_LIMIT))
        ));
    }
}

#[test]
fn a_list_past_the_uid_limit_fails() {
    let mut uids = vec![1; MAX_FOLDER_UIDS - 1];
    hold(&mut uids, &[2]).unwrap();
    assert!(matches!(
        hold(&mut uids, &[3]),
        Err(SessionError::Protocol(UID_LIMIT))
    ));
}

#[test]
fn a_uid_of_zero_never_enters_the_list_rfc3501_2_3_1_1() {
    let mut uids = Vec::new();
    hold(&mut uids, &[0, 7, 0, 9]).unwrap();
    assert_eq!(uids, [7, 9]);
}

#[test]
fn a_text_splits_on_whitespace_and_a_quoted_run_is_one_term() {
    assert_eq!(terms("  q3 \t plan\r\nzoë "), ["q3", "plan", "zoë"]);
    assert_eq!(
        terms("\"q3 plan\" from:\"Anna J\" x"),
        ["\"q3 plan\"", "from:\"Anna J\"", "x"]
    );
    assert_eq!(terms("a \"open to the end"), ["a", "\"open to the end"]);
    assert_eq!(terms("\"\" \"\"\"\" a"), ["a"]);
    assert_eq!(terms(""), Vec::<&str>::new());
    assert_eq!(bare("from:\"Anna J\""), "from:Anna J");
}

#[test]
fn debug_output_counts_the_terms_and_prints_neither_a_term_nor_the_label() {
    let searched = SearchAsk {
        unseen: true,
        gmail: Some(GmailSearch {
            label: Some("Receipts"),
        }),
        ..ask("xylophone \"quarterly plan\"", "anna.jansen@example.org")
    };
    let printed = format!("{searched:?}");
    for word in [
        "xylophone",
        "quarterly",
        "plan",
        "anna",
        "jansen",
        "example",
        "Receipts",
    ] {
        assert!(!printed.contains(word), "{printed}");
    }
    assert_eq!(
        printed,
        "SearchAsk { text_terms: 2, from_terms: 1, unseen: true, flagged: false, since: None, \
         before: None, gmail: true, label: true, until: false, \
         server: Searching { literals: Plus, gmail: true } }"
    );
}

#[test]
fn a_term_past_the_caps_or_holding_a_nul_never_goes_out() {
    let long = "a".repeat(SEARCH_TERM_BYTES + 1);
    let many = ["t"; SEARCH_TERMS + 1].join(" ");
    let some = ["t"; SEARCH_TERMS - 1].join(" ");
    for (text, from, words) in [
        (long.as_str(), "", TERM_LIMIT),
        ("", long.as_str(), TERM_LIMIT),
        (many.as_str(), "", TERM_LIMIT),
        (some.as_str(), "a b", TERM_LIMIT),
        ("a\0b", "", NUL),
        ("", "\"x\0\"", NUL),
        ("", "", NO_KEY),
        ("\"\"", " ", NO_KEY),
    ] {
        assert!(
            matches!(built(&ask(text, from)), Err(SessionError::Protocol(found)) if found == words),
            "{text:?} {from:?}"
        );
    }
    let at_the_caps = "a".repeat(SEARCH_TERM_BYTES);
    assert!(built(&ask(&at_the_caps, &some[..some.len() - 2])).is_ok());
}

#[test]
fn each_term_goes_as_a_literal_after_charset_and_the_window_rfc3501_6_4_4() {
    let command = built(&ask("zoë \"q3 plan\"", "anna")).unwrap();
    assert_eq!(
        command.first,
        "UID SEARCH CHARSET UTF-8 1:3 TEXT {4+}\r\nzoë TEXT {7+}\r\nq3 plan FROM {4+}\r\nanna"
    );
    assert_eq!(command.later, Vec::<String>::new());
    let plain = SearchAsk {
        unseen: true,
        flagged: true,
        since: Date::from_calendar_date(2026, Month::February, 1).ok(),
        before: Date::from_calendar_date(2026, Month::October, 24).ok(),
        ..ask("", "")
    };
    assert_eq!(
        built(&plain).unwrap().first,
        "UID SEARCH 1:3 UNSEEN FLAGGED SINCE 1-Feb-2026 BEFORE 24-Oct-2026"
    );
}

#[test]
fn a_literal_waits_for_the_continuation_where_the_server_reads_none_without_rfc7888_5() {
    let synchronizing = SearchAsk {
        server: Searching::default(),
        ..ask("a\r\nb", "c")
    };
    let command = built(&synchronizing).unwrap();
    assert_eq!(command.first, "UID SEARCH CHARSET UTF-8 1:3 TEXT {1}");
    assert_eq!(command.later, ["a TEXT {1}", "b FROM {1}", "c"]);
    let long = "x".repeat(5000);
    let keys = [
        Piece::Words("TEXT".to_owned()),
        Piece::Literal(long.clone()),
    ];
    let minus = segments(&keys, WINDOW, Literals::Minus).unwrap();
    assert_eq!(minus.first, "UID SEARCH CHARSET UTF-8 1:3 TEXT {5000}");
    assert_eq!(minus.later, std::slice::from_ref(&long));
    let plus = segments(&keys, WINDOW, Literals::Plus).unwrap();
    assert!(plus.first.ends_with(&format!("{{5000+}}\r\n{long}")));
    let past = [
        Piece::Words("TEXT".to_owned()),
        Piece::Literal("x".repeat(MAX_COMMAND_BYTES)),
    ];
    assert!(matches!(
        segments(&past, WINDOW, Literals::Plus),
        Err(SessionError::Protocol(LINE_LIMIT))
    ));
}

#[test]
fn gmail_takes_the_text_and_the_sender_as_one_raw_string_and_the_label_as_a_literal() {
    let inbox = SearchAsk {
        gmail: Some(GmailSearch {
            label: Some("\\Inbox"),
        }),
        ..ask("invoice -spam label:x", "Anna \"Jansen\"")
    };
    let raw = "invoice -spam label:x from:\"Anna Jansen\"";
    assert_eq!(
        built(&inbox).unwrap().first,
        format!(
            "UID SEARCH CHARSET UTF-8 1:3 X-GM-RAW {{{}+}}\r\n{raw} X-GM-LABELS {{6+}}\r\n\\Inbox",
            raw.len()
        )
    );
    let user = SearchAsk {
        gmail: Some(GmailSearch {
            label: Some("Work \"Q3\""),
        }),
        ..ask("", "")
    };
    assert_eq!(
        built(&user).unwrap().first,
        "UID SEARCH CHARSET UTF-8 1:3 X-GM-LABELS {9+}\r\nWork \"Q3\""
    );
    for label in ["\\All", "\\Trash", "", "a\r\nb"] {
        let refused = SearchAsk {
            gmail: Some(GmailSearch { label: Some(label) }),
            ..ask("x", "")
        };
        assert!(
            matches!(built(&refused), Err(SessionError::Protocol(NO_LABEL))),
            "{label:?}"
        );
    }
    let elsewhere = SearchAsk {
        server: Searching::default(),
        ..inbox
    };
    assert!(matches!(
        built(&elsewhere),
        Err(SessionError::Protocol(NO_GMAIL))
    ));
}

#[test]
fn a_day_goes_out_as_an_imap_date_with_four_digits_of_year_rfc3501_9() {
    let day = |year, month, day| Date::from_calendar_date(year, month, day).unwrap();
    assert_eq!(
        imap_date(day(2026, Month::January, 5)).as_deref(),
        Some("5-Jan-2026")
    );
    assert_eq!(
        imap_date(day(987, Month::December, 31)).as_deref(),
        Some("31-Dec-0987")
    );
    assert_eq!(
        imap_date(day(MAX_YEAR, Month::June, 1)).as_deref(),
        Some("1-Jun-9999")
    );
    assert_eq!(imap_date(day(-1, Month::June, 1)), None);
}

/// Text a client could type: line breaks, quotes, backslashes,
/// parentheses, a NUL, size markers, a second command, characters
/// outside ASCII and terms near the byte cap.
fn typed() -> impl Strategy<Value = String> {
    let piece = prop_oneof![
        Just("\r\n".to_owned()),
        Just("\"".to_owned()),
        Just("\\".to_owned()),
        Just("(".to_owned()),
        Just(")".to_owned()),
        Just("\0".to_owned()),
        Just("{5}".to_owned()),
        Just("{3+}\r\n".to_owned()),
        Just(" ".to_owned()),
        Just("\r\nA9 LOGOUT\r\n".to_owned()),
        Just("é".to_owned()),
        Just("日本".to_owned()),
        "[a-z]{1,8}",
        "[a-z]{250,260}",
        any::<String>(),
    ];
    prop::collection::vec(piece, 0..10).prop_map(|pieces| pieces.concat())
}

fn literals() -> impl Strategy<Value = Literals> {
    prop_oneof![
        Just(Literals::Plus),
        Just(Literals::Minus),
        Just(Literals::Synchronizing),
    ]
}

proptest! {
    #[test]
    fn every_term_reaches_the_server_as_the_same_bytes_and_ends_no_command(
        text in typed(),
        from in typed(),
        literals in literals(),
        gmail in any::<bool>(),
    ) {
        let asked = SearchAsk {
            gmail: gmail.then(GmailSearch::default),
            server: Searching { literals, gmail: true },
            ..ask(&text, &from)
        };
        let (text_terms, from_terms) = (terms(&text), terms(&from));
        let all: Vec<&&str> = text_terms.iter().chain(&from_terms).collect();
        let over = all.len() > SEARCH_TERMS || all.iter().any(|term| term.len() > SEARCH_TERM_BYTES);
        let nul = all.iter().any(|term| term.contains('\0'));
        let command = match built(&asked) {
            Err(SessionError::Protocol(words)) => {
                let expected = if over { TERM_LIMIT } else if nul { NUL } else { NO_KEY };
                prop_assert_eq!(words, expected);
                prop_assert!(over || nul || all.is_empty());
                return Ok(());
            }
            Err(other) => return Err(TestCaseError::fail(other.to_string())),
            Ok(command) => command,
        };
        prop_assert!(!over && !nul);
        let read = served(&wire(&command), literals);
        let search = parse(&read).unwrap();
        prop_assert_eq!(search.charset.as_deref(), Some("UTF-8"));
        let mut expected = vec![Key::Window(1, 3)];
        if gmail {
            expected.push(Key::GmailRaw(raw(&text_terms, &from_terms)));
        } else {
            expected.extend(text_terms.iter().map(|term| Key::Text(bare(term))));
            expected.extend(from_terms.iter().map(|term| Key::From(bare(term))));
        }
        prop_assert_eq!(search.keys, expected);
        let words: Vec<&str> = read.parts.iter().filter_map(|part| match part {
            command::Part::Text(text) => Some(text.as_str()),
            command::Part::Literal(_) => None,
        }).collect();
        prop_assert!(words.iter().all(|text| !text.contains(['\r', '\n', '\0'])), "{words:?}");
    }
}
