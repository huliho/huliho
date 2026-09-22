// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A recorded IMAP conversation: every connection, every command and
//! what the server said back. A literal is a segment of its own, so a
//! line can be rewritten and sent again with its size markers right.

use std::sync::{Mutex, PoisonError};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt};

/// The bytes one line of text or one literal may take on the way in,
/// the bound the guard puts on a whole response; the recorder sits in
/// front of the guard.
pub const MAX_LITERAL_BYTES: usize = 1024 * 1024;

/// What a byte outside UTF-8 becomes: one ASCII byte, so a literal keeps
/// the length the server sent.
const UNREADABLE_BYTE: char = '?';

/// Every connection of one recording, in the order they were accepted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transcript {
    pub sessions: Vec<Conversation>,
}

/// One connection: the greeting and everything after it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conversation {
    pub exchanges: Vec<Exchange>,
}

/// One command and every line the server sent before the next one.
/// The first exchange of a connection holds the greeting and no
/// command.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exchange {
    pub client: Option<String>,
    pub server: Vec<Line>,
}

/// One line the server sent, its literals kept apart; the size marker
/// in front of a literal and the line ending are implied.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Line(pub Vec<Segment>);

/// A run of text or one literal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Segment {
    Text(String),
    Literal { literal: String },
}

/// The recording as the recorder fills it, shared by every connection.
pub type Shared = std::sync::Arc<Mutex<Transcript>>;

impl Transcript {
    /// Opens a connection's record; the index names it from then on.
    pub fn start_session(&mut self) -> usize {
        self.sessions.push(Conversation::default());
        self.sessions.len() - 1
    }

    /// Notes a command the client sent on that connection.
    pub fn client(&mut self, session: usize, line: String) {
        let exchange = Exchange {
            client: Some(line),
            server: Vec::new(),
        };
        if let Some(conversation) = self.sessions.get_mut(session) {
            conversation.exchanges.push(exchange);
        }
    }

    /// Notes a line the server sent on that connection, under the last
    /// command or as the greeting.
    pub fn server(&mut self, session: usize, line: Line) {
        let Some(conversation) = self.sessions.get_mut(session) else {
            return;
        };
        if conversation.exchanges.is_empty() {
            conversation.exchanges.push(Exchange::default());
        }
        if let Some(exchange) = conversation.exchanges.last_mut() {
            exchange.server.push(line);
        }
    }

    /// The transcript as indented JSON with a final line break.
    ///
    /// # Panics
    ///
    /// Never: every field serializes.
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut json = serde_json::to_string_pretty(self).expect("a transcript serializes");
        json.push('\n');
        json
    }

    /// A transcript read back from its JSON.
    ///
    /// # Errors
    ///
    /// Returns the parse failure.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// A copy of a shared recording as it stands.
    #[must_use]
    pub fn snapshot(shared: &Shared) -> Self {
        shared
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Exchange {
    /// The command without its tag; `None` for the greeting.
    #[must_use]
    pub fn command(&self) -> Option<&str> {
        self.client.as_deref().map(|line| split_tag(line).1)
    }
}

impl Line {
    /// A line of one run of text.
    #[must_use]
    pub fn text(text: &str) -> Self {
        Self(vec![Segment::Text(text.to_owned())])
    }

    /// The bytes on the wire: each literal behind its size marker, the
    /// line ending at the end.
    #[must_use]
    pub fn bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        for segment in &self.0 {
            match segment {
                Segment::Text(text) => bytes.extend_from_slice(text.as_bytes()),
                Segment::Literal { literal } => {
                    bytes.extend_from_slice(format!("{{{}}}\r\n", literal.len()).as_bytes());
                    bytes.extend_from_slice(literal.as_bytes());
                }
            }
        }
        bytes.extend_from_slice(b"\r\n");
        bytes
    }

    /// The first run of text, where a tag would stand.
    #[must_use]
    pub fn opening(&self) -> &str {
        match self.0.first() {
            Some(Segment::Text(text)) => text,
            _ => "",
        }
    }

    /// The same line under another tag when it carries `from`; an
    /// untagged line stays as it is.
    #[must_use]
    pub fn retagged(&self, from: &str, to: &str) -> Self {
        let mut segments = self.0.clone();
        if let Some(Segment::Text(text)) = segments.first_mut()
            && let Some(rest) = text.strip_prefix(from)
            && rest.starts_with(' ')
        {
            *text = format!("{to}{rest}");
        }
        Self(segments)
    }
}

/// The tag and the rest of a command line; a line without a space is
/// all tag.
#[must_use]
pub fn split_tag(line: &str) -> (&str, &str) {
    line.split_once(' ').unwrap_or((line, ""))
}

/// One line as the server sent it, its literals read whole; `None` when
/// the server left or a run of text or a literal passes
/// `MAX_LITERAL_BYTES`.
pub async fn read_line<R: AsyncBufRead + Unpin>(reader: &mut R) -> Option<Line> {
    let bounded = u64::try_from(MAX_LITERAL_BYTES)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    let mut segments = Vec::new();
    loop {
        let mut buffer = Vec::new();
        let read = (&mut *reader)
            .take(bounded)
            .read_until(b'\n', &mut buffer)
            .await
            .ok()?;
        if read == 0 || buffer.len() > MAX_LITERAL_BYTES {
            return None;
        }
        while buffer
            .last()
            .is_some_and(|byte| matches!(byte, b'\r' | b'\n'))
        {
            buffer.pop();
        }
        let Some((marker, size)) = literal_size(&buffer) else {
            if !buffer.is_empty() || segments.is_empty() {
                segments.push(Segment::Text(lossy(&buffer)));
            }
            return Some(Line(segments));
        };
        if size > MAX_LITERAL_BYTES {
            return None;
        }
        segments.push(Segment::Text(lossy(&buffer[..marker])));
        let mut literal = vec![0; size];
        reader.read_exact(&mut literal).await.ok()?;
        segments.push(Segment::Literal {
            literal: lossy(&literal),
        });
    }
}

