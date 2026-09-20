// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The message model of the scripted IMAP server: EXAMINE, UID SEARCH
//! over a window of sequence numbers and UID FETCH over the mail of a
//! folder, with the ways a server misbehaves as switches.

use std::fmt::Write as _;

use super::mailboxes::{Folder, Mailboxes, unquote};

/// One message a folder holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub uid: u32,
    pub flags: Vec<String>,
    /// INTERNALDATE as the wire carries it.
    pub internal_date: String,
    pub size: u32,
    /// The header block a FETCH of header fields answers, whole.
    pub header: String,
    /// BODYSTRUCTURE as the wire carries it.
    pub structure: String,
}

/// A text/plain body.
pub const PLAIN: &str = "(\"TEXT\" \"PLAIN\" (\"CHARSET\" \"utf-8\") NIL NIL \"7BIT\" 12 1)";

/// An attached PDF.
const PDF: &str = "(\"APPLICATION\" \"PDF\" (\"NAME\" \"a.pdf\") NIL NIL \"BASE64\" 9 NIL (\"ATTACHMENT\" (\"FILENAME\" \"a.pdf\")))";

impl Message {
    /// A seen plain-text message whose header and date follow from the
    /// UID.
    #[must_use]
    pub fn new(uid: u32) -> Self {
        Self {
            uid,
            flags: vec!["\\Seen".to_owned()],
            internal_date: format!("01-Jan-2026 00:{:02}:{:02} +0000", uid / 60 % 60, uid % 60),
            size: 1000 + uid,
            header: format!(
                "From: Sanne <sanne@example.test>\r\nTo: mo@example.test\r\nSubject: Message {uid}\r\nMessage-ID: <m{uid}@example.test>\r\n\r\n"
            ),
            structure: PLAIN.to_owned(),
        }
    }

    /// The same message under other flags.
    #[must_use]
    pub fn flagged(self, flags: &[&str]) -> Self {
        Self {
            flags: flags.iter().map(|flag| (*flag).to_owned()).collect(),
            ..self
        }
    }

    /// The same message with a PDF attached.
    #[must_use]
    pub fn with_attachment(self) -> Self {
        Self {
            structure: format!("({PLAIN}{PDF} \"MIXED\")"),
            ..self
        }
    }

    /// The same message with a PDF behind text parts side by side, as
    /// many as it takes for a structure of `bytes` bytes or more.
    #[must_use]
    pub fn sprawling(self, bytes: usize) -> Self {
        let parts = PLAIN.repeat(bytes.div_ceil(PLAIN.len()));
        Self {
            structure: format!("({parts}{PDF} \"MIXED\")"),
            ..self
        }
    }

    /// The same message with `depth` multiparts around its body.
    #[must_use]
    pub fn nested(self, depth: usize) -> Self {
        let mut structure = self.structure;
        for _ in 0..depth {
            structure = format!("({structure} \"MIXED\")");
        }
        Self { structure, ..self }
    }
}

/// How the server misbehaves on a selected mailbox.
#[derive(Debug, Clone, Copy, Default)]
pub struct Behavior {
    /// Lines nobody asked for ahead of the messages of every UID FETCH
    /// answer: this many EXISTS lines, then an EXPUNGE, a flag update
    /// for the first message, a message outside the range and, after
    /// the messages, a second line for the first one.
    pub volunteered: usize,
    /// A MODSEQ item on every FETCH line (RFC 7162 section 3.1.4).
    pub fetch_modseq: Option<u64>,
    /// EXAMINE of a missing folder answers NO with the name in raw
    /// UTF-8, as Dovecot does.
    pub utf8_no: bool,
    /// A connection closes instead of answering once it has answered
    /// this many UID FETCH commands.
    pub drops_after: Option<usize>,
    /// The lowest this many messages leave the folder behind the first
    /// UID SEARCH answer of a connection, an EXPUNGE line for each (RFC
    /// 3501 section 7.4.1).
    pub expunges: usize,
    /// EXAMINE answers without its EXISTS line.
    pub no_exists: bool,
}

/// What one connection remembers between commands.
#[derive(Debug, Default)]
pub(super) struct Conversation {
    selected: Option<String>,
    fetches: usize,
    searched: bool,
}

impl Conversation {
    /// The answer to EXAMINE or a UID command; `None` closes the
    /// connection.
    pub(super) fn answer(
        &mut self,
        mailboxes: &Mailboxes,
        command: &str,
        tag: &str,
    ) -> Option<String> {
        let behavior = mailboxes.behavior;
        let reply = if let Some(rest) = command.strip_prefix("EXAMINE ") {
            self.examine(mailboxes, rest)
        } else if let Some(window) = command.strip_prefix("UID SEARCH ") {
            let Some(lines) = self.search(mailboxes, window) else {
                return Some(format!("{tag} BAD no such sequence number\r\n"));
            };
            Some(lines)
        } else if let Some(rest) = command.strip_prefix("UID FETCH ") {
            if behavior.drops_after == Some(self.fetches) {
                return None;
            }
            self.fetches += 1;
            self.folder(mailboxes)
                .and_then(|folder| fetch(&folder, rest, behavior))
        } else {
            None
        };
        Some(match reply {
            Some(lines) => format!("{lines}{tag} OK done\r\n"),
            None if behavior.utf8_no => format!("{tag} NO Mailbox doesn't exist: Caf\u{e9}\r\n"),
            None => format!("{tag} NO refused\r\n"),
        })
    }

