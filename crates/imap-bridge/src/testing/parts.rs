// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The parts of a multipart message on the scripted server: the bytes a
//! fetch of each section and of the whole message answers, the corpus
//! message the body tests share and the content of a part past a
//! window.

use std::borrow::Cow;
use std::fmt::Write as _;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;

use super::messages::Message;

/// The longest line a sender writes of base64 (RFC 2045 section 6.8).
const BASE64_LINE: usize = 76;

/// The byte values varied content runs through, a prime so no window
/// edge meets the same value twice in a row.
const BYTE_VALUES: u8 = 251;

/// Content of that many bytes, the byte values taken round and round.
#[must_use]
pub fn varied(bytes: usize) -> Vec<u8> {
    (0..BYTE_VALUES).cycle().take(bytes).collect()
}

/// The content under base64 in lines of 76 symbols, as a sender writes
/// a part.
#[must_use]
pub fn base64_lines(content: &[u8]) -> String {
    let encoded = BASE64.encode(content);
    let mut lines = String::new();
    for line in encoded.as_bytes().chunks(BASE64_LINE) {
        lines.push_str(&String::from_utf8_lossy(line));
        lines.push_str("\r\n");
    }
    lines
}

/// One section of a message with the raw bytes a fetch of it answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    /// The part number as a fetch names it, `1.2` for one.
    pub section: String,
    pub body: String,
}

/// A part by its section.
#[must_use]
pub fn part(section: &str, body: &str) -> Part {
    Part {
        section: section.to_owned(),
        body: body.to_owned(),
    }
}

/// The section a fetch names for the one part of a one-part message.
const TEXT_SECTION: &str = "TEXT";

/// The Content-ID of the corpus image, as the HTML references it.
pub const CORPUS_IMAGE_CID: &str = "logo@example.test";

/// The HTML part of the corpus message, its image by `cid:`.
pub const CORPUS_HTML: &str =
    "<html><body><p>Hello <b>there</b></p><img src=\"cid:logo@example.test\"></body></html>";

/// The plain alternative of the corpus message.
pub const CORPUS_PLAIN: &str = "Hello there";

/// The receiving server's verdict the corpus header carries.
pub const CORPUS_AUTHENTICATION: &str = "mx.example.test; dkim=pass header.d=example.test";

impl Message {
    /// The same message as the given structure over these parts.
    #[must_use]
    pub fn with_parts(self, structure: &str, parts: Vec<Part>) -> Self {
        Self {
            structure: structure.to_owned(),
            parts,
            ..self
        }
    }

    /// The whole message as a fetch of the empty section answers it:
    /// the header, then the text of a one-part message or the parts of
    /// a multipart one, each behind a line naming its section. The
    /// bytes stand in for a message; the structure is scripted beside
    /// them.
    #[must_use]
    pub fn raw(&self) -> String {
        let mut raw = self.header.clone();
        if self.parts.is_empty() {
            raw.push_str(&self.body);
        }
        for part in &self.parts {
            let _ = write!(raw, "--part {}\r\n{}\r\n", part.section, part.body);
        }
        raw
    }

    /// The bytes of one section: the whole message for the empty one,
    /// the text of a one-part message, a numbered part of a multipart
    /// one, `None` for a section the message lacks.
    pub(super) fn section(&self, section: &str) -> Option<Cow<'_, str>> {
        if section.is_empty() {
            return Some(Cow::Owned(self.raw()));
        }
        if section == TEXT_SECTION || (self.parts.is_empty() && section == "1") {
            return Some(Cow::Borrowed(&self.body));
        }
        self.parts
            .iter()
            .find(|part| part.section == section)
            .map(|part| Cow::Borrowed(part.body.as_str()))
    }
}

/// A message with an alternative body, an image by `cid:`, an
/// attachment under an RFC 2231 name and an attached message: mixed over
/// (alternative over plain and related over html and png), pdf, rfc822.
/// Its header carries an Authentication-Results line.
#[must_use]
pub fn corpus(uid: u32) -> Message {
    let plain = format!(
        "(\"TEXT\" \"PLAIN\" (\"CHARSET\" \"utf-8\") NIL NIL \"7BIT\" {} 1)",
        CORPUS_PLAIN.len()
    );
    let html = format!(
        "(\"TEXT\" \"HTML\" (\"CHARSET\" \"utf-8\") NIL NIL \"QUOTED-PRINTABLE\" {} 1)",
        CORPUS_HTML.len()
    );
    let png = format!(
        "(\"IMAGE\" \"PNG\" (\"NAME\" \"logo.png\") \"<{CORPUS_IMAGE_CID}>\" NIL \"BASE64\" 12 NIL (\"INLINE\" (\"FILENAME\" \"logo.png\")) NIL NIL)"
    );
    let pdf = "(\"APPLICATION\" \"PDF\" NIL NIL NIL \"BASE64\" 8 NIL (\"ATTACHMENT\" (\"FILENAME*\" \"utf-8''rapport%20caf%C3%A9.pdf\")) NIL NIL)";
    let envelope = "(NIL \"Attached\" NIL NIL NIL NIL NIL NIL NIL NIL)";
    let inner = "(\"TEXT\" \"PLAIN\" NIL NIL NIL \"7BIT\" 5 1)";
    let rfc822 = format!(
        "(\"MESSAGE\" \"RFC822\" NIL NIL NIL \"7BIT\" 90 {envelope} {inner} 4 NIL (\"ATTACHMENT\" (\"FILENAME\" \"inner.eml\")) NIL NIL)"
    );
    let related = format!("({html}{png} \"RELATED\" (\"TYPE\" \"text/html\") NIL NIL NIL)");
    let alternative = format!("({plain}{related} \"ALTERNATIVE\" NIL NIL NIL NIL)");
    let structure =
        format!("({alternative}{pdf}{rfc822} \"MIXED\" (\"BOUNDARY\" \"b1\") NIL NIL NIL)");
    let message = Message::new(uid);
    let header = format!(
        "Authentication-Results: {CORPUS_AUTHENTICATION}\r\n{}",
        message.header
    );
    Message { header, ..message }.with_parts(
        &structure,
        vec![
            part("1.1", CORPUS_PLAIN),
            part("1.2.1", CORPUS_HTML),
            part("1.2.2", "iVBORw0KGgo="),
            part("2", "JVBERi0x"),
            part("3", "Subject: Attached\r\n\r\nInner"),
        ],
    )
}
