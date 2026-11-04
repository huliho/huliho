// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! APPEND (RFC 3501 section 6.3.11) and `UID REPLACE` (RFC 8508) of the
//! scripted server. The message stays in the mailbox model, so a later
//! FETCH reads it back; its header, its size and a BODYSTRUCTURE come from
//! its own bytes. An APPEND to a label of a Gmail account lands in All
//! Mail under a UID of the label folder, as Gmail names it, its `\Draft`
//! flag shown as the label alone.

use std::fmt::Write as _;

use mail_parser::{MessageParser, MessagePart, MimeHeaders, PartType};

use super::gmail::ALL_MAIL;
use super::{Extension, Mailboxes, unquote};
use crate::testing::behavior::Storing;
use crate::testing::command::{Command, Part};
use crate::testing::folder::Folder;
use crate::testing::messages::Message;

/// The flag All Mail shows as a label on a Gmail account.
const DRAFT: &str = "\\Draft";

/// What one message command asked.
struct Asked<'a> {
    /// The message a `UID REPLACE` takes away.
    uid: Option<u32>,
    mailbox: String,
    flags: Vec<String>,
    literal: &'a str,
}

/// What a command comes to before the tagged line.
enum Answer {
    /// The lines ahead of the OK and the code it carries.
    Took(String, String),
    /// `NO`, with what stands after it.
    No(&'static str),
    Bad,
}

impl Mailboxes {
    /// The answer to an APPEND or a `UID REPLACE`, its tagged line
    /// included, on a connection with that mailbox open; `None` once the
    /// connection closes instead.
    pub(in crate::testing) async fn message_answer(
        &self,
        (open, writable): (Option<&str>, bool),
        received: &Command,
        tag: &str,
    ) -> Option<String> {
        match self.behavior.appending {
            Storing::Stores => {}
            Storing::Refuses => return Some(format!("{tag} NO refused\r\n")),
            Storing::Bad => return Some(format!("{tag} BAD refused\r\n")),
            Storing::Drops => return None,
            Storing::Stalls => std::future::pending().await,
        }
        let answer = match read(received) {
            None => Answer::Bad,
            Some(asked) => self.took(&asked, open.filter(|_| writable)),
        };
        Some(match answer {
            Answer::Took(lines, code) => format!("{lines}{tag} OK {code}done\r\n"),
            Answer::No(words) => format!("{tag} NO {words}\r\n"),
            Answer::Bad => format!("{tag} BAD not offered\r\n"),
        })
    }

    fn took(&self, asked: &Asked<'_>, open: Option<&str>) -> Answer {
        if asked.uid.is_some() && !self.has(Extension::Replace) {
            return Answer::Bad;
        }
        let bytes = u64::try_from(asked.literal.len()).unwrap_or(u64::MAX);
        if self.append_limit.is_some_and(|limit| bytes > limit) {
            return Answer::No("[TOOBIG] too big");
        }
        let uid_plus = self.has(Extension::Uidplus);
        let mut folders = self.lock();
        let Some(into) = folders
            .iter()
            .position(|folder| folder.name == asked.mailbox && folder.is_selectable())
            .filter(|into| !folders[*into].read_only)
        else {
            return Answer::No("refused");
        };
        let mut lines = String::new();
        if let Some(uid) = asked.uid {
            let Some(source) = open.and_then(|name| folders.iter().position(|f| f.name == name))
            else {
                return Answer::No("refused");
            };
            let Some(number) = folders[source].mail.iter().position(|m| m.uid == uid) else {
                return Answer::No("refused");
            };
            let (validity, new_uid) = self.arrive(&mut folders, into, asked);
            if uid_plus {
                let _ = write!(
                    lines,
                    "* OK [APPENDUID {validity} {new_uid}] Replacement Message ready\r\n"
                );
            }
            let exists = folders[into].mail.len();
            let _ = write!(lines, "* {exists} EXISTS\r\n* {} EXPUNGE\r\n", number + 1);
            folders[source].mail.remove(number);
            folders[source].recount();
            return Answer::Took(lines, String::new());
        }
        let (validity, new_uid) = self.arrive(&mut folders, into, asked);
        let code = if uid_plus {
            format!("[APPENDUID {validity} {new_uid}] ")
        } else {
            String::new()
        };
        Answer::Took(lines, code)
    }