    fn examine(&mut self, mailboxes: &Mailboxes, rest: &str) -> Option<String> {
        let (name, _) = unquote(rest)?;
        let folder = mailboxes
            .folders()
            .into_iter()
            .find(|folder| folder.name == name)?;
        self.selected = Some(name);
        let exists = if mailboxes.behavior.no_exists {
            String::new()
        } else {
            format!("* {} EXISTS\r\n", folder.mail.len())
        };
        Some(format!(
            "* FLAGS (\\Answered \\Flagged \\Deleted \\Seen \\Draft)\r\n{exists}* 0 RECENT\r\n* OK [UIDVALIDITY {}] UIDs valid\r\n* OK [UIDNEXT {}] Predicted next UID\r\n",
            folder.uid_validity, folder.uid_next
        ))
    }

    /// `<low>:<high>` in sequence numbers: the UIDs of that window, then
    /// the expunges the behavior asks for. `None` for a number the
    /// folder does not hold (RFC 3501 section 9).
    fn search(&mut self, mailboxes: &Mailboxes, window: &str) -> Option<String> {
        let folder = self.folder(mailboxes)?;
        let (low, high) = window.split_once(':')?;
        let (low, high): (usize, usize) = (low.parse().ok()?, high.parse().ok()?);
        let found = folder.mail.get(low.checked_sub(1)?..high)?;
        let uids: Vec<String> = found
            .iter()
            .map(|message| message.uid.to_string())
            .collect();
        let mut lines = format!("* SEARCH {}\r\n", uids.join(" "));
        let leaving = mailboxes.behavior.expunges.min(folder.mail.len());
        if !self.searched && leaving > 0 {
            mailboxes.expunge(&folder.name, leaving);
            lines.push_str(&"* 1 EXPUNGE\r\n".repeat(leaving));
        }
        self.searched = true;
        Some(lines)
    }

    fn folder(&self, mailboxes: &Mailboxes) -> Option<Folder> {
        let selected = self.selected.as_deref()?;
        mailboxes
            .folders()
            .into_iter()
            .find(|folder| folder.name == selected)
    }
}

/// `<low>:<high> (<items>)`: a line per message in the range, with
/// BODYSTRUCTURE only where the items name it.
fn fetch(folder: &Folder, rest: &str, behavior: Behavior) -> Option<String> {
    let (range, items) = rest.split_once(' ')?;
    let (low, high) = range.split_once(':')?;
    let (low, high): (u32, u32) = (low.parse().ok()?, high.parse().ok()?);
    let asked = Asked {
        structure: items.contains("BODYSTRUCTURE"),
        fields: items
            .split_once("HEADER.FIELDS (")?
            .1
            .split_once(')')?
            .0
            .to_owned(),
        modseq: behavior.fetch_modseq,
    };
    let found: Vec<&Message> = folder
        .mail
        .iter()
        .filter(|message| (low..=high).contains(&message.uid))
        .collect();
    let mut lines = String::new();
    if behavior.volunteered > 0
        && let Some(first) = found.first()
    {
        for _ in 0..behavior.volunteered {
            lines.push_str("* 9 EXISTS\r\n");
        }
        let _ = write!(
            lines,
            "* 3 EXPUNGE\r\n* 1 FETCH (UID {} FLAGS (\\Flagged))\r\n",
            first.uid
        );
        lines.push_str(&line(&Message::new(high.saturating_add(1000)), &asked));
    }
    for message in &found {
        lines.push_str(&line(message, &asked));
    }
    if behavior.volunteered > 0
        && let Some(first) = found.first()
    {
        lines.push_str(&line(&(*first).clone().flagged(&["\\Flagged"]), &asked));
    }
    Some(lines)
}

/// What one UID FETCH asked for beyond the fixed items.
struct Asked {
    structure: bool,
    fields: String,
    modseq: Option<u64>,
}

fn line(message: &Message, asked: &Asked) -> String {
    let mut items = format!(
        "UID {} FLAGS ({}) INTERNALDATE \"{}\" RFC822.SIZE {}",
        message.uid,
        message.flags.join(" "),
        message.internal_date,
        message.size
    );
    if let Some(modseq) = asked.modseq {
        let _ = write!(items, " MODSEQ ({modseq})");
    }
    if asked.structure {
        let _ = write!(items, " BODYSTRUCTURE {}", message.structure);
    }
    format!(
        "* {} FETCH ({items} BODY[HEADER.FIELDS ({})] {{{}}}\r\n{})\r\n",
        message.uid,
        asked.fields,
        message.header.len(),
        message.header
    )
}
