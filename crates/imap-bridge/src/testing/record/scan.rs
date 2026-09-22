// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The scan of a transcript: every value that could name a person, the
//! account or its Gmail ids must have the shape the redaction leaves.
//! Anything else is a finding, and a transcript with one is not checked
//! in. A LOGIN line is judged whole and never quoted; an AUTHENTICATE
//! line is a finding whatever follows it, since no fixed shape exists
//! for one.

use super::fixed::{
    GREETING, ID_FIELDS, MAX_RENUMBERED_ID, Piece, RECORDED_USER, SUBJECT_FIELD, Section,
    addresses, angle_ids, fill, gmail_ids, greeting_head, is_fixed_address, is_fixed_subject,
    is_tagged, opaque_tokens, pieces, section,
};
use crate::testing::PASSWORD;
use crate::testing::mailboxes::quote;
use crate::testing::transcript::{Line, Segment, Transcript, split_tag};

/// The octets of a dotted address (RFC 791).
const IPV4_PARTS: usize = 4;
const IPV4_MAX_OCTET: u32 = 255;

/// Where a finding sits.
struct Place {
    session: usize,
    exchange: usize,
}

/// Every place where a live value remains; empty for a clean transcript.
#[must_use]
pub fn scan(transcript: &Transcript) -> Vec<String> {
    let mut findings = Vec::new();
    for (session, conversation) in transcript.sessions.iter().enumerate() {
        for (exchange, recorded) in conversation.exchanges.iter().enumerate() {
            let place = Place { session, exchange };
            if let Some(line) = recorded.client.as_deref() {
                client_line(&place, line, &mut findings);
            }
            for (index, line) in recorded.server.iter().enumerate() {
                if exchange == 0 && index == 0 {
                    greeting(&place, line, &mut findings);
                } else {
                    server_line(&place, line, &mut findings);
                }
            }
        }
    }
    findings
}

fn note(place: &Place, findings: &mut Vec<String>, what: &str) {
    findings.push(format!(
        "session {} exchange {}: {what}",
        place.session, place.exchange
    ));
}

fn client_line(place: &Place, line: &str, findings: &mut Vec<String>) {
    let (tag, command) = split_tag(line);
    if command.starts_with("AUTHENTICATE") {
        note(place, findings, "an AUTHENTICATE line has no fixed shape");
        return;
    }
    if command.starts_with("LOGIN ") {
        let fixed = format!("{tag} LOGIN {} {}", quote(RECORDED_USER), quote(PASSWORD));
        if line != fixed {
            note(place, findings, "the LOGIN line is not the fixed one");
        }
        return;
    }
    text(place, line, findings);
}

fn greeting(place: &Place, line: &Line, findings: &mut Vec<String>) {
    let (_, words) = greeting_head(line.opening());
    if words != GREETING {
        note(place, findings, "the greeting keeps its words");
    }
}

fn server_line(place: &Place, line: &Line, findings: &mut Vec<String>) {
    let tagged = is_tagged(line.opening());
    let mut before = "";
    for segment in &line.0 {
        match segment {
            Segment::Text(run) => {
                before = run;
                text(place, run, findings);
                if tagged {
                    for (start, end) in opaque_tokens(run) {
                        let token = &run[start..end];
                        note(
                            place,
                            findings,
                            &format!("an opaque token remains on a tagged line ({token})"),
                        );
                    }
                }
            }
            Segment::Literal { literal } => match section(before) {
                Section::Header => {
                    header(place, literal, findings);
                    text(place, literal, findings);
                }
                Section::Body => {
                    if *literal != fill(literal.len()) {
                        note(place, findings, "a body text is not the filler");
                    }
                }
                Section::Other => text(place, literal, findings),
            },
        }
    }
}

fn header(place: &Place, block: &str, findings: &mut Vec<String>) {
    for piece in pieces(block) {
        let Piece::Field(field) = piece else {
            continue;
        };
        let name = field.name.as_str();
        if name == SUBJECT_FIELD && !is_fixed_subject(&field.value) {
            note(
                place,
                findings,
                &format!("a subject remains: {}", field.value),
            );
        }
        if ID_FIELDS.contains(&name) {
            for (start, end) in angle_ids(&field.value) {
                let inside = &field.value[start..end];
                if !is_fixed_address(inside) {
                    note(
                        place,
                        findings,
                        &format!("a message id remains: <{inside}>"),
                    );
                }
            }
        }
    }
}

