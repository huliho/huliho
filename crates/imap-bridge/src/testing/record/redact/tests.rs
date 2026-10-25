// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What the redaction pass leaves of a header, a login, a body, a tagged
//! answer, a greeting and a command with literals.

use super::*;

fn fetch(header: &str) -> Line {
    Line(vec![
        Segment::Text(
            "* 1 FETCH (UID 1 X-GM-MSGID 1278455344230334865 X-GM-THRID 1266894439832287888 BODY[HEADER.FIELDS (FROM TO SUBJECT MESSAGE-ID REFERENCES)] "
                .to_owned(),
        ),
        Segment::Literal {
            literal: header.to_owned(),
        },
        Segment::Text(")".to_owned()),
    ])
}

fn literal_of(line: &Line) -> &str {
    match &line.0[1] {
        Segment::Literal { literal } => literal,
        other @ Segment::Text(_) => panic!("{other:?}"),
    }
}

#[test]
fn a_header_loses_its_names_subjects_and_ids_and_keeps_the_fixed_ones() {
    let header = "From: Eric <eric@gmail.com>\r\nTo: mo@example.test,\r\n\tSanne <sanne@example.test>\r\nSubject: =?UTF-8?Q?Caf=C3=A9?=\r\n over two lines\r\nMessage-ID: <abc.123@mail.gmail.com>\r\nReferences: <m1@example.test>\r\n <xyz@mail.gmail.com>\r\nDate: Mon, 1 Sep 2026 12:00:00 +0000\r\n\r\n";
    let mut redactor = Redactor::default();
    let line = redactor.server_line(&fetch(header));
    assert_eq!(
        literal_of(&line),
        "From: user1@example.test\r\nTo: mo@example.test,\r\n\tSanne <sanne@example.test>\r\nSubject: Message 1000\r\nMessage-ID: <1@example.test>\r\nReferences: <m1@example.test> <2@example.test>\r\nDate: Mon, 1 Sep 2026 12:00:00 +0000\r\n\r\n"
    );
    assert_eq!(
        line.opening(),
        "* 1 FETCH (UID 1 X-GM-MSGID 1 X-GM-THRID 1 BODY[HEADER.FIELDS (FROM TO SUBJECT MESSAGE-ID REFERENCES)] "
    );
    let again = redactor.server_line(&fetch(
        "From: eric@gmail.com\r\nSubject: Re: Message 1\r\n\r\n",
    ));
    assert_eq!(
        literal_of(&again),
        "From: user1@example.test\r\nSubject: Re: Message 1\r\n\r\n"
    );
    assert!(again.opening().contains("X-GM-MSGID 1 X-GM-THRID 1 "));
}

#[test]
fn a_login_names_the_fixed_user_and_the_live_one_maps_to_it_after() {
    let mut redactor = Redactor::default();
    assert_eq!(
        redactor.client_line("A0002 LOGIN \"eric.k@gmail.com\" \"app pass\\\"word\""),
        "A0002 LOGIN \"sanne@example.test\" \"correct horse\""
    );
    assert_eq!(
        redactor.text("A0002 OK eric.k@gmail.com authenticated (Success)"),
        "A0002 OK sanne@example.test authenticated (Success)"
    );
    assert_eq!(
        redactor.client_line("A0003 LIST \"\" \"*\""),
        "A0003 LIST \"\" \"*\""
    );
}

#[test]
fn a_literal_of_a_command_is_redacted_and_its_marker_names_its_new_length() {
    let mut redactor = Redactor::default();
    let line = "A0009 UID SEARCH CHARSET UTF-8 1:5 X-GM-RAW {26+}\r\nplan from:\"eric@gmail.com\" X-GM-LABELS {6}\r\n\\Inbox";
    let redacted = redactor.client_line(line);
    let raw = "plan from:\"user1@example.test\"";
    assert_eq!(
        redacted,
        format!(
            "A0009 UID SEARCH CHARSET UTF-8 1:5 X-GM-RAW {{{}+}}\r\n{raw} X-GM-LABELS {{6}}\r\n\\Inbox",
            raw.len()
        )
    );
    let read = Command::from_line(&redacted).unwrap();
    assert_eq!(read.line(), redacted);
    assert_eq!(read.waited(), 1);
    let kept = "A0010 UID SEARCH CHARSET UTF-8 1:5 TEXT {4+}\r\nzoë!";
    assert_eq!(redactor.client_line(kept), kept);
}

#[test]
fn a_body_becomes_filler_of_its_length_and_a_mime_header_stays() {
    let mut redactor = Redactor::default();
    let line = Line(vec![
        Segment::Text("* 2 FETCH (UID 4 BODY[1.MIME]<0> ".to_owned()),
        Segment::Literal {
            literal: "Content-Type: text/plain\r\n\r\n".to_owned(),
        },
        Segment::Text(" BODY[1]<0> ".to_owned()),
        Segment::Literal {
            literal: "Dear Eric, see you at 3.".to_owned(),
        },
        Segment::Text(")".to_owned()),
    ]);
    let redacted = redactor.server_line(&line);
    assert_eq!(redacted.0[1], line.0[1]);
    assert_eq!(
        redacted.0[3],
        Segment::Literal {
            literal: fill("Dear Eric, see you at 3.".len())
        }
    );
}

#[test]
fn a_tagged_answer_loses_its_token_and_untagged_data_keeps_its_long_words() {
    let mut redactor = Redactor::default();
    let tagged = Line::text("C1 OK Thats all she wrote! 5b1f17b1804b1-49fbbf104a9mb2431653045e9");
    assert_eq!(
        redactor.server_line(&tagged),
        Line::text("C1 OK Thats all she wrote! token")
    );
    let boundary = Line::text(
        "* 1 FETCH (BODYSTRUCTURE (\"TEXT\" \"PLAIN\" NIL NIL NIL \"7BIT\" 1 1) \"ALTERNATIVE\" (\"BOUNDARY\" \"000000000000949fc4065c17d0d8\"))",
    );
    assert_eq!(redactor.server_line(&boundary), boundary);
}

#[test]
fn the_greeting_keeps_its_status_and_code_and_loses_its_words() {
    let gmail = Line::text("* OK Gimap ready for requests from 203.0.113.9 k4mb12345678abc");
    assert_eq!(greeting(&gmail), Line::text("* OK ready"));
    let coded = Line::text("* OK [CAPABILITY IMAP4rev1 AUTH=PLAIN] Dovecot ready.");
    assert_eq!(
        greeting(&coded),
        Line::text("* OK [CAPABILITY IMAP4rev1 AUTH=PLAIN] ready")
    );
    assert_eq!(greeting(&Line::text("* BYE")), Line::text("* BYE ready"));
}
