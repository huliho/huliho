// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The transfer encoding of a part undone window by window (RFC 2045
//! section 6): base64 and quoted-printable through mail-parser's
//! decoders, any other encoding as the bytes stand. What a window
//! leaves unfinished waits for the next one, so a quartet, an escape or
//! a line is never decoded in two halves and the edge of a window
//! changes nothing in what a part decodes to; the one exception is a
//! quoted-printable line past the longest a message may carry, which
//! is cut where it stands. Whoever sent the mail wrote these bytes:
//! what is no encoding among them is read past or as it stands, never
//! as a reason to give the part up.

#[cfg(test)]
mod tests;

use mail_parser::decoders::base64::base64_decode;
use mail_parser::parsers::MessageStream;

/// What pads the last quartet of a base64 body.
const PAD: u8 = b'=';

/// The symbols of one base64 quartet.
const QUARTET: usize = 4;

/// The longest unfinished escape a cut can leave: `=X`.
const ESCAPE_BYTES: usize = 2;

/// The escape an equals sign is written as.
const EQUALS: &[u8] = b"=3D";

/// The longest line a message may carry with its line break (RFC 5322
/// section 2.1.1); a longer one is cut where it stands rather than
/// waited for.
const LONG_LINE: usize = 1000;

/// How the bytes of a blob become its content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Encoding {
    /// The bytes are the content: a whole message, a part under `7bit`,
    /// `8bit` or `binary` and a part under an encoding nobody knows
    /// (RFC 8621 section 4.1.4).
    Identity,
    Base64,
    QuotedPrintable,
}

/// The decoder of one blob: its encoding and what the last window left
/// unfinished.
pub(super) struct Decoder {
    encoding: Encoding,
    carried: Vec<u8>,
}

impl Decoder {
    /// A decoder that hands the bytes on as they come.
    pub(super) fn identity() -> Self {
        Self {
            encoding: Encoding::Identity,
            carried: Vec::new(),
        }
    }

    /// The decoder for a transfer encoding as the structure names it,
    /// in lower case.
    pub(super) fn of(encoding: &str) -> Self {
        let encoding = match encoding {
            "base64" => Encoding::Base64,
            "quoted-printable" => Encoding::QuotedPrintable,
            _ => Encoding::Identity,
        };
        Self {
            encoding,
            carried: Vec::new(),
        }
    }

    /// Whether the bytes need no decoding.
    pub(super) fn is_identity(&self) -> bool {
        self.encoding == Encoding::Identity
    }

    /// The content of the next window; `None` where the decoder gives
    /// the bytes up, which the way they are handed to it leaves it no
    /// cause for. `last` says no window follows, so what is still
    /// carried decodes with it.
    pub(super) fn push(&mut self, window: Vec<u8>, last: bool) -> Option<Vec<u8>> {
        match self.encoding {
            Encoding::Identity => Some(window),
            Encoding::Base64 => {
                // Bytes outside the alphabet are ignored (RFC 2045
                // section 6.8), so a stray one never fails a download.
                let symbols = window
                    .into_iter()
                    .filter(|byte| symbol(*byte) || *byte == PAD);
                self.carried.extend(symbols);
                let cut = if last {
                    self.carried.len()
                } else {
                    quartets(&self.carried)
                };
                let content = base64_decode(&self.carried[..cut]);
                self.carried.drain(..cut);
                content
            }
            Encoding::QuotedPrintable => {
                self.carried.extend_from_slice(&window);
                let cut = if last {
                    self.carried.len()
                } else {
                    lines(&self.carried)
                };
                let content = unquoted(&self.carried[..cut]);
                self.carried.drain(..cut);
                content
            }
        }
    }
}

/// Lines of quoted-printable decoded; `None` where the decoder gives
/// them up, which the mending leaves it no cause for.
fn unquoted(lines: &[u8]) -> Option<Vec<u8>> {
    let mended = mended(lines);
    let (end, content) = MessageStream::new(&mended).decode_quoted_printable_mime(&[]);
    (end != usize::MAX).then(|| content.into_owned())
}

/// The bytes as the decoder reads them without loss and without regard
/// to what came before them (RFC 2045 section 6.7). An `=` that opens
/// neither an escape nor a soft break is written as its own escape,
/// where the decoder would give every byte up. Each line ends in CRLF,
/// the form a line break of the encoding stands for.
fn mended(bytes: &[u8]) -> Vec<u8> {
    let mut mended = Vec::with_capacity(bytes.len());
    let mut before = 0;
    for (at, byte) in bytes.iter().enumerate() {
        match byte {
            b'=' if !opens(&bytes[at + 1..]) => mended.extend_from_slice(EQUALS),
            b'\n' if before != b'\r' => mended.extend_from_slice(b"\r\n"),
            _ => mended.push(*byte),
        }
        before = *byte;
    }
    mended
}

/// Whether what follows an `=` makes it an escape or a soft line break:
/// two hex digits, or nothing but white space up to the line break.
fn opens(rest: &[u8]) -> bool {
    if let [first, second, ..] = rest
        && first.is_ascii_hexdigit()
        && second.is_ascii_hexdigit()
    {
        return true;
    }
    let blank = rest
        .iter()
        .take_while(|byte| matches!(byte, b' ' | b'\t' | b'\r'))
        .count();
    rest.get(blank) == Some(&b'\n')
}

/// A symbol of the base64 alphabet (RFC 2045 section 6.8, table 1).
fn symbol(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/')
}

/// The end of the last whole quartet among symbols and pads; a pad
/// ends a quartet early, as the decoder reads it.
fn quartets(symbols: &[u8]) -> usize {
    let mut pending = 0;
    let mut end = 0;
    for (index, symbol) in symbols.iter().enumerate() {
        pending = if *symbol == PAD {
            0
        } else {
            (pending + 1) % QUARTET
        };
        if pending == 0 {
            end = index + 1;
        }
    }
    end
}

/// Where quoted-printable bytes may be cut: after their last line
/// break, so an escape, a soft break and the white space ahead of a
/// line end stay whole. A line still open waits for its break; one
/// past `LONG_LINE` is cut ahead of an escape or a CR the cut would
/// split.
fn lines(bytes: &[u8]) -> usize {
    if let Some(last) = bytes.iter().rposition(|byte| *byte == b'\n') {
        return last + 1;
    }
    if bytes.len() < LONG_LINE {
        return 0;
    }
    let tail = bytes.len().saturating_sub(ESCAPE_BYTES);
    bytes[tail..]
        .iter()
        .position(|byte| matches!(byte, b'=' | b'\r'))
        .map_or(bytes.len(), |at| tail + at)
}