    /// The message in its mailbox; the UIDVALIDITY and the UID the answer
    /// names. A label folder of a Gmail account hands it to All Mail and
    /// names a UID of its own.
    fn arrive(&self, folders: &mut [Folder], into: usize, asked: &Asked<'_>) -> (u32, u32) {
        let label = folders[into]
            .label
            .clone()
            .filter(|_| self.has(Extension::Gmail));
        let flags = asked.flags.clone();
        let Some(label) = label else {
            let folder = &mut folders[into];
            let uid = folder.uid_next;
            let modseq = folder.highest_modseq + 1;
            folder.mail.push(message(uid, modseq, flags, asked.literal));
            folder.recount();
            return (folder.uid_validity, uid);
        };
        let (label_validity, label_uid) = {
            let folder = &mut folders[into];
            let uid = folder.uid_next;
            folder.uid_next += 1;
            (folder.uid_validity, uid)
        };
        if let Some(store) = folders.iter_mut().find(|folder| folder.name == ALL_MAIL) {
            let uid = store.uid_next;
            let modseq = store.highest_modseq + 1;
            let shown: Vec<String> = flags.into_iter().filter(|flag| flag != DRAFT).collect();
            let arrived = Message {
                labels: vec![label],
                ..message(uid, modseq, shown, asked.literal)
            };
            store.mail.push(arrived);
            store.recount();
        }
        (label_validity, label_uid)
    }
}

/// `APPEND "<name>" [(flags)] {n}` or `UID REPLACE <uid> "<name>" ...`
/// with its literal; `None` for any other shape.
fn read(received: &Command) -> Option<Asked<'_>> {
    let [Part::Text(line), Part::Literal(literal), ..] = received.parts.as_slice() else {
        return None;
    };
    let (_, command) = line.split_once(' ')?;
    let (uid, rest) = if let Some(rest) = command.strip_prefix("APPEND ") {
        (None, rest)
    } else {
        let rest = command.strip_prefix("UID REPLACE ")?;
        let (uid, rest) = rest.split_once(' ')?;
        (Some(uid.parse().ok()?), rest)
    };
    let (mailbox, rest) = unquote(rest)?;
    let rest = rest.trim_start();
    let flags = match rest.strip_prefix('(') {
        Some(inner) => inner.split_once(')')?.0,
        None => "",
    };
    let flags = flags.split_whitespace().map(str::to_owned).collect();
    Some(Asked {
        uid,
        mailbox,
        flags,
        literal,
    })
}

/// A message from its bytes.
fn message(uid: u32, modseq: u64, flags: Vec<String>, literal: &str) -> Message {
    let (header, body) = literal.split_once("\r\n\r\n").unwrap_or((literal, ""));
    Message {
        flags,
        size: u32::try_from(literal.len()).unwrap_or(u32::MAX),
        header: format!("{header}\r\n\r\n"),
        structure: structure(literal),
        body: body.to_owned(),
        modseq,
        labels: Vec::new(),
        ..Message::new(uid)
    }
}

/// The BODYSTRUCTURE of a message as the wire carries it.
fn structure(literal: &str) -> String {
    let parsed = MessageParser::default().parse(literal.as_bytes());
    let parts = parsed.as_ref().map(|message| message.parts.as_slice());
    parts
        .and_then(|parts| parts.first().map(|part| written(parts, part)))
        .unwrap_or_else(|| crate::testing::messages::plain_leaf(literal.len()))
}

fn written(parts: &[MessagePart<'_>], part: &MessagePart<'_>) -> String {
    let content_type = part.content_type();
    let main = content_type.map_or("text", |found| found.c_type.as_ref());
    let sub = content_type
        .and_then(|found| found.c_subtype.as_deref())
        .unwrap_or("plain");
    if let PartType::Multipart(children) = &part.body {
        let inner: String = children
            .iter()
            .filter_map(|child| parts.get(usize::try_from(*child).ok()?))
            .map(|child| written(parts, child))
            .collect();
        return format!("({inner} \"{}\")", sub.to_ascii_uppercase());
    }
    let (main, sub) = (main.to_ascii_uppercase(), sub.to_ascii_uppercase());
    let encoding = part
        .content_transfer_encoding()
        .unwrap_or("7BIT")
        .to_ascii_uppercase();
    let size = part.len();
    let lines = if main == "TEXT" { " 1" } else { "" };
    let disposition = part
        .content_disposition()
        .map_or_else(String::new, |found| {
            let name = found
                .attribute("filename")
                .map_or_else(String::new, |name| format!(" (\"FILENAME\" \"{name}\")"));
            format!(" NIL (\"{}\"{name})", found.c_type.to_ascii_uppercase())
        });
    format!(
        "(\"{main}\" \"{sub}\" (\"CHARSET\" \"utf-8\") NIL NIL \"{encoding}\" {size}{lines}{disposition})"
    )
}
