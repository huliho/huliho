// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

use mail_parser::{HeaderName, MessageParser};
use proptest::prelude::*;

use super::*;

/// Tuesday 3 November 2026, 12:00 UTC.
const NOW: i64 = 1_793_707_200;

const DRAFT: &[u8] = b"From: Sanne <sanne@example.test>\r\n\
Bcc: hidden@example.test,\r\n  other@example.test\r\n\
To: mo@example.test\r\n\
Date: Mon, 2 Nov 2026 10:00:00 +0000\r\n\
Message-ID: <d1@example.test>\r\n\
Subject: Hoi\r\n\
\r\n\
Bcc: stays in the body\r\n\
Date: so does this\r\n";

#[test]
fn the_date_is_the_send_time_as_rfc5322_3_3_writes_it() {
    assert_eq!(Date::new(NOW).to_rfc822(), "Tue, 3 Nov 2026 12:00:00 +0000");
}

#[test]
fn the_wire_drops_every_bcc_field_with_its_folded_lines_and_nothing_else_rfc8621_7_5() {
    let forms = forms(DRAFT, NOW);
    let wire = String::from_utf8(forms.wire).unwrap();
    assert_eq!(
        wire,
        "From: Sanne <sanne@example.test>\r\n\
To: mo@example.test\r\n\
Date: Tue, 3 Nov 2026 12:00:00 +0000\r\n\
Message-ID: <d1@example.test>\r\n\
Subject: Hoi\r\n\
\r\n\
Bcc: stays in the body\r\n\
Date: so does this\r\n"
    );
}

#[test]
fn the_sent_copy_keeps_the_bcc_field_under_the_same_date() {
    let forms = forms(DRAFT, NOW);
    let sent = String::from_utf8(forms.sent).unwrap();
    assert_eq!(
        sent,
        "From: Sanne <sanne@example.test>\r\n\
Bcc: hidden@example.test,\r\n  other@example.test\r\n\
To: mo@example.test\r\n\
Date: Tue, 3 Nov 2026 12:00:00 +0000\r\n\
Message-ID: <d1@example.test>\r\n\
Subject: Hoi\r\n\
\r\n\
Bcc: stays in the body\r\n\
Date: so does this\r\n"
    );
}

#[test]
fn a_message_without_a_date_gets_one_at_the_top_and_two_become_one() {
    let undated = forms(b"Subject: x\r\n\r\nbody", NOW).wire;
    assert_eq!(
        undated,
        b"Date: Tue, 3 Nov 2026 12:00:00 +0000\r\nSubject: x\r\n\r\nbody"
    );
    let twice = forms(b"Date: a\r\nSubject: x\r\nDATE : b\r\n\r\n", NOW).wire;
    assert_eq!(
        twice,
        b"Date: Tue, 3 Nov 2026 12:00:00 +0000\r\nSubject: x\r\n\r\n"
    );
}

#[test]
fn a_field_name_is_read_without_regard_to_case_and_resent_bcc_leaves_too_rfc5322_3_6_6() {
    let draft = b"BCC: a@example.test\r\nbcc : b@example.test\r\nResent-Bcc: c@example.test\r\n\
X-Bcc-Note: kept\r\nSubject: x\r\n\r\n";
    let wire = String::from_utf8(forms(draft, NOW).wire).unwrap();
    assert!(!wire.contains("@example.test"), "{wire}");
    assert!(wire.contains("X-Bcc-Note: kept\r\n"), "{wire}");
}

/// How many Bcc and Resent-Bcc fields the recipient reader finds.
fn blind_fields(bytes: &[u8]) -> usize {
    MessageParser::default()
        .parse_headers(bytes)
        .map_or(0, |parsed| {
            let names = parsed.headers().iter().map(|header| &header.name);
            names
                .filter(|name| matches!(name, HeaderName::Bcc | HeaderName::ResentBcc))
                .count()
        })
}

#[test]
fn a_name_the_recipient_reader_finds_past_white_space_or_a_colon_leaves_the_wire_too() {
    let fields = [
        " Bcc: a@example.test\r\n",
        "B cc: b@example.test\r\n",
        ":Bcc: c@example.test\r\n",
        "Resent - Bcc: d@example.test\r\n",
        "Note\r\n Bcc: e@example.test\r\n",
    ];
    let draft = format!(
        "{}{}Subject: x\r\n{}{}{}Date: Mon, 2 Nov 2026 10:00:00 +0000\r\n\r\n",
        fields[0], fields[1], fields[2], fields[3], fields[4]
    );
    assert_eq!(blind_fields(draft.as_bytes()), fields.len());
    let forms = forms(draft.as_bytes(), NOW);
    assert_eq!(blind_fields(&forms.wire), 0);
    let wire = String::from_utf8(forms.wire).unwrap();
    assert!(!wire.contains("@example.test"), "{wire}");
    let sent = String::from_utf8(forms.sent).unwrap();
    for field in fields {
        assert!(sent.contains(field), "{sent}");
    }
}

