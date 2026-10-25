// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A client command as the scripted IMAP server reads it: the line, and
//! each literal by its byte count, a synchronizing one after the
//! server's continuation request (RFC 3501 section 7.5), a
//! non-synchronizing one where the script reads it so (RFC 7888).

use tokio::io::{AsyncBufRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use super::transcript::{MAX_LITERAL_BYTES, bounded_line};
use crate::session::Literals;

/// What the server asks a literal with.
const CONTINUATION: &[u8] = b"+ Ready for literal\r\n";

/// What a literal that waits for the continuation request gets.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Continuation {
    /// The request, then the literal is read.
    #[default]
    Asks,
    /// Nothing ever; the connection holds until the client gives up.
    Withholds,
    /// A tagged NO in its place, which ends the command (RFC 3501
    /// section 2.2.1).
    Refuses,
}

/// How the server reads the literals of a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reading {
    /// What it reads without a continuation request.
    pub literals: Literals,
    pub continuation: Continuation,
}

/// One command: runs of text and the literals between them, in order.
/// A run of text that a literal follows ends with its size marker.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Command {
    pub parts: Vec<Part>,
}

/// A run of text or one literal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part {
    Text(String),
    Literal(String),
}

impl Command {
    /// The command as the wire carried it, its last line end left out.
    #[must_use]
    pub fn line(&self) -> String {
        let mut line = String::new();
        for part in &self.parts {
            match part {
                Part::Text(text) => line.push_str(text),
                Part::Literal(literal) => {
                    line.push_str("\r\n");
                    line.push_str(literal);
                }
            }
        }
        line
    }

    /// The literals of the command that waited for a continuation
    /// request.
    #[must_use]
    pub fn waited(&self) -> usize {
        let waits = |part: &&Part| match part {
            Part::Text(text) => marker(text).is_some_and(|(_, synchronizing)| synchronizing),
            Part::Literal(_) => false,
        };
        self.parts.iter().filter(waits).count()
    }

    /// The command [`Command::line`] wrote, read back: a line break
    /// follows only a run of text that ends with a size marker, and the
    /// literal after it takes the bytes the marker names. `None` for a
    /// line break elsewhere or a literal cut short.
    #[must_use]
    pub fn from_line(line: &str) -> Option<Self> {
        let mut command = Self::default();
        let mut rest = line;
        loop {
            let Some((text, after)) = rest.split_once("\r\n") else {
                command.parts.push(Part::Text(rest.to_owned()));
                return Some(command);
            };
            let (size, _) = marker(text)?;
            let literal = after.get(..size)?;
            command.parts.push(Part::Text(text.to_owned()));
            command.parts.push(Part::Literal(literal.to_owned()));
            rest = &after[size..];
        }
    }
}

/// One run of text of a command off the wire, its line break left out,
/// with the size its marker names and whether that literal waits for the
/// continuation request. `None` when the client left, sent bytes outside
/// UTF-8 or a line past `MAX_LITERAL_BYTES`.
pub async fn text_run<R: AsyncBufRead + Unpin>(
    reader: &mut R,
) -> Option<(String, Option<(usize, bool)>)> {
    let text = String::from_utf8(bounded_line(reader).await?).ok()?;
    let marker = marker(&text);
    Some((text, marker))
}

/// The literal a marker announced, read by its size; `None` past
/// `MAX_LITERAL_BYTES`, for bytes outside UTF-8 or when the client left.
pub async fn literal<R: AsyncBufRead + Unpin>(reader: &mut R, size: usize) -> Option<String> {
    if size > MAX_LITERAL_BYTES {
        return None;
    }
    let mut literal = vec![0; size];
    reader.read_exact(&mut literal).await.ok()?;
    String::from_utf8(literal).ok()
}

/// How a command was settled before the server could take it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Settled {
    /// Its continuation request is withheld.
    Held,
    /// Its literal was refused with a NO.
    Refused,
    /// A non-synchronizing literal came that the script does not read
    /// so; it was read whole (RFC 7888 section 4).
    Unread,
}

/// Reads the next command for the script to answer, every command handed
/// to `seen` as it arrives. A command settled before the script could
/// take it is answered here: a NO for a literal refused, a BAD for a
/// non-synchronizing literal the script does not read so. One whose
/// continuation request is withheld holds the connection. `None` when
/// the client left or sent bytes outside UTF-8, or when a run of text or
/// a literal passes `MAX_LITERAL_BYTES`.
pub async fn read<R, W>(
    reader: &mut R,
    writer: &mut W,
    reading: Reading,
    mut seen: impl FnMut(&str),
) -> Option<Command>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    loop {
        let (command, settled) = read_one(reader, writer, reading).await?;
        let line = command.line();
        seen(&line);
        let tag = line.split_once(' ').map_or(line.as_str(), |(tag, _)| tag);
        let reply = match settled {
            None => return Some(command),
            Some(Settled::Held) => std::future::pending().await,
            Some(Settled::Refused) => format!("{tag} NO literal refused\r\n"),
            Some(Settled::Unread) => format!("{tag} BAD literal not read so\r\n"),
        };
        writer.write_all(reply.as_bytes()).await.ok()?;
        writer.flush().await.ok()?;
    }
}

