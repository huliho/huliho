// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The redaction pass over a recording: every address, subject, message
//! id and Gmail id of the live session becomes a fixed value, the same
//! one wherever it recurs; a body text becomes filler of its length and
//! the greeting loses its words. A value that already has a fixed shape
//! stays, so a corpus written in these shapes reads back as written.

use std::collections::HashMap;

use super::fixed::{
    ADDRESS_FIELDS, FIXED_DOMAIN, Field, GREETING, ID_FIELDS, Piece, RECORDED_USER, SUBJECT_FIELD,
    SUBJECTS_FROM, Section, TOKEN, addresses, angle_ids, fill, fixed_message_id, fixed_subject,
    gmail_ids, greeting_head, is_fixed_address, is_fixed_subject, is_tagged, join, opaque_tokens,
    pieces, section,
};
use crate::testing::PASSWORD;
use crate::testing::mailboxes::{quote, unquote};
use crate::testing::transcript::{Conversation, Exchange, Line, Segment, Transcript, split_tag};

/// The same greeting with its words replaced.
#[must_use]
pub fn greeting(line: &Line) -> Line {
    let (head, _) = greeting_head(line.opening());
    Line::text(&format!("{head} {GREETING}"))
}

/// The same text with every opaque token replaced by the fixed word.
fn without_tokens(text: &str) -> String {
    let mut out = String::new();
    let mut last = 0;
    for (start, end) in opaque_tokens(text) {
        out.push_str(&text[last..start]);
        out.push_str(TOKEN);
        last = end;
    }
    out.push_str(&text[last..]);
    out
}

/// The maps of one pass, so a value recurs as the same fixed one.
#[derive(Default)]
pub struct Redactor {
    addresses: HashMap<String, String>,
    subjects: HashMap<String, String>,
    ids: HashMap<String, String>,
    msgids: HashMap<u64, u64>,
    thrids: HashMap<u64, u64>,
}

/// The transcript with every personal value replaced.
#[must_use]
pub fn redact(recorded: &Transcript) -> Transcript {
    let mut redactor = Redactor::default();
    Transcript {
        sessions: recorded
            .sessions
            .iter()
            .map(|conversation| redactor.conversation(conversation))
            .collect(),
    }
}

impl Redactor {
    fn conversation(&mut self, conversation: &Conversation) -> Conversation {
        Conversation {
            exchanges: conversation
                .exchanges
                .iter()
                .enumerate()
                .map(|(index, exchange)| self.exchange(index == 0, exchange))
                .collect(),
        }
    }

    fn exchange(&mut self, first: bool, exchange: &Exchange) -> Exchange {
        Exchange {
            client: exchange
                .client
                .as_deref()
                .map(|line| self.client_line(line)),
            server: exchange
                .server
                .iter()
                .enumerate()
                .map(|(index, line)| {
                    if first && index == 0 {
                        greeting(line)
                    } else {
                        self.server_line(line)
                    }
                })
                .collect(),
        }
    }

    /// A LOGIN names the fixed user and the fixture password; the live
    /// user maps to the fixed one wherever it recurs.
    fn client_line(&mut self, line: &str) -> String {
        let (tag, command) = split_tag(line);
        let Some(arguments) = command.strip_prefix("LOGIN ") else {
            return self.text(line);
        };
        let user =
            unquote(arguments).map_or_else(|| split_tag(arguments).0.to_owned(), |(user, _)| user);
        self.addresses
            .entry(user)
            .or_insert_with(|| RECORDED_USER.to_owned());
        format!("{tag} LOGIN {} {}", quote(RECORDED_USER), quote(PASSWORD))
    }

    /// A tagged answer loses every opaque token as well, since a server
    /// names its connection there.
    fn server_line(&mut self, line: &Line) -> Line {
        let tagged = is_tagged(line.opening());
        let mut before = String::new();
        let segments = line
            .0
            .iter()
            .map(|segment| match segment {
                Segment::Text(text) => {
                    before.clone_from(text);
                    let redacted = self.text(text);
                    Segment::Text(if tagged {
                        without_tokens(&redacted)
                    } else {
                        redacted
                    })
                }
                Segment::Literal { literal } => Segment::Literal {
                    literal: self.literal(&before, literal),
                },
            })
            .collect();
        Line(segments)
    }

