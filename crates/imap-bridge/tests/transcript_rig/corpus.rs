// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The mail of the scenario suite: five messages in the account at the
//! start and one delivered later, written in the fixed shapes the
//! redaction leaves alone, plus the edits a second client makes and how
//! the scripted server's model plays them.

use std::fmt::Write as _;

use huliho_imap_bridge::testing::mailboxes::{ALL_MAIL, SPAM};
use huliho_imap_bridge::testing::record::fixed::{FIXED_DOMAIN, fixed_subject};
use huliho_imap_bridge::testing::{Mailboxes, Message};

/// The header every seeded message carries, so a run finds what an
/// earlier one left.
pub const MARKER: (&str, &str) = ("X-Huliho-Live", "transcript");

/// The user label the suite creates and one message gains.
pub const LABEL: &str = "Project";

/// The messages in the account when the suite starts.
pub const SEEDED: u32 = 5;

/// The message a second client delivers while the suite runs.
pub const DELIVERED: u32 = 6;

/// The message whose body runs past the preview ask.
pub const LONG: u32 = 4;

/// The one word the long body repeats.
const LONG_WORD: &str = "word ";

/// The bytes of the long body, past the two KiB a plain part is asked
/// for.
pub const LONG_BODY_BYTES: usize = 3000;

/// The label a flagged message gains on Gmail.
const STARRED: &str = "\\Starred";

/// The label every message of the inbox carries.
const INBOX_LABEL: &str = "\\Inbox";

/// One message of the corpus.
pub struct Seed {
    pub number: u32,
    pub seen: bool,
    pub reply_to: Option<u32>,
    pub attachment: bool,
    pub body: String,
}

/// The messages by number: a reply to the first, one with an attachment,
/// one long, one unseen.
pub fn seed(number: u32) -> Seed {
    let body = if number == LONG {
        LONG_WORD.repeat(LONG_BODY_BYTES / LONG_WORD.len())
    } else {
        format!("Body of message {number}.")
    };
    Seed {
        number,
        seen: number != SEEDED,
        reply_to: (number == 2).then_some(1),
        attachment: number == 3,
        body,
    }
}

/// The five messages seeded at the start, oldest first.
pub fn seeds() -> Vec<Seed> {
    (1..=SEEDED).map(seed).collect()
}

/// The fixed subject of the message with that number.
fn subject_of(number: u32) -> String {
    fixed_subject(usize::try_from(number).unwrap())
}

impl Seed {
    pub fn subject(&self) -> String {
        match self.reply_to {
            Some(parent) => format!("Re: {}", subject_of(parent)),
            None => subject_of(self.number),
        }
    }

    pub fn message_id(&self) -> String {
        format!("<m{}@{FIXED_DOMAIN}>", self.number)
    }

    /// INTERNALDATE as APPEND takes it, one day per message. Gmail
    /// stamps its own arrival time instead, so nothing reads the date
    /// back; the order of the numbers is the order of the appends.
    pub fn internal_date(&self) -> String {
        format!("{:02}-Jan-2027 12:00:00 +0000", self.number)
    }

    fn header(&self) -> String {
        let mut header = format!(
            "From: Sanne <sanne@{FIXED_DOMAIN}>\r\nTo: mo@{FIXED_DOMAIN}\r\nSubject: {}\r\nDate: {}\r\nMessage-ID: {}\r\n",
            self.subject(),
            self.date(),
            self.message_id()
        );
        if let Some(parent) = self.reply_to {
            let id = seed(parent).message_id();
            let _ = write!(header, "In-Reply-To: {id}\r\nReferences: {id}\r\n");
        }
        let _ = write!(
            header,
            "{}: {}\r\nMIME-Version: 1.0\r\n",
            MARKER.0, MARKER.1
        );
        header
    }

    /// The Date header, the same instant as INTERNALDATE.
    fn date(&self) -> String {
        format!("{:02} Jan 2027 12:00:00 +0000", self.number)
    }

    /// The message as APPEND takes it.
    pub fn rfc5322(&self) -> String {
        let header = self.header();
        if self.attachment {
            return format!(
                "{header}Content-Type: multipart/mixed; boundary=\"b1\"\r\n\r\n--b1\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{}\r\n--b1\r\nContent-Type: text/plain; name=\"notes.txt\"\r\nContent-Disposition: attachment; filename=\"notes.txt\"\r\n\r\nnotes\r\n--b1--\r\n",
                self.body
            );
        }
        format!(
            "{header}Content-Type: text/plain; charset=utf-8\r\n\r\n{}\r\n",
            self.body
        )
    }

    /// The same message as the scripted server holds it, its UID the
    /// number.
    pub fn model(&self) -> Message {
        let text = format!(
            "(\"TEXT\" \"PLAIN\" (\"CHARSET\" \"utf-8\") NIL NIL \"7BIT\" {} 1)",
            self.body.len()
        );
        let structure = if self.attachment {
            format!(
                "({text}(\"TEXT\" \"PLAIN\" (\"NAME\" \"notes.txt\") NIL NIL \"7BIT\" 5 1 NIL (\"ATTACHMENT\" (\"FILENAME\" \"notes.txt\"))) \"MIXED\")"
            )
        } else {
            text
        };
        let flags = if self.seen {
            vec!["\\Seen"]
        } else {
            Vec::new()
        };
        let mut message = Message {
            internal_date: self.internal_date(),
            size: u32::try_from(self.rfc5322().len()).unwrap(),
            header: format!("{}\r\n", self.header()),
            structure,
            body: self.body.clone(),
            ..Message::new(self.number)
        }
        .flagged(&flags);
        if let Some(parent) = self.reply_to {
            message = message.in_thread(Message::new(parent).thrid);
        }
        message
    }
}

/// What a second client does to the account while the suite runs, by
/// message number.
#[derive(Debug, Clone, Copy)]
pub enum Change {
    /// A message arrives in the inbox.
    Deliver(u32),
    /// A message gains the flag.
    Flag(u32, &'static str),
    /// A message gains the user label.
    Label(u32),
    /// A message is marked spam.
    MoveToSpam(u32),
    /// A message is deleted for good.
    DeleteForever(u32),
}

/// The change as the scripted server's model takes it; a flag on Gmail
/// shows as a label too.
pub fn apply_to_model(mailboxes: &Mailboxes, change: Change) {
    match change {
        Change::Deliver(number) => mailboxes.append(ALL_MAIL, seed(number).model()),
        Change::Flag(number, flag) => {
            mailboxes.store_flags(ALL_MAIL, number, &["\\Seen", flag]);
            mailboxes.relabel(ALL_MAIL, number, &[INBOX_LABEL, STARRED]);
        }
        Change::Label(number) => mailboxes.relabel(ALL_MAIL, number, &[INBOX_LABEL, LABEL]),
        Change::MoveToSpam(number) => mailboxes.move_to((ALL_MAIL, number), SPAM),
        Change::DeleteForever(number) => mailboxes.expunge_uid(ALL_MAIL, number),
    }
}
