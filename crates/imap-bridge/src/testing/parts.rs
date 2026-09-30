// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The parts of a multipart message on the scripted server: the bytes a
//! fetch of each section answers, and the corpus message the body tests
//! share.

use super::messages::Message;

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

    /// The bytes of one section: the text of a one-part message, a
    /// numbered part of a multipart one, `None` for a section the
    /// message lacks.
    pub(super) fn section(&self, section: &str) -> Option<&str> {
        if section == TEXT_SECTION || (self.parts.is_empty() && section == "1") {
            return Some(&self.body);
        }
        self.parts
            .iter()
            .find(|part| part.section == section)
            .map(|part| part.body.as_str())
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
