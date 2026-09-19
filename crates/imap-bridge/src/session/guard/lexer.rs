// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Enough of the response grammar (RFC 3501 section 9) to tell a
//! parenthesis that opens a list from one inside a quoted string or a
//! literal. A string is told apart only where the protocol parser does
//! the same, on an untagged data line; everywhere else every
//! parenthesis counts, so the lexer never sees less depth than the
//! parser.

use super::Limit;

/// The parenthesis levels one response may open. A message forwarded as
/// an attachment ten times over nests some 25 deep; the parser takes
/// about 20 KiB of stack per level in an unoptimized build, so this
/// many fit half the 2 MiB of a worker thread.
pub const MAX_NESTING: usize = 32;

/// The bytes one response may take, literals included: the UID list of
/// a folder of some three million messages, the largest answer the read
/// path asks for.
pub const MAX_RESPONSE_BYTES: usize = 32 * 1024 * 1024;

/// The words that open a line of free text (RFC 3501 section 7.1).
const STATUS_WORDS: [&[u8]; 5] = [b"OK", b"NO", b"BAD", b"BYE", b"PREAUTH"];

/// The longest status word, `PREAUTH`.
const WORD_BYTES: usize = 7;

/// Where the lexer stands in the stream.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Lexer {
    line: Line,
    depth: usize,
    bytes: usize,
    /// Whether the last byte was a carriage return that may end a line.
    cr: bool,
}

/// How far the current line is understood.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Line {
    /// Nothing read yet.
    #[default]
    Start,
    /// A `*` read; a space makes it an untagged line.
    Star,
    /// The word after `* `, in upper case, until it ends or fills.
    Word { bytes: [u8; WORD_BYTES], len: usize },
    /// A status line, a tagged line or a continuation: no string is
    /// told apart.
    Text,
    /// Untagged data: quoted strings and literals are skipped.
    Data(Mode),
}

/// Where the lexer stands on a data line (RFC 3501 section 4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Plain,
    Quoted,
    /// After a backslash inside a quoted string.
    Escaped,
    /// Inside `{`: the size so far, `None` before its first digit.
    Size(Option<u32>),
    /// After `}`: the carriage return is due.
    SizeCr(u32),
    /// After that: the line feed is due.
    SizeLf(u32),
    /// Inside a literal with this many bytes to go.
    Literal(u32),
}

impl Lexer {
    /// Reads one chunk; an error names the bound the stream passed.
    pub(super) fn feed(&mut self, chunk: &[u8]) -> Result<(), Limit> {
        let mut rest = chunk;
        while let Some((&byte, tail)) = rest.split_first() {
            if let Line::Data(Mode::Literal(left)) = self.line {
                let skipped = usize::try_from(left).map_or(rest.len(), |left| left.min(rest.len()));
                self.count(skipped)?;
                let left = left.saturating_sub(u32::try_from(skipped).unwrap_or(left));
                self.line = Line::Data(if left == 0 {
                    Mode::Plain
                } else {
                    Mode::Literal(left)
                });
                rest = &rest[skipped..];
                continue;
            }
            self.count(1)?;
            self.step(byte)?;
            rest = tail;
        }
        Ok(())
    }

    fn count(&mut self, bytes: usize) -> Result<(), Limit> {
        self.bytes = self.bytes.saturating_add(bytes);
        if self.bytes > MAX_RESPONSE_BYTES {
            return Err(Limit::Size);
        }
        Ok(())
    }

    fn step(&mut self, byte: u8) -> Result<(), Limit> {
        match self.line {
            Line::Start => {
                self.line = if byte == b'*' { Line::Star } else { Line::Text };
                self.text(byte)
            }
            Line::Star => {
                self.line = if byte == b' ' {
                    Line::Word {
                        bytes: [0; WORD_BYTES],
                        len: 0,
                    }
                } else {
                    Line::Text
                };
                self.text(byte)
            }
            Line::Word { bytes, len } => {
                self.line = word(bytes, len, byte);
                self.text(byte)
            }
            Line::Text => self.text(byte),
            Line::Data(mode) => self.data(mode, byte),
        }
    }

    /// A byte where no string is told apart: every parenthesis counts
    /// and a line feed after a carriage return ends the response.
    fn text(&mut self, byte: u8) -> Result<(), Limit> {
        match byte {
            b'(' => {
                self.depth += 1;
                if self.depth > MAX_NESTING {
                    return Err(Limit::Nesting);
                }
            }
            b')' => self.depth = self.depth.saturating_sub(1),
            _ => {}
        }
        self.end_of_line(byte);
        Ok(())
    }

    fn end_of_line(&mut self, byte: u8) {
        if byte == b'\n' && self.cr {
            *self = Self::default();
        } else {
            self.cr = byte == b'\r';
        }
    }

    /// A byte on a data line. A size the parser would refuse is no
    /// literal and an escape it would refuse no escape, so both read as
    /// the bytes they are.
    fn data(&mut self, mode: Mode, byte: u8) -> Result<(), Limit> {
        let next = match (mode, byte) {
            (Mode::Plain, b'"') | (Mode::Escaped, b'\\' | b'"') => Mode::Quoted,
            (Mode::Plain, b'{') => Mode::Size(None),
            (Mode::Plain, _) => return self.text(byte),
            (Mode::Quoted, b'\\') => Mode::Escaped,
            (Mode::Quoted, b'"') | (Mode::SizeLf(0), b'\n') => Mode::Plain,
            (Mode::Quoted, _) => {
                // A quoted string holds no line break, so one ends the response here too.
                self.end_of_line(byte);
                return Ok(());
            }
            (Mode::Escaped, _) => return self.again(Mode::Quoted, byte),
            (Mode::Size(digits), b'0'..=b'9') => digits
                .unwrap_or(0)
                .checked_mul(10)
                .and_then(|size| size.checked_add(u32::from(byte - b'0')))
                .map_or(Mode::Plain, |size| Mode::Size(Some(size))),
            (Mode::Size(Some(size)), b'}') => Mode::SizeCr(size),
            (Mode::SizeCr(size), b'\r') => Mode::SizeLf(size),
            (Mode::SizeLf(size), b'\n') => Mode::Literal(size),
            (Mode::Literal(left), _) if left > 1 => Mode::Literal(left - 1),
            (Mode::Literal(_), _) => Mode::Plain,
            (Mode::Size(_) | Mode::SizeCr(_) | Mode::SizeLf(_), _) => {
                return self.again(Mode::Plain, byte);
            }
        };
        self.line = Line::Data(next);
        self.cr = false;
        Ok(())
    }

    /// Reads the byte once more in the mode it turned out to belong to.
    fn again(&mut self, mode: Mode, byte: u8) -> Result<(), Limit> {
        self.line = Line::Data(mode);
        self.data(mode, byte)
    }
}

/// The line kind once the word after `* ` ends or fills: free text
/// behind a status word, data otherwise. A word that only starts like a
/// status word counts as one, which errs toward counting everything.
fn word(mut bytes: [u8; WORD_BYTES], len: usize, byte: u8) -> Line {
    let ended = matches!(byte, b' ' | b'\r');
    let mut len = len;
    if !ended {
        bytes[len] = byte.to_ascii_uppercase();
        len += 1;
    }
    if !ended && len < WORD_BYTES {
        return Line::Word { bytes, len };
    }
    if STATUS_WORDS
        .iter()
        .any(|status| bytes[..len].starts_with(status))
    {
        Line::Text
    } else {
        Line::Data(Mode::Plain)
    }
}
