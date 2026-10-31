// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The message model of the scripted IMAP server: EXAMINE, SELECT, NOOP,
//! UID FETCH and UID STORE over the mail of a folder, with the ways a
//! server misbehaves as switches. UID SEARCH answers from `search`; UID
//! MOVE, UID COPY and UID EXPUNGE go to the mailbox model.

pub mod search;

use std::fmt::Write as _;

use super::behavior::{Behavior, Storing};
use super::fetch;
use super::folder::Folder;
use super::mailboxes::{Mailboxes, Refusal, is_transfer, labels_of, quote, unquote};
use super::parts::Part;

/// Where the scripted Gmail ids start, so no UID reads as one.
const GMAIL_ID_BASE: u64 = 1_000_000_000_000;

/// The size a scripted message states is this much above its UID, so a
/// test reads the UID back from the size.
pub const SIZE_ABOVE_UID: u32 = 1000;

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
    /// The one text part of the message, as the wire carries it.
    pub body: String,
    /// The numbered parts of a multipart message; empty for one part.
    pub parts: Vec<Part>,
    /// The Content-Type of that part.
    pub content_type: String,
    /// Its Content-Transfer-Encoding, where the message names one.
    pub transfer_encoding: Option<String>,
    /// The mod-sequence of the message (RFC 7162 section 3.1.2).
    pub modseq: u64,
    /// X-GM-LABELS as the wire spells them: an atom for a system label,
    /// a name for a user label.
    pub labels: Vec<String>,
    /// X-GM-MSGID.
    pub msgid: u64,
    /// X-GM-THRID; a message is its own thread unless a test says so.
    pub thrid: u64,
}

/// A text/plain body of a dozen bytes.
pub const PLAIN: &str = "(\"TEXT\" \"PLAIN\" (\"CHARSET\" \"utf-8\") NIL NIL \"7BIT\" 12 1)";

/// An attached PDF.
const PDF: &str = "(\"APPLICATION\" \"PDF\" (\"NAME\" \"a.pdf\") NIL NIL \"BASE64\" 9 NIL (\"ATTACHMENT\" (\"FILENAME\" \"a.pdf\")))";

/// A text/plain leaf of `bytes` bytes.
#[must_use]
pub fn plain_leaf(bytes: usize) -> String {
    format!("(\"TEXT\" \"PLAIN\" (\"CHARSET\" \"utf-8\") NIL NIL \"7BIT\" {bytes} 1)")
}

/// A text/html leaf of `bytes` bytes.
#[must_use]
pub fn html_leaf(bytes: usize) -> String {
    format!("(\"TEXT\" \"HTML\" (\"CHARSET\" \"utf-8\") NIL NIL \"7BIT\" {bytes} 1)")
}

