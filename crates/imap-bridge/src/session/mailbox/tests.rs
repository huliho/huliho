// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A typed name on its way to a command line, and what the scripted
//! server reads off that line whatever the user typed.

use async_imap::imap_proto::Response;
use proptest::prelude::*;

use super::*;
use crate::session::write::LINE_LIMIT;
use crate::session::{Literals, MAX_COMMAND_BYTES};
use crate::testing::command::{self, Continuation, Reading};
use crate::testing::mailboxes::arguments;

const TOP: Place<'static> = Place {
    parent: None,
    delimiter: Some('/'),
};

fn under(parent: &str) -> Place<'_> {
    Place {
        parent: Some(parent),
        delimiter: Some('/'),
    }
}

/// The names the scripted server reads off a command it took whole,
/// with nothing left on the wire for a second one.
fn served(command: &str) -> Vec<String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let reading = Reading {
        literals: Literals::Synchronizing,
        continuation: Continuation::Asks,
    };
    let wire = format!("A1 {command}\r\n");
    let line = runtime.block_on(async {
        let mut reader = tokio::io::BufReader::new(wire.as_bytes());
        let mut asked = Vec::new();
        let read = command::read(&mut reader, &mut asked, reading, |_| {}).await;
        let mut rest = Vec::new();
        tokio::io::AsyncReadExt::read_to_end(&mut reader, &mut rest)
            .await
            .unwrap();
        assert!(rest.is_empty() && asked.is_empty(), "{rest:?} {asked:?}");
        read.unwrap().line()
    });
    let (_, rest) = line.split_once(' ').unwrap();
    let (_, rest) = rest.split_once(' ').unwrap();
    let (names, after) = arguments(rest).unwrap();
    assert!(after.is_empty(), "{after:?}");
    names
}

#[test]
fn a_name_goes_out_in_modified_utf7_under_its_parent_rfc3501_5_1_3() {
    let name = MailboxName::new("台北", under("~peter/mail")).unwrap();
    assert_eq!(name.wire(), "~peter/mail/&U,BTFw-");
    assert_eq!(name.leaf(), "台北");
    let top = MailboxName::new("Rock & Roll", TOP).unwrap();
    assert_eq!(top.wire(), "Rock &- Roll");
    let flat = Place {
        parent: None,
        delimiter: None,
    };
    assert_eq!(MailboxName::new("a/b.c", flat).unwrap().wire(), "a/b.c");
}

#[test]
fn a_typed_name_is_kept_in_nfc_rfc8621_2() {
    let name = MailboxName::new("Cafe\u{301}", TOP).unwrap();
    assert_eq!(name.leaf(), "Caf\u{e9}");
    assert_eq!(name.wire(), "Caf&AOk-");
}

#[test]
fn a_quote_and_a_backslash_go_out_escaped_rfc9051_4_3() {
    let name = MailboxName::new("say \"hi\\\"", TOP).unwrap();
    let command = create_command(&name).unwrap();
    assert_eq!(command, "CREATE \"say \\\"hi\\\\\\\"\"");
    assert_eq!(served(&command), ["say \"hi\\\""]);
    let rename = rename_command("Old \"one\"", &name).unwrap();
    assert_eq!(served(&rename), ["Old \"one\"", "say \"hi\\\""]);
}

#[test]
fn a_name_up_to_the_byte_limit_goes_and_one_byte_more_is_refused_rfc8621_1_3_1() {
    let ascii = "a".repeat(MAX_MAILBOX_NAME_BYTES);
    assert!(MailboxName::new(&ascii, TOP).is_ok());
    let wide = "日".repeat(MAX_MAILBOX_NAME_BYTES / "日".len());
    assert!(MailboxName::new(&wide, TOP).is_ok());
    for long in [format!("{ascii}a"), format!("{wide}a")] {
        assert_eq!(MailboxName::new(&long, TOP), Err(NameError::TooLong));
    }
    let deep = "p".repeat(MAX_WIRE_NAME_BYTES - 2);
    assert!(MailboxName::new("x", under(&deep)).is_ok());
    assert_eq!(
        MailboxName::new("xy", under(&deep)),
        Err(NameError::TooLong)
    );
}

#[test]
fn a_name_that_cannot_name_a_mailbox_is_refused_before_anything_is_sent() {
    for (typed, refused) in [
        ("", NameError::Empty),
        ("a\r\nA2 DELETE INBOX", NameError::Control),
        ("a\0b", NameError::Control),
        ("tab\there", NameError::Control),
        ("a\u{7f}", NameError::Control),
        ("a\u{85}b", NameError::Control),
        ("Work/Sub", NameError::Delimiter),
        (" Work", NameError::EdgeSpace),
        ("Work ", NameError::EdgeSpace),
        ("\u{a0}Work", NameError::EdgeSpace),
        ("Work\u{3000}", NameError::EdgeSpace),
    ] {
        assert_eq!(MailboxName::new(typed, TOP), Err(refused), "{typed:?}");
    }
    let flat = Place {
        parent: Some("INBOX"),
        delimiter: None,
    };
    assert_eq!(MailboxName::new("Work", flat), Err(NameError::Flat));
    // The delimiter counts as the wire spells the name: `,` sits inside
    // the run that carries 台北.
    let comma = Place {
        parent: None,
        delimiter: Some(','),
    };
    assert_eq!(MailboxName::new("台北", comma), Err(NameError::Delimiter));
    assert!(MailboxName::new("Work.Sub", TOP).is_ok());
}