/// Where a size marker closes the line and the size it names (RFC 3501
/// section 4.3).
fn literal_size(line: &[u8]) -> Option<(usize, usize)> {
    let inner = line.strip_suffix(b"}")?;
    let open = inner.iter().rposition(|byte| *byte == b'{')?;
    let digits = &inner[open + 1..];
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let size = std::str::from_utf8(digits).ok()?.parse().ok()?;
    Some((open, size))
}

/// Bytes as text, each byte outside UTF-8 as `UNREADABLE_BYTE`.
fn lossy(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len());
    for chunk in bytes.utf8_chunks() {
        text.push_str(chunk.valid());
        text.extend(std::iter::repeat_n(UNREADABLE_BYTE, chunk.invalid().len()));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_line_with_literals_reads_into_segments_and_writes_back_with_its_sizes() {
        let wire = b"* 1 FETCH (UID 7 BODY[1.MIME]<0> {4}\r\nA: b BODY[1]<0> {2}\r\nhi)\r\n";
        let mut reader = tokio::io::BufReader::new(&wire[..]);
        let line = read_line(&mut reader).await.unwrap();
        assert_eq!(
            line.0,
            [
                Segment::Text("* 1 FETCH (UID 7 BODY[1.MIME]<0> ".to_owned()),
                Segment::Literal {
                    literal: "A: b".to_owned()
                },
                Segment::Text(" BODY[1]<0> ".to_owned()),
                Segment::Literal {
                    literal: "hi".to_owned()
                },
                Segment::Text(")".to_owned()),
            ]
        );
        assert_eq!(line.bytes(), wire);
        assert!(read_line(&mut reader).await.is_none());
    }

    #[tokio::test]
    async fn a_plain_line_and_a_literal_that_ends_the_line_read_whole() {
        let wire = b"* OK ready\r\n* LIST () \"/\" {4}\r\nI\r\nX\r\n";
        let mut reader = tokio::io::BufReader::new(&wire[..]);
        assert_eq!(
            read_line(&mut reader).await.unwrap(),
            Line::text("* OK ready")
        );
        let listed = read_line(&mut reader).await.unwrap();
        assert_eq!(
            listed.0,
            [
                Segment::Text("* LIST () \"/\" ".to_owned()),
                Segment::Literal {
                    literal: "I\r\nX".to_owned()
                },
            ]
        );
        assert_eq!(listed.bytes(), b"* LIST () \"/\" {4}\r\nI\r\nX\r\n");
    }

    #[tokio::test]
    async fn a_size_past_the_bound_or_bytes_outside_utf8_never_break_the_read() {
        let too_big = format!("* X {{{}}}\r\n", MAX_LITERAL_BYTES + 1);
        let mut reader = tokio::io::BufReader::new(too_big.as_bytes());
        assert!(read_line(&mut reader).await.is_none());
        let too_long = format!("* {}\r\n", "x".repeat(MAX_LITERAL_BYTES));
        let mut reader = tokio::io::BufReader::new(too_long.as_bytes());
        assert!(read_line(&mut reader).await.is_none());
        let at_the_bound = format!("* {}\r\n", "x".repeat(MAX_LITERAL_BYTES - "* \r\n".len()));
        assert_eq!(at_the_bound.len(), MAX_LITERAL_BYTES);
        let mut reader = tokio::io::BufReader::new(at_the_bound.as_bytes());
        assert!(read_line(&mut reader).await.is_some());
        let wire = b"* OK caf\xe9 {2}\r\n\xe9\xe9\r\n";
        let mut reader = tokio::io::BufReader::new(&wire[..]);
        let line = read_line(&mut reader).await.unwrap();
        assert_eq!(
            line.0,
            [
                Segment::Text("* OK caf? ".to_owned()),
                Segment::Literal {
                    literal: "??".to_owned()
                },
            ]
        );
        assert_eq!(line.bytes().len(), wire.len());
    }

    #[test]
    fn a_tagged_line_is_retagged_and_an_untagged_one_is_not() {
        let done = Line::text("A0003 OK done");
        assert_eq!(done.retagged("A0003", "A0009"), Line::text("A0009 OK done"));
        assert_eq!(
            Line::text("* OK A0003 is fine").retagged("A0003", "A9"),
            Line::text("* OK A0003 is fine")
        );
        assert_eq!(
            Line::text("A00031 OK").retagged("A0003", "A9"),
            Line::text("A00031 OK")
        );
        assert_eq!(
            split_tag("A0001 LIST \"\" \"*\""),
            ("A0001", "LIST \"\" \"*\"")
        );
        assert_eq!(split_tag("DONE"), ("DONE", ""));
    }

    #[test]
    fn a_transcript_round_trips_through_json_with_the_greeting_apart() {
        let mut transcript = Transcript::default();
        let session = transcript.start_session();
        transcript.server(session, Line::text("* OK ready"));
        transcript.client(session, "A1 CAPABILITY".to_owned());
        transcript.server(session, Line::text("* CAPABILITY IMAP4rev1"));
        transcript.server(session, Line::text("A1 OK done"));
        let exchanges = &transcript.sessions[0].exchanges;
        assert_eq!(exchanges.len(), 2);
        assert_eq!(exchanges[0].command(), None);
        assert_eq!(exchanges[1].command(), Some("CAPABILITY"));
        let json = transcript.to_json();
        assert!(json.ends_with('\n'));
        assert_eq!(Transcript::from_json(&json).unwrap(), transcript);
    }
}