/// One command, up to where it was settled if it was.
async fn read_one<R, W>(
    reader: &mut R,
    writer: &mut W,
    reading: Reading,
) -> Option<(Command, Option<Settled>)>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let (mut command, mut settled) = (Command::default(), None);
    loop {
        let (text, marker) = text_run(reader).await?;
        command.parts.push(Part::Text(text));
        let Some((size, synchronizing)) = marker.filter(|(size, _)| *size <= MAX_LITERAL_BYTES)
        else {
            return marker.is_none().then_some((command, settled));
        };
        if synchronizing {
            match reading.continuation {
                Continuation::Asks => {
                    writer.write_all(CONTINUATION).await.ok()?;
                    writer.flush().await.ok()?;
                }
                Continuation::Withholds => return Some((command, Some(Settled::Held))),
                Continuation::Refuses => return Some((command, Some(Settled::Refused))),
            }
        } else if !reading.literals.unsynchronized(size) {
            settled = Some(Settled::Unread);
        }
        command
            .parts
            .push(Part::Literal(literal(reader, size).await?));
    }
}

/// The size a marker at the end of the line names and whether its
/// literal waits for the continuation request: `{n}` or `{n+}` (RFC 7888
/// section 8).
fn marker(line: &str) -> Option<(usize, bool)> {
    let inner = line.strip_suffix('}')?;
    let (_, digits) = inner.rsplit_once('{')?;
    let (digits, synchronizing) = match digits.strip_suffix('+') {
        Some(digits) => (digits, false),
        None => (digits, true),
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some((digits.parse().ok()?, synchronizing))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One command off the wire with the bytes the server wrote back.
    async fn one(
        wire: &[u8],
        literals: Literals,
        continuation: Continuation,
    ) -> (Option<(Command, Option<Settled>)>, Vec<u8>) {
        let mut reader = tokio::io::BufReader::new(wire);
        let mut written = Vec::new();
        let reading = Reading {
            literals,
            continuation,
        };
        let read = read_one(&mut reader, &mut written, reading).await;
        (read, written)
    }

    #[tokio::test]
    async fn a_synchronizing_literal_is_asked_for_and_a_plus_one_is_not_rfc7888_3() {
        let wire = b"A1 UID SEARCH TEXT {4}\r\na\r\nb FROM {2+}\r\nzz\r\n";
        let (taken, written) = one(wire, Literals::Plus, Continuation::Asks).await;
        let (command, settled) = taken.unwrap();
        assert_eq!(
            command.parts,
            [
                Part::Text("A1 UID SEARCH TEXT {4}".to_owned()),
                Part::Literal("a\r\nb".to_owned()),
                Part::Text(" FROM {2+}".to_owned()),
                Part::Literal("zz".to_owned()),
                Part::Text(String::new()),
            ]
        );
        assert_eq!(settled, None);
        assert_eq!(written, CONTINUATION);
        assert_eq!(
            command.line(),
            "A1 UID SEARCH TEXT {4}\r\na\r\nb FROM {2+}\r\nzz"
        );
    }

    #[tokio::test]
    async fn a_literal_withheld_or_refused_settles_the_command_before_its_bytes_rfc3501_2_2_1() {
        let wire = b"A1 UID SEARCH TEXT {4}\r\n";
        for (continuation, held) in [
            (Continuation::Withholds, Settled::Held),
            (Continuation::Refuses, Settled::Refused),
        ] {
            let (taken, written) = one(wire, Literals::Plus, continuation).await;
            let (command, settled) = taken.unwrap();
            assert_eq!(settled, Some(held));
            assert_eq!(command.line(), "A1 UID SEARCH TEXT {4}");
            assert_eq!(written, b"");
        }
    }

    #[tokio::test]
    async fn a_plus_literal_the_script_does_not_read_so_is_read_whole_and_answered_bad_rfc7888_4() {
        let long = "x".repeat(4097);
        let wire = format!("A1 X {{4097+}}\r\n{long}\r\nA2 NOOP\r\n");
        let (taken, _) = one(wire.as_bytes(), Literals::Minus, Continuation::Asks).await;
        assert_eq!(taken.unwrap().1, Some(Settled::Unread));
        let short = b"A1 X {1+}\r\nx\r\n";
        let (taken, _) = one(short, Literals::Synchronizing, Continuation::Asks).await;
        assert_eq!(taken.unwrap().1, Some(Settled::Unread));
        let (taken, _) = one(short, Literals::Minus, Continuation::Asks).await;
        assert_eq!(taken.unwrap().1, None);
        let mut reader = tokio::io::BufReader::new(wire.as_bytes());
        let (mut written, mut seen) = (Vec::new(), 0);
        let reading = Reading {
            literals: Literals::Minus,
            continuation: Continuation::Asks,
        };
        let next = read(&mut reader, &mut written, reading, |_| seen += 1).await;
        assert_eq!(next.unwrap().line(), "A2 NOOP");
        assert_eq!(written, b"A1 BAD literal not read so\r\n");
        assert_eq!(seen, 2);
    }

    #[test]
    fn only_a_size_marker_at_the_end_of_the_line_names_a_literal() {
        assert_eq!(marker("A1 X {12}"), Some((12, true)));
        assert_eq!(marker("A1 X {12+}"), Some((12, false)));
        for line in ["A1 X {}", "A1 X {+}", "A1 X {1a}", "A1 X {12} Y", "A1 NOOP"] {
            assert_eq!(marker(line), None, "{line}");
        }
    }
}