/// A header of one line that folds under the Date stamp and ends in a
/// zone name cut by the line break: the recipient reader's date reader
/// takes the two bytes after the letter whatever they are, so it runs
/// past the empty line and reads the Bcc line of the body as a field.
const FOLDED_INTO_THE_BODY: &[u8] = b" \xff\xff\xff\x06\xb2+e(\n\n?\r\xf9 gC\x00\x11\x11\x11\x11\x13\x13\x11\x11\x13\x13\x13:\x00\x80\x01\x95\n \xcd\nBCC:C\x00;;\xe6\xf8\rte::\xff\xff\xff\ndate:\r<\n\x9a\n";

/// The least of that shape: the cut zone name, the empty line and a
/// name the reader takes at the end of the bytes.
const LEAST_FOLDED: &[u8] = b" +e\n\n\nBCC";

/// A Date the reader runs past the empty line, so the From, To and Bcc
/// lines under it read as fields of the draft and as body text of the
/// wire copy, where the stamp took the Date's place.
const DATED_PAST_THE_BODY: &[u8] = b"Date: Mon, 2 Nov 2026 10:00:00 +0000\r\n +e\n\n\n\
From: sanne@example.test\r\nTo: mo@example.test\r\nBcc: secret@example.test\r\n\r\nbody\r\n";

