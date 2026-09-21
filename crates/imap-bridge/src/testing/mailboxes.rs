// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The mailbox model of the scripted IMAP server: LIST, LSUB and STATUS
//! over folders a test edits between passes, behind the extensions the
//! script advertises.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super::folder::Folder;
use super::messages::{Behavior, Message};

#[cfg(test)]
mod tests;

/// An extension the scripted server advertises and honors; a command
/// that needs one it does not advertise gets a BAD.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Extension {
    ListExtended,
    ListStatus,
    SpecialUse,
    Condstore,
}

impl Extension {
    fn capability(self) -> &'static str {
        match self {
            Self::ListExtended => "LIST-EXTENDED",
            Self::ListStatus => "LIST-STATUS",
            Self::SpecialUse => "SPECIAL-USE",
            Self::Condstore => "CONDSTORE",
        }
    }

    /// Every extension, the shape of Dovecot.
    #[must_use]
    pub fn all() -> BTreeSet<Self> {
        [
            Self::ListExtended,
            Self::ListStatus,
            Self::SpecialUse,
            Self::Condstore,
        ]
        .into_iter()
        .collect()
    }
}

/// The folders behind one scripted server, shared with the test that
/// edits them between passes.
#[derive(Debug, Clone)]
pub struct Mailboxes {
    folders: Arc<Mutex<Vec<Folder>>>,
    /// The hierarchy delimiter.
    pub delimiter: char,
    /// What the server advertises and honors.
    pub extensions: BTreeSet<Extension>,
    /// An `* OK [ALERT]` line after the first folder's lines of a LIST
    /// answer.
    pub chatter: bool,
    /// How the server misbehaves on a selected mailbox.
    pub behavior: Behavior,
}

/// Why a command was refused: BAD for a form outside the advertised
/// extensions, NO for a mailbox STATUS has no answer for.
enum Refusal {
    Bad,
    No,
}

impl Default for Mailboxes {
    /// One INBOX behind no extension.
    fn default() -> Self {
        Self::new(vec![Folder::new("INBOX")], BTreeSet::new())
    }
}

impl Mailboxes {
    /// The given folders behind the given extensions, `/` between
    /// levels.
    #[must_use]
    pub fn new(folders: Vec<Folder>, extensions: BTreeSet<Extension>) -> Self {
        Self {
            folders: Arc::new(Mutex::new(folders)),
            delimiter: '/',
            extensions,
            chatter: false,
            behavior: Behavior::default(),
        }
    }

    /// A Dovecot with the English defaults: their attributes, an
    /// Archive, every extension.
    #[must_use]
    pub fn dovecot() -> Self {
        Self::new(
            vec![
                Folder::new("INBOX").with_counts(17, 3),
                Folder::special("Drafts", "\\Drafts"),
                Folder::special("Sent", "\\Sent"),
                Folder::special("Junk", "\\Junk"),
                Folder::special("Trash", "\\Trash"),
                Folder::special("Archive", "\\Archive"),
            ],
            Extension::all(),
        )
    }

    /// Replaces the folders; the next LIST shows them.
    pub fn set(&self, folders: Vec<Folder>) {
        *self.lock() = folders;
    }

    /// Adds one folder.
    pub fn push(&self, folder: Folder) {
        self.lock().push(folder);
    }

    /// Removes the folder of that name, if any.
    pub fn remove(&self, name: &str) {
        self.lock().retain(|folder| folder.name != name);
    }

    /// Drops the lowest `count` messages of the folder of that name.
    pub(super) fn expunge(&self, name: &str, count: usize) {
        self.edit(name, |folder| {
            folder.mail.drain(..count.min(folder.mail.len()));
        });
    }

    /// Delivers a message to the folder of that name, as another client
    /// would: the counts, UIDNEXT and the mod-sequence follow.
    pub fn append(&self, name: &str, message: Message) {
        self.edit(name, |folder| {
            let modseq = folder.highest_modseq + 1;
            folder.mail.push(Message { modseq, ..message });
        });
    }

    /// Removes the message of that UID from the folder of that name.
    pub fn expunge_uid(&self, name: &str, uid: u32) {
        self.edit(name, |folder| {
            folder.mail.retain(|message| message.uid != uid);
        });
    }

    /// Replaces the flags of the message of that UID; its mod-sequence
    /// moves past the folder's (RFC 7162 section 3.1.2).
    pub fn store_flags(&self, name: &str, uid: u32, flags: &[&str]) {
        self.edit(name, |folder| {
            let modseq = folder.highest_modseq + 1;
            if let Some(message) = folder.mail.iter_mut().find(|message| message.uid == uid) {
                message.flags = flags.iter().map(|flag| (*flag).to_owned()).collect();
                message.modseq = modseq;
            }
        });
    }

    /// Replaces the flags of every message of the folder of that name,
    /// as a client that marks a folder read does; every mod-sequence
    /// moves past the folder's.
    pub fn mark_all(&self, name: &str, flags: &[&str]) {
        self.edit(name, |folder| {
            let modseq = folder.highest_modseq + 1;
            for message in &mut folder.mail {
                message.flags = flags.iter().map(|flag| (*flag).to_owned()).collect();
                message.modseq = modseq;
            }
        });
    }

    /// One edit of a folder with its counts brought up to date.
    fn edit(&self, name: &str, change: impl FnOnce(&mut Folder)) {
        if let Some(folder) = self.lock().iter_mut().find(|folder| folder.name == name) {
            change(folder);
            folder.recount();
        }
    }

    /// The folders as they stand.
    #[must_use]
    pub fn folders(&self) -> Vec<Folder> {
        self.lock().clone()
    }