    fn literal(&mut self, before: &str, literal: &str) -> String {
        match section(before) {
            Section::Header => {
                let rewritten = self.header(literal);
                self.text(&rewritten)
            }
            Section::Body => fill(literal.len()),
            Section::Other => self.text(literal),
        }
    }

    fn header(&mut self, block: &str) -> String {
        let pieces: Vec<Piece> = pieces(block)
            .into_iter()
            .map(|piece| match piece {
                Piece::Field(field) => Piece::Field(self.field(field)),
                other @ Piece::Other(_) => other,
            })
            .collect();
        join(&pieces)
    }

    fn field(&mut self, field: Field) -> Field {
        let name = field.name.as_str();
        let value = if ADDRESS_FIELDS.contains(&name) {
            self.address_field(&field.value)
        } else if name == SUBJECT_FIELD {
            self.subject(&field.value)
        } else if ID_FIELDS.contains(&name) {
            self.id_field(&field.value)
        } else {
            None
        };
        match value {
            Some(value) => Field {
                raw: format!("{}: {value}", field.label),
                ..field
            },
            None => field,
        }
    }

    /// The addresses of the field, names dropped; `None` when every one
    /// is fixed or none is found.
    fn address_field(&mut self, value: &str) -> Option<String> {
        let found = addresses(value);
        if found.is_empty()
            || found
                .iter()
                .all(|(start, end)| is_fixed_address(&value[*start..*end]))
        {
            return None;
        }
        let mapped: Vec<String> = found
            .iter()
            .map(|(start, end)| self.address(&value[*start..*end]))
            .collect();
        Some(mapped.join(", "))
    }

    fn subject(&mut self, value: &str) -> Option<String> {
        if is_fixed_subject(value) {
            return None;
        }
        let next = SUBJECTS_FROM + self.subjects.len();
        Some(
            self.subjects
                .entry(value.to_owned())
                .or_insert_with(|| fixed_subject(next))
                .clone(),
        )
    }

    fn id_field(&mut self, value: &str) -> Option<String> {
        let spans = angle_ids(value);
        if spans
            .iter()
            .all(|(start, end)| is_fixed_address(&value[*start..*end]))
        {
            return None;
        }
        let mut out = String::new();
        let mut last = 0;
        for (start, end) in spans {
            out.push_str(&value[last..start]);
            let inside = &value[start..end];
            if is_fixed_address(inside) {
                out.push_str(inside);
            } else {
                let next = self.ids.len() + 1;
                let fixed = self
                    .ids
                    .entry(inside.to_owned())
                    .or_insert_with(|| fixed_message_id(next));
                out.push_str(&fixed[1..fixed.len() - 1]);
            }
            last = end;
        }
        out.push_str(&value[last..]);
        Some(out)
    }

    fn address(&mut self, token: &str) -> String {
        let next = self.addresses.len() + 1;
        self.addresses
            .entry(token.to_owned())
            .or_insert_with(|| format!("user{next}@{FIXED_DOMAIN}"))
            .clone()
    }

    /// The pass every run of text gets: the Gmail ids renumbered, then
    /// every address outside the fixed domain mapped.
    fn text(&mut self, text: &str) -> String {
        let renumbered = self.renumber(text);
        let mut out = String::new();
        let mut last = 0;
        for (start, end) in addresses(&renumbered) {
            out.push_str(&renumbered[last..start]);
            let token = &renumbered[start..end];
            if is_fixed_address(token) {
                out.push_str(token);
            } else {
                out.push_str(&self.address(token));
            }
            last = end;
        }
        out.push_str(&renumbered[last..]);
        out
    }

    fn renumber(&mut self, text: &str) -> String {
        let mut out = String::new();
        let mut last = 0;
        for (item, start, end) in gmail_ids(text) {
            out.push_str(&text[last..start]);
            let value: u64 = text[start..end].parse().unwrap_or(u64::MAX);
            let map = if item.starts_with("X-GM-MSGID") {
                &mut self.msgids
            } else {
                &mut self.thrids
            };
            let next = u64::try_from(map.len())
                .unwrap_or(u64::MAX)
                .saturating_add(1);
            let fixed = *map.entry(value).or_insert(next);
            out.push_str(&fixed.to_string());
            last = end;
        }
        out.push_str(&text[last..]);
        out
    }
}

#[cfg(test)]
mod tests {
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
        let tagged =
            Line::text("C1 OK Thats all she wrote! 5b1f17b1804b1-49fbbf104a9mb2431653045e9");
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
}