#[test]
fn a_wire_copy_the_recipient_reader_finds_a_bcc_in_is_refused_rfc8621_7_5() {
    assert_eq!(blind_fields(FOLDED_INTO_THE_BODY), 0);
    assert_eq!(blind_fields(&forms(FOLDED_INTO_THE_BODY, NOW).wire), 1);
    assert_eq!(
        sendable(FOLDED_INTO_THE_BODY, NOW),
        Err(Unsendable::InvalidEmail("headers"))
    );
    assert_eq!(names(LEAST_FOLDED), [] as [HeaderName<'_>; 0]);
    assert_eq!(
        names(&forms(LEAST_FOLDED, NOW).wire),
        [HeaderName::Date, HeaderName::Bcc]
    );
    assert_eq!(
        sendable(LEAST_FOLDED, NOW),
        Err(Unsendable::InvalidEmail("headers"))
    );
}

#[test]
fn a_date_cut_at_a_zone_name_reads_the_body_into_the_header_and_is_refused_rfc8621_7_5() {
    let draft = b"From: sanne@example.test\r\nTo: mo@example.test\r\nResent-Date: +e\r\n\r\n\
Bcc: kim@example.test\r\n";
    assert_eq!(blind_fields(draft), 1);
    let wire = forms(draft, NOW).wire;
    assert!(wire.ends_with(b"Resent-Date: +e\r\n\r\nBcc: kim@example.test\r\n"));
    assert_eq!(blind_fields(&wire), 1);
    assert_eq!(
        sendable(draft, NOW),
        Err(Unsendable::InvalidEmail("headers"))
    );
}

#[test]
fn the_fields_under_a_date_the_reader_runs_past_stay_body_text_and_are_refused_rfc8621_7_5() {
    use HeaderName::{Bcc, Date, From, To};
    assert_eq!(names(DATED_PAST_THE_BODY), [Date, From, To, Bcc]);
    let wire = forms(DATED_PAST_THE_BODY, NOW).wire;
    assert_eq!(names(&wire), [Date]);
    assert!(wire.ends_with(
        b"\n\nFrom: sanne@example.test\r\nTo: mo@example.test\r\n\
Bcc: secret@example.test\r\n\r\nbody\r\n"
    ));
    assert_eq!(
        sendable(DATED_PAST_THE_BODY, NOW),
        Err(Unsendable::InvalidEmail("headers"))
    );
}

#[test]
fn a_draft_with_bcc_fields_folded_or_not_a_bcc_body_line_and_any_number_of_dates_goes_out() {
    assert_eq!(sendable(DRAFT, NOW), Ok(forms(DRAFT, NOW)));
    let undated = b"From: sanne@example.test\r\nBcc: hidden@example.test\r\n\r\nBcc: body\r\n";
    assert_eq!(sendable(undated, NOW), Ok(forms(undated, NOW)));
    let twice = b"Date: a\r\nSubject: x\r\nDATE : b\r\n\r\n";
    assert_eq!(sendable(twice, NOW), Ok(forms(twice, NOW)));
}

#[test]
fn a_header_without_a_body_or_a_last_line_break_stays_well_formed() {
    let wire = forms(b"Subject: x\r\nBcc: a@example.test", NOW).wire;
    assert_eq!(
        wire,
        b"Date: Tue, 3 Nov 2026 12:00:00 +0000\r\nSubject: x\r\n"
    );
    let bare = forms(b"Subject: x", NOW).wire;
    assert!(bare.ends_with(b"Subject: x\r\n"));
}

/// A field name as a message may spell it: colons and white space before
/// it, white space inside it, in any case.
fn name() -> impl Strategy<Value = String> {
    let base = prop::sample::select(vec![
        "From",
        "To",
        "Cc",
        "Bcc",
        "Resent-Bcc",
        "Subject",
        "Date",
        "X-Note",
    ]);
    let gap = prop::option::of(("[ \t]{1,2}", any::<prop::sample::Index>()));
    (base, "[: \t]{0,3}", gap, any::<bool>(), any::<bool>()).prop_map(
        |(base, lead, gap, upper, lower)| {
            let mut name = match (upper, lower) {
                (true, _) => base.to_ascii_uppercase(),
                (false, true) => base.to_ascii_lowercase(),
                (false, false) => base.to_owned(),
            };
            if let Some((space, at)) = gap {
                name.insert_str(at.index(name.len() + 1), &space);
            }
            format!("{lead}{name}")
        },
    )
}

fn field() -> impl Strategy<Value = String> {
    let value = "[a-z@. ]{0,12}";
    let folds = prop::collection::vec("[a-z@.]{1,8}", 0..3);
    (name(), value, folds).prop_map(|(name, value, folds)| {
        let folded = folds.join("\r\n ");
        if folded.is_empty() {
            format!("{name}: {value}\r\n")
        } else {
            format!("{name}: {value}\r\n {folded}\r\n")
        }
    })
}

/// The fields as the header block holds them: one that opens with white
/// space continues the one before it.
fn joined(header: &[String]) -> Vec<String> {
    let mut fields: Vec<String> = Vec::new();
    for field in header {
        match fields.last_mut() {
            Some(last) if field.starts_with([' ', '\t']) => last.push_str(field),
            _ => fields.push(field.clone()),
        }
    }
    fields
}

/// The name of a field as the recipient reader reads it.
fn read_name(field: &str) -> String {
    let mut name = String::new();
    for byte in field.bytes() {
        match byte {
            b':' if name.is_empty() => {}
            b':' | b'\n' => break,
            _ if byte.is_ascii_whitespace() => {}
            _ => name.push(char::from(byte.to_ascii_lowercase())),
        }
    }
    name
}

fn blind_or_dated(field: &str) -> bool {
    ["bcc", "resent-bcc", "date"].contains(&read_name(field).as_str())
}

proptest! {
    #[test]
    fn every_bcc_field_leaves_the_wire_one_date_stands_and_the_rest_stays_rfc8621_7_5(
        header in prop::collection::vec(field(), 0..10),
        body in "(Bcc: [a-z@.]{1,8}\r\n|Date: x\r\n|[a-z ]{0,16}\r\n){0,6}",
    ) {
        let message = format!("{}\r\n{body}", header.concat());
        let forms = forms(message.as_bytes(), NOW);
        let parsed = MessageParser::default().parse_headers(&forms.wire).unwrap();
        prop_assert!(parsed.bcc().is_none());
        prop_assert_eq!(blind_fields(&forms.wire), 0);
        let names = |bytes: &[u8], wanted: HeaderName<'static>| {
            MessageParser::default()
                .parse_headers(bytes)
                .map_or(0, |parsed| parsed.headers().iter().filter(|h| h.name == wanted).count())
        };
        prop_assert_eq!(names(&forms.wire, HeaderName::Date), 1);
        prop_assert_eq!(names(&forms.sent, HeaderName::Date), 1);
        let tail = format!("\r\n{body}");
        prop_assert!(forms.wire.ends_with(tail.as_bytes()));
        prop_assert!(forms.sent.ends_with(tail.as_bytes()));
        let fields = joined(&header);
        let kept: String = fields.iter().filter(|field| !blind_or_dated(field)).cloned().collect();
        let wire = String::from_utf8(forms.wire.clone()).unwrap();
        let wire_fields: String = wire[..wire.len() - tail.len()]
            .split_inclusive("\r\n")
            .filter(|line| !line.starts_with("Date: Tue"))
            .collect();
        prop_assert_eq!(wire_fields, kept);
        let sent = String::from_utf8(forms.sent).unwrap();
        let blind: String = fields
            .iter()
            .filter(|field| blind_or_dated(field) && read_name(field) != "date")
            .cloned()
            .collect();
        for field in blind.split_inclusive("\r\n") {
            prop_assert!(sent.contains(field));
        }
    }
}