/// The checks every run of text gets.
fn text(place: &Place, run: &str, findings: &mut Vec<String>) {
    for (start, end) in addresses(run) {
        let token = &run[start..end];
        if !is_fixed_address(token) {
            note(place, findings, &format!("an address remains: {token}"));
        }
    }
    for (item, start, end) in gmail_ids(run) {
        let value: u64 = run[start..end].parse().unwrap_or(u64::MAX);
        if value > MAX_RENUMBERED_ID {
            note(place, findings, &format!("{item}{value} is not renumbered"));
        }
    }
    for word in run.split_whitespace() {
        let word = word.trim_matches(|c: char| !c.is_ascii_digit());
        if is_ipv4(word) {
            note(
                place,
                findings,
                &format!("an address of a host remains: {word}"),
            );
        }
    }
}

fn is_ipv4(word: &str) -> bool {
    let parts: Vec<&str> = word.split('.').collect();
    parts.len() == IPV4_PARTS
        && parts.iter().all(|part| {
            !part.is_empty()
                && part
                    .parse::<u32>()
                    .is_ok_and(|octet| octet <= IPV4_MAX_OCTET)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::transcript::{Conversation, Exchange};

    fn transcript(client: &str, server: Vec<Line>) -> Transcript {
        Transcript {
            sessions: vec![Conversation {
                exchanges: vec![
                    Exchange {
                        client: None,
                        server: vec![Line::text("* OK ready")],
                    },
                    Exchange {
                        client: Some(client.to_owned()),
                        server,
                    },
                ],
            }],
        }
    }

    fn fetch(header: &str) -> Line {
        Line(vec![
            Segment::Text("* 1 FETCH (UID 1 X-GM-MSGID 3 BODY[HEADER.FIELDS (FROM)] ".to_owned()),
            Segment::Literal {
                literal: header.to_owned(),
            },
            Segment::Text(")".to_owned()),
        ])
    }

    #[test]
    fn a_clean_transcript_has_no_findings() {
        let login = format!("A1 LOGIN {} {}", quote(RECORDED_USER), quote(PASSWORD));
        let clean = transcript(
            &login,
            vec![
                fetch(
                    "From: sanne@example.test\r\nSubject: Message 1\r\nMessage-ID: <m1@example.test>\r\nIn-Reply-To: <3@example.test>\r\n\r\n",
                ),
                Line::text("A1 OK sanne@example.test authenticated (Success)"),
            ],
        );
        assert_eq!(scan(&clean), Vec::<String>::new());
    }

    #[test]
    fn every_kind_of_leak_is_a_finding_of_its_own() {
        let leaky = transcript(
            "A1 LOGIN \"eric@gmail.com\" \"correct horse\"",
            vec![
                fetch(
                    "From: eric@gmail.com\r\nSubject: Lunch\r\nMessage-ID: <abc@mail.gmail.com>\r\n\r\n",
                ),
                Line::text("* 2 FETCH (UID 2 X-GM-THRID 1266894439832287888)"),
                Line(vec![
                    Segment::Text("* 3 FETCH (UID 3 BODY[TEXT]<0> ".to_owned()),
                    Segment::Literal {
                        literal: "Dear Eric".to_owned(),
                    },
                    Segment::Text(")".to_owned()),
                ]),
                Line::text("A1 OK from 203.0.113.9"),
                Line::text("A1 OK Thats all she wrote! 5b1f17b1804b1-49fbbf104a9mb2431653045e9"),
            ],
        );
        let findings = scan(&leaky);
        let kinds: Vec<&str> = findings
            .iter()
            .map(|finding| finding.split_once(": ").unwrap().1)
            .collect();
        assert_eq!(
            kinds,
            [
                "the LOGIN line is not the fixed one",
                "a subject remains: Lunch",
                "a message id remains: <abc@mail.gmail.com>",
                "an address remains: eric@gmail.com",
                "an address remains: abc@mail.gmail.com",
                "X-GM-THRID 1266894439832287888 is not renumbered",
                "a body text is not the filler",
                "an address of a host remains: 203.0.113.9",
                "an opaque token remains on a tagged line (5b1f17b1804b1-49fbbf104a9mb2431653045e9)",
            ]
        );
        assert!(
            !findings
                .iter()
                .any(|finding| finding.contains("correct horse"))
        );
    }

    #[test]
    fn an_authenticate_line_is_a_finding_whatever_follows_it() {
        let oauth = transcript("A1 AUTHENTICATE XOAUTH2", vec![Line::text("A1 OK done")]);
        assert_eq!(
            scan(&oauth),
            ["session 0 exchange 1: an AUTHENTICATE line has no fixed shape"]
        );
    }

    #[test]
    fn a_greeting_with_its_words_is_a_finding() {
        let mut talkative = transcript("A1 NOOP", vec![Line::text("A1 OK done")]);
        talkative.sessions[0].exchanges[0].server[0] =
            Line::text("* OK Gimap ready for requests from 203.0.113.9 k4mb");
        let findings = scan(&talkative);
        assert_eq!(
            findings[0],
            "session 0 exchange 0: the greeting keeps its words"
        );
    }
}