#[test]
fn a_typed_name_never_shows_in_a_debug_format() {
    let name = MailboxName::new("Geheim plan ü", under("Werk")).unwrap();
    let printed = format!("{name:?} {:?}", Ok::<_, NameError>(name.clone()));
    for word in ["Geheim", "plan", "Werk", "APw"] {
        assert!(!printed.contains(word), "{printed}");
    }
    assert!(printed.contains("leaf_bytes"), "{printed}");
}

#[test]
fn a_command_past_the_line_limit_is_never_sent_rfc7162_4() {
    let name = MailboxName::new("Elders", TOP).unwrap();
    // Every quote of the old name goes out escaped, so it takes two bytes.
    let around = "RENAME \"\" \"Elders\"".len();
    let fits = "\"".repeat((MAX_COMMAND_BYTES - around) / 2);
    assert_eq!(
        rename_command(&fits, &name).unwrap().len(),
        MAX_COMMAND_BYTES
    );
    let over = format!("{fits}\"");
    assert!(matches!(
        rename_command(&over, &name),
        Err(SessionError::Protocol(LINE_LIMIT))
    ));
}

#[test]
fn rights_keep_every_lowercase_letter_and_digit_once_in_order_rfc4314_7() {
    let read = |line: &[u8]| match Response::from_bytes(line).unwrap().1 {
        Response::MyRights(found) => Rights::of(&found.rights),
        other => panic!("{other:?}"),
    };
    let owner = read(b"* MYRIGHTS INBOX lrswipkxtecdan\r\n");
    assert_eq!(owner.letters(), "acdeiklnprstwx");
    assert!(owner.has('k') && owner.has('x'));
    let odd = read(b"* MYRIGHTS \"Locked\" \"sLl r9!s\"\r\n");
    assert_eq!(odd.letters(), "9lrs");
    assert!(!odd.has('L') && !odd.has('!') && !odd.has('k'));
    assert_eq!(read(b"* MYRIGHTS x \"\"\r\n"), Rights::default());
}

/// The characters a command line gives a meaning to, white space of
/// several kinds, a combining accent, wide characters and a character
/// outside the basic plane.
const PRINTABLE: [char; 27] = [
    'a', 'Z', '0', ' ', '"', '\\', '/', '.', ',', '&', '-', '+', '{', '}', '(', ')', '*', '%',
    '\u{a0}', '\u{3000}', 'e', '\u{301}', 'é', '台', '📬', 'b', 'c',
];

/// Line breaks and the other controls.
const CONTROLS: [char; 6] = ['\r', '\n', '\0', '\t', '\u{7f}', '\u{85}'];

/// Text the way a user might type it, most of it free of controls so
/// the names that go out carry the characters that matter on a line.
fn typed() -> impl Strategy<Value = String> {
    let printable = prop::sample::select(PRINTABLE.to_vec());
    let any = prop::sample::select([PRINTABLE.as_slice(), CONTROLS.as_slice()].concat());
    let text = |pool| prop::collection::vec(pool, 0..12).prop_map(String::from_iter);
    prop_oneof![
        4 => text(printable),
        2 => text(any),
        1 => "\\PC{0,8}",
        1 => "[a-z]{250,260}",
    ]
}

fn place() -> impl Strategy<Value = (Option<&'static str>, Option<char>)> {
    let parent = prop::option::of(prop::sample::select(vec![
        "INBOX",
        "Work",
        "&U,BTFw-",
        "say \"hi\\\"",
    ]));
    let delimiter = prop_oneof![
        4 => Just(Some('/')),
        4 => Just(Some('.')),
        1 => Just(None),
    ];
    (parent, delimiter)
}

proptest! {
    #[test]
    fn a_typed_name_reaches_the_server_whole_in_modified_utf7_or_is_refused_before_anything_is_sent(
        typed in typed(),
        (parent, delimiter) in place(),
    ) {
        let nfc: String = typed.nfc().collect();
        let name = match MailboxName::new(&typed, Place { parent, delimiter }) {
            Err(refused) => {
                let edge = nfc.starts_with(char::is_whitespace) || nfc.ends_with(char::is_whitespace);
                let holds = |found: char| utf7::encode(&nfc).contains(found);
                let fits = match refused {
                    NameError::Empty => nfc.is_empty(),
                    NameError::TooLong => nfc.len() > MAX_MAILBOX_NAME_BYTES,
                    NameError::Control => nfc.chars().any(char::is_control),
                    NameError::Delimiter => delimiter.is_some_and(holds),
                    NameError::EdgeSpace => edge,
                    NameError::Flat => parent.is_some() && delimiter.is_none(),
                };
                prop_assert!(fits, "{refused:?} for {typed:?}");
                return Ok(());
            }
            Ok(name) => name,
        };
        prop_assert_eq!(name.leaf(), nfc.as_str());
        prop_assert!(!nfc.is_empty() && nfc.len() <= MAX_MAILBOX_NAME_BYTES);
        prop_assert!(!nfc.chars().any(char::is_control));
        let command = create_command(&name).unwrap();
        prop_assert!(command.chars().all(|c| matches!(c, ' '..='~')), "{command:?}");
        let names = served(&command);
        prop_assert_eq!(names.as_slice(), [name.wire().to_owned()]);
        let expected = match (parent, delimiter) {
            (Some(parent), Some(delimiter)) => format!("{}{delimiter}{nfc}", utf7::decode(parent)),
            _ => nfc.clone(),
        };
        prop_assert_eq!(utf7::decode(&names[0]), expected);
        let renamed = served(&rename_command("Old", &name).unwrap());
        prop_assert_eq!(renamed, ["Old".to_owned(), name.wire().to_owned()]);
    }
}
