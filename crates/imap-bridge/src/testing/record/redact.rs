// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The redaction pass over a recording: every address, subject, message
//! id and Gmail id of the live session becomes a fixed value, the same
//! one wherever it recurs; a body text becomes filler of its length and
//! the greeting loses its words. A value that already has a fixed shape
//! stays, so a corpus written in these shapes reads back as written.

#[cfg(test)]
mod tests;

use std::collections::HashMap;

use super::fixed::{
    ADDRESS_FIELDS, FIXED_DOMAIN, Field, GREETING, ID_FIELDS, Piece, RECORDED_USER, SUBJECT_FIELD,
    SUBJECTS_FROM, Section, TOKEN, addresses, angle_ids, fill, fixed_message_id, fixed_subject,
    gmail_ids, greeting_head, is_fixed_address, is_fixed_subject, is_tagged, join, opaque_tokens,
    pieces, section,
};
use crate::testing::PASSWORD;
use crate::testing::command::{Command, Part};
use crate::testing::mailboxes::{quote, unquote};
use crate::testing::transcript::{Conversation, Exchange, Line, Segment, Transcript, split_tag};

/// The same greeting with its words replaced.
#[must_use]
pub fn greeting(line: &Line) -> Line {
    let (head, _) = greeting_head(line.opening());
    Line::text(&format!("{head} {GREETING}"))
}

/// A run of text with its size marker naming `size`; the `+` of a
/// literal that does not wait stays.
fn resized(run: &str, size: usize) -> String {
    let Some(open) = run.rfind('{') else {
        return run.to_owned();
    };
    let plus = if run.ends_with("+}") { "+" } else { "" };
    format!("{}{{{size}{plus}}}", &run[..open])
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
    /// user maps to the fixed one wherever it recurs. Any other command
    /// is redacted piece by piece, so the size marker of each literal
    /// names the length the redaction left it.
    fn client_line(&mut self, line: &str) -> String {
        let (tag, command) = split_tag(line);
        let Some(arguments) = command.strip_prefix("LOGIN ") else {
            return match Command::from_line(line) {
                Some(read) => self.command(&read),
                None => self.text(line),
            };
        };
        let user =
            unquote(arguments).map_or_else(|| split_tag(arguments).0.to_owned(), |(user, _)| user);
        self.addresses
            .entry(user)
            .or_insert_with(|| RECORDED_USER.to_owned());
        format!("{tag} LOGIN {} {}", quote(RECORDED_USER), quote(PASSWORD))
    }

    /// A command of runs of text and literals, every piece redacted and
    /// written back with its marker resized.
    fn command(&mut self, read: &Command) -> String {
        let mut out = String::new();
        let mut parts = read.parts.iter().peekable();
        while let Some(part) = parts.next() {
            let (Part::Text(run) | Part::Literal(run)) = part;
            let redacted = self.text(run);
            match parts.next_if(|next| matches!(next, Part::Literal(_))) {
                Some(Part::Literal(literal)) => {
                    let literal = self.text(literal);
                    out.push_str(&resized(&redacted, literal.len()));
                    out.push_str("\r\n");
                    out.push_str(&literal);
                }
                _ => out.push_str(&redacted),
            }
        }
        out
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