impl Message {
    /// A seen plain-text message whose header, date and body follow
    /// from the UID; the structure states the body's size.
    #[must_use]
    pub fn new(uid: u32) -> Self {
        let body = format!("Body of message {uid}.");
        Self {
            uid,
            flags: vec!["\\Seen".to_owned()],
            internal_date: format!("01-Jan-2026 00:{:02}:{:02} +0000", uid / 60 % 60, uid % 60),
            size: SIZE_ABOVE_UID + uid,
            header: format!(
                "From: Sanne <sanne@example.test>\r\nTo: mo@example.test\r\nSubject: Message {uid}\r\nMessage-ID: <m{uid}@example.test>\r\n\r\n"
            ),
            structure: plain_leaf(body.len()),
            body,
            parts: Vec::new(),
            content_type: "text/plain; charset=utf-8".to_owned(),
            transfer_encoding: None,
            modseq: 1,
            labels: vec!["\\Inbox".to_owned()],
            msgid: GMAIL_ID_BASE + u64::from(uid),
            thrid: GMAIL_ID_BASE + u64::from(uid),
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

    /// The same message under these labels.
    #[must_use]
    pub fn labeled(self, labels: &[&str]) -> Self {
        Self {
            labels: labels.iter().map(|label| (*label).to_owned()).collect(),
            ..self
        }
    }

    /// The same message in the thread of that id.
    #[must_use]
    pub fn in_thread(self, thrid: u64) -> Self {
        Self { thrid, ..self }
    }

    /// The same message with a PDF attached; its text is part 1.
    #[must_use]
    pub fn with_attachment(self) -> Self {
        Self {
            structure: format!("({}{PDF} \"MIXED\")", plain_leaf(self.body.len())),
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

    /// The same message as one HTML part holding `body`.
    #[must_use]
    pub fn html(self, body: &str) -> Self {
        Self {
            structure: html_leaf(body.len()),
            body: body.to_owned(),
            content_type: "text/html; charset=utf-8".to_owned(),
            ..self
        }
    }

    /// The same message with its text under a transfer encoding.
    #[must_use]
    pub fn encoded(self, encoding: &str, body: &str) -> Self {
        Self {
            body: body.to_owned(),
            transfer_encoding: Some(encoding.to_owned()),
            ..self
        }
    }

    /// The MIME header of the text part, as a fetch of its header
    /// fields or of `<part>.MIME` answers it.
    pub(super) fn mime_header(&self) -> String {
        let mut header = format!("Content-Type: {}\r\n", self.content_type);
        if let Some(encoding) = &self.transfer_encoding {
            let _ = write!(header, "Content-Transfer-Encoding: {encoding}\r\n");
        }
        header.push_str("\r\n");
        header
    }
}

/// What one connection remembers between commands.
#[derive(Debug, Default)]
pub(super) struct Conversation {
    selected: Option<String>,
    /// Whether a SELECT opened the selected mailbox and the folder takes
    /// writes; a STORE, a MOVE and an EXPUNGE need one.
    writable: bool,
    fetches: usize,
    stores: usize,
    selects: usize,
    searched: bool,
}

impl Conversation {
    /// Whether this command deletes the mailbox the connection has
    /// open.
    pub(super) fn deletes_selected(&self, command: &str) -> bool {
        let selected = self.selected.as_deref();
        selected.is_some_and(|name| command == format!("DELETE {}", quote(name)))
    }

    /// Whether this command is the UID FETCH or a UID STORE the
    /// behavior never answers.
    pub(super) fn stalls(&self, command: &str, behavior: Behavior) -> bool {
        (command.starts_with("UID FETCH ") && behavior.stalls_at == Some(self.fetches))
            || (command.starts_with("UID STORE ") && self.storing(behavior) == Storing::Stalls)
            || (is_transfer(command) && behavior.moving == Storing::Stalls)
    }

    /// How the next UID STORE of this connection is taken.
    fn storing(&self, behavior: Behavior) -> Storing {
        if self.stores < behavior.storing_from {
            Storing::Stores
        } else {
            behavior.storing
        }
    }

    /// The answer to EXAMINE, SELECT, NOOP or a UID command; `None`
    /// closes the connection. The tagged OK of an open names the access
    /// it granted (RFC 3501 section 6.3.1).
    pub(super) fn answer(
        &mut self,
        mailboxes: &Mailboxes,
        command: &str,
        tag: &str,
    ) -> Option<String> {
        let open = (self.selected.as_deref(), self.writable);
        if is_transfer(command) && mailboxes.behavior.moving == Storing::Drops {
            return None;
        }
        if let Some(reply) = mailboxes.transfer(open, command, tag) {
            return Some(reply);
        }
        let behavior = mailboxes.behavior;
        let mut access = "";
        let reply = if let Some(rest) = command.strip_prefix("EXAMINE ") {
            access = "[READ-ONLY] ";
            self.open(mailboxes, rest, None)
        } else if let Some(rest) = command.strip_prefix("SELECT ") {
            let opened = self.open(mailboxes, rest, Some(self.selects));
            self.selects += 1;
            access = if self.writable {
                "[READ-WRITE] "
            } else {
                "[READ-ONLY] "
            };
            opened
        } else if let Some(rest) = command.strip_prefix("UID STORE ") {
            let storing = self.storing(behavior);
            self.stores += 1;
            if storing == Storing::Drops {
                return None;
            }
            match self.store(mailboxes, rest, storing) {
                Ok(lines) => Some(lines),
                Err(Refusal::Bad) => return Some(format!("{tag} BAD not offered\r\n")),
                Err(Refusal::No) => None,
            }
        } else if command == "NOOP" {
            Some(String::new())
        } else if let Some(rest) = command.strip_prefix("UID FETCH ") {
            if behavior.drops_after == Some(self.fetches) {
                return None;
            }
            self.fetches += 1;
            let answered = self
                .folder(mailboxes)
                .ok_or(Refusal::No)
                .and_then(|folder| fetch::answer(&folder, rest, mailboxes));
            match answered {
                Ok(lines) => Some(lines),
                Err(Refusal::Bad) => return Some(format!("{tag} BAD not offered\r\n")),
                Err(Refusal::No) => None,
            }
        } else {
            None
        };
        Some(match reply {
            Some(lines) => format!("{lines}{tag} OK {access}done\r\n"),
            None if behavior.utf8_no => format!("{tag} NO Mailbox doesn't exist: Caf\u{e9}\r\n"),
            None => format!("{tag} NO refused\r\n"),
        })
    }

    /// `UID STORE <set> +FLAGS.SILENT (<flags>)` or `-FLAGS.SILENT` on
    /// the mailbox a SELECT opened, and the same of X-GM-LABELS on a
    /// Gmail account: NO on one an EXAMINE opened, for a flag the folder
    /// does not keep and where the behavior refuses, BAD where it says
    /// so or the extension is off.
    fn store(
        &self,
        mailboxes: &Mailboxes,
        rest: &str,
        storing: Storing,
    ) -> Result<String, Refusal> {
        if storing == Storing::Bad {
            return Err(Refusal::Bad);
        }
        let (set, items) = rest.split_once(' ').ok_or(Refusal::Bad)?;
        let (head, list) = items.split_once(".SILENT (").ok_or(Refusal::Bad)?;
        let list = list.strip_suffix(')').ok_or(Refusal::Bad)?;
        let (add, item) = match head.split_at_checked(1) {
            Some(("+", item)) => (true, item),
            Some(("-", item)) => (false, item),
            _ => return Err(Refusal::Bad),
        };
        let selected = self.selected.as_deref().filter(|_| self.writable);
        let selected = selected.filter(|_| storing != Storing::Refuses);
        match item {
            "FLAGS" => {
                let flags: Vec<&str> = list.split(' ').collect();
                selected.and_then(|name| mailboxes.store(name, (set, &flags), add))
            }
            "X-GM-LABELS" if mailboxes.has(super::mailboxes::Extension::Gmail) => {
                let labels = labels_of(list).ok_or(Refusal::Bad)?;
                selected.and_then(|name| mailboxes.store_labels(name, (set, &labels), add))
            }
            _ => return Err(Refusal::Bad),
        }
        .ok_or(Refusal::No)
    }

    /// EXAMINE, or the SELECT with that index on this connection; a
    /// SELECT names the flags the folder keeps and opens a read-only
    /// folder, or one the behavior closes to writes, read-only. The
    /// SELECT the behavior renumbers shows the UIDVALIDITY one past the
    /// folder's.
    fn open(&mut self, mailboxes: &Mailboxes, rest: &str, select: Option<usize>) -> Option<String> {
        let (name, _) = unquote(rest)?;
        let folder = mailboxes
            .folders()
            .into_iter()
            .find(|folder| folder.name == name)?;
        let behavior = mailboxes.behavior;
        let writable = select.is_some_and(|index| behavior.read_only_at != Some(index));
        let renumbered = select.is_some_and(|index| behavior.renumbered_at == Some(index));
        let uid_validity = if renumbered {
            folder.uid_validity + 1
        } else {
            folder.uid_validity
        };
        self.selected = Some(name);
        self.writable = writable && !folder.read_only;
        let permanent = match folder.permanent.filter(|_| writable) {
            Some(flags) => format!("* OK [PERMANENTFLAGS ({flags})] Flags permitted\r\n"),
            None => String::new(),
        };
        let exists = if behavior.no_exists {
            String::new()
        } else {
            format!("* {} EXISTS\r\n", folder.mail.len())
        };
        let modseq = if mailboxes.has(super::mailboxes::Extension::Condstore) {
            format!("* OK [HIGHESTMODSEQ {}] Highest\r\n", folder.highest_modseq)
        } else {
            String::new()
        };
        Some(format!(
            "* FLAGS (\\Answered \\Flagged \\Deleted \\Seen \\Draft)\r\n{permanent}{exists}* 0 RECENT\r\n* OK [UIDVALIDITY {uid_validity}] UIDs valid\r\n* OK [UIDNEXT {}] Predicted next UID\r\n{modseq}",
            folder.uid_next
        ))
    }

    fn folder(&self, mailboxes: &Mailboxes) -> Option<Folder> {
        let selected = self.selected.as_deref()?;
        mailboxes
            .folders()
            .into_iter()
            .find(|folder| folder.name == selected)
    }
}