    fn lock(&self) -> MutexGuard<'_, Vec<Folder>> {
        self.folders.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(super) fn has(&self, extension: Extension) -> bool {
        self.extensions.contains(&extension)
    }

    /// The capability words the extensions add.
    pub(super) fn capabilities(&self) -> String {
        self.extensions
            .iter()
            .map(|extension| extension.capability())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The answer to LIST, LSUB or STATUS: the untagged lines, then the
    /// tagged one. Any other verb is BAD.
    pub(super) fn answer(&self, verb: &str, command: &str, tag: &str) -> String {
        let reply = match verb {
            "LIST" => self.list(command),
            "LSUB" => Ok(self.lsub()),
            "STATUS" => self.status(command),
            _ => Err(Refusal::Bad),
        };
        match reply {
            Ok(lines) => format!("{lines}{tag} OK done\r\n"),
            Err(Refusal::Bad) => format!("{tag} BAD not offered\r\n"),
            Err(Refusal::No) => format!("{tag} NO refused\r\n"),
        }
    }

    /// `LIST "" "*"` with an optional RETURN list, every option checked
    /// against the extensions (RFC 5258, RFC 6154 section 2, RFC 5819).
    fn list(&self, command: &str) -> Result<String, Refusal> {
        let options = match command.split_once(" RETURN (") {
            Some((_, rest)) if self.has(Extension::ListExtended) => rest.trim_end_matches(')'),
            Some(_) => return Err(Refusal::Bad),
            None => "",
        };
        if options.contains("SPECIAL-USE") && !self.has(Extension::SpecialUse) {
            return Err(Refusal::Bad);
        }
        let status_items = options.split_once("STATUS (").map(|(_, items)| items);
        if status_items.is_some() && !self.has(Extension::ListStatus) {
            return Err(Refusal::Bad);
        }
        let subscribed = options.contains("SUBSCRIBED");
        let mut lines = String::new();
        for (index, folder) in self.folders().iter().enumerate() {
            lines.push_str(&self.list_line("LIST", folder, subscribed));
            // RFC 5819 section 2: a mailbox STATUS would refuse gets no line.
            if let Some(items) = status_items
                && folder.is_selectable()
                && !folder.refuses_status
            {
                lines.push_str(&self.status_line(folder, items)?);
            }
            if self.chatter && index == 0 {
                lines.push_str("* OK [ALERT] scripted\r\n");
            }
        }
        Ok(lines)
    }

    fn lsub(&self) -> String {
        self.folders()
            .iter()
            .filter(|folder| folder.subscribed)
            .map(|folder| self.list_line("LSUB", folder, false))
            .collect()
    }

    /// `STATUS <name> (<items>)` on one folder.
    fn status(&self, command: &str) -> Result<String, Refusal> {
        let rest = command.strip_prefix("STATUS ").ok_or(Refusal::Bad)?;
        let (name, rest) = unquote(rest).ok_or(Refusal::Bad)?;
        let items = rest
            .trim()
            .strip_prefix('(')
            .and_then(|items| items.strip_suffix(')'))
            .ok_or(Refusal::Bad)?;
        let folders = self.folders();
        let folder = folders
            .iter()
            .find(|folder| folder.name == name && folder.is_selectable() && !folder.refuses_status)
            .ok_or(Refusal::No)?;
        self.status_line(folder, items)
    }

    fn list_line(&self, verb: &str, folder: &Folder, subscribed: bool) -> String {
        let mut attributes = folder.attributes.clone();
        if subscribed && folder.subscribed {
            attributes.push("\\Subscribed");
        }
        if self.has(Extension::SpecialUse)
            && let Some(special) = folder.special_use
        {
            attributes.push(special);
        }
        format!(
            "* {verb} ({}) \"{}\" {}\r\n",
            attributes.join(" "),
            self.delimiter,
            quote(&folder.name)
        )
    }

    /// The STATUS line for the items asked, in the order asked;
    /// HIGHESTMODSEQ needs CONDSTORE (RFC 7162 section 3.1.7).
    fn status_line(&self, folder: &Folder, items: &str) -> Result<String, Refusal> {
        let mut pairs = Vec::new();
        for item in items.split_whitespace() {
            let value = match item {
                "MESSAGES" => u64::from(folder.messages),
                "UNSEEN" => u64::from(folder.unseen),
                "UIDNEXT" => u64::from(folder.uid_next),
                "UIDVALIDITY" => u64::from(folder.uid_validity),
                "HIGHESTMODSEQ" if self.has(Extension::Condstore) => folder.highest_modseq,
                _ => return Err(Refusal::Bad),
            };
            pairs.push(format!("{item} {value}"));
        }
        Ok(format!(
            "* STATUS {} ({})\r\n",
            quote(&folder.name),
            pairs.join(" ")
        ))
    }
}

/// A quoted string with `\` and `"` escaped (RFC 9051 section 4.3); a
/// name holding a line break travels as a literal.
fn quote(name: &str) -> String {
    if name.contains(['\r', '\n']) {
        return format!("{{{}}}\r\n{name}", name.len());
    }
    let escaped = name.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

/// Reads one quoted string off the front; the rest follows it.
pub(super) fn unquote(text: &str) -> Option<(String, &str)> {
    let mut chars = text.strip_prefix('"')?.char_indices();
    let mut name = String::new();
    while let Some((index, c)) = chars.next() {
        match c {
            '\\' => name.push(chars.next()?.1),
            '"' => return Some((name, &text[index + 2..])),
            other => name.push(other),
        }
    }
    None
}
