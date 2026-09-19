// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The mailbox model of the scripted IMAP server: LIST, LSUB and STATUS
//! over folders a test edits between passes, behind the extensions the
//! script advertises.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

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

/// One folder the server lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    /// The name as the wire carries it, modified UTF-7 included.
    pub name: String,
    /// The attributes beyond the special-use one, as written.
    pub attributes: Vec<&'static str>,
    /// The special-use attribute, sent only while SPECIAL-USE is on.
    pub special_use: Option<&'static str>,
    pub subscribed: bool,
    /// STATUS answers NO for this folder and a LIST-STATUS answer leaves
    /// its line out, while LIST shows it selectable.
    pub refuses_status: bool,
    pub messages: u32,
    pub unseen: u32,
    pub uid_next: u32,
    pub uid_validity: u32,
    pub highest_modseq: u64,
}

impl Folder {
    /// A selectable, subscribed, empty folder.
    #[must_use]
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            attributes: vec!["\\HasNoChildren"],
            special_use: None,
            subscribed: true,
            refuses_status: false,
            messages: 0,
            unseen: 0,
            uid_next: 1,
            uid_validity: 1,
            highest_modseq: 1,
        }
    }

    /// A folder with a special-use attribute such as `\Sent`.
    #[must_use]
    pub fn special(name: &str, attribute: &'static str) -> Self {
        Self {
            special_use: Some(attribute),
            ..Self::new(name)
        }
    }

    /// A hierarchy placeholder nothing can select.
    #[must_use]
    pub fn noselect(name: &str) -> Self {
        Self {
            attributes: vec!["\\Noselect", "\\HasChildren"],
            ..Self::new(name)
        }
    }

    /// The same folder holding `messages` of which `unseen` are unread.
    #[must_use]
    pub fn with_counts(self, messages: u32, unseen: u32) -> Self {
        Self {
            messages,
            unseen,
            uid_next: messages + 1,
            ..self
        }
    }

    fn is_selectable(&self) -> bool {
        !self
            .attributes
            .iter()
            .any(|attribute| attribute.eq_ignore_ascii_case("\\Noselect"))
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

    /// The folders as they stand.
    #[must_use]
    pub fn folders(&self) -> Vec<Folder> {
        self.lock().clone()
    }

    fn lock(&self) -> MutexGuard<'_, Vec<Folder>> {
        self.folders.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn has(&self, extension: Extension) -> bool {
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
fn unquote(text: &str) -> Option<(String, &str)> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_return_option_behind_an_absent_extension_is_bad_rfc5258() {
        let plain = Mailboxes::default();
        let bad = plain.answer("LIST", "LIST \"\" \"*\" RETURN (SUBSCRIBED)", "A1");
        assert_eq!(bad, "A1 BAD not offered\r\n");
        let extended = Mailboxes::new(
            vec![Folder::new("INBOX")],
            [Extension::ListExtended].into_iter().collect(),
        );
        let special = extended.answer("LIST", "LIST \"\" \"*\" RETURN (SPECIAL-USE)", "A1");
        assert_eq!(special, "A1 BAD not offered\r\n");
        let status = extended.answer("LIST", "LIST \"\" \"*\" RETURN (STATUS (MESSAGES))", "A1");
        assert_eq!(status, "A1 BAD not offered\r\n");
    }

    #[test]
    fn a_dovecot_shaped_list_carries_attributes_status_and_the_tagged_ok() {
        let mailboxes = Mailboxes::dovecot();
        let answer = mailboxes.answer(
            "LIST",
            "LIST \"\" \"*\" RETURN (SUBSCRIBED SPECIAL-USE STATUS (MESSAGES UNSEEN))",
            "A2",
        );
        assert!(
            answer.starts_with("* LIST (\\HasNoChildren \\Subscribed) \"/\" \"INBOX\"\r\n* STATUS \"INBOX\" (MESSAGES 17 UNSEEN 3)\r\n"),
            "{answer}"
        );
        assert!(answer.contains("* LIST (\\HasNoChildren \\Subscribed \\Sent) \"/\" \"Sent\"\r\n"));
        assert!(answer.ends_with("A2 OK done\r\n"));
    }

    #[test]
    fn status_answers_no_for_a_missing_mailbox_and_quotes_specials() {
        let mailboxes = Mailboxes::new(vec![Folder::new("Say \"hi\"")], BTreeSet::new());
        let found = mailboxes.answer("STATUS", "STATUS \"Say \\\"hi\\\"\" (MESSAGES)", "A3");
        assert_eq!(
            found,
            "* STATUS \"Say \\\"hi\\\"\" (MESSAGES 0)\r\nA3 OK done\r\n"
        );
        let missing = mailboxes.answer("STATUS", "STATUS \"Other\" (MESSAGES)", "A4");
        assert_eq!(missing, "A4 NO refused\r\n");
        let modseq = mailboxes.answer("STATUS", "STATUS \"Say \\\"hi\\\"\" (HIGHESTMODSEQ)", "A5");
        assert_eq!(modseq, "A5 BAD not offered\r\n");
    }

    #[test]
    fn a_folder_that_refuses_status_is_listed_as_selectable_and_answers_no() {
        let shared = Folder {
            refuses_status: true,
            ..Folder::new("Shared")
        };
        let mailboxes = Mailboxes::new(vec![shared], BTreeSet::new());
        let listed = mailboxes.answer("LIST", "LIST \"\" \"*\"", "A6");
        assert_eq!(
            listed,
            "* LIST (\\HasNoChildren) \"/\" \"Shared\"\r\nA6 OK done\r\n"
        );
        let refused = mailboxes.answer("STATUS", "STATUS \"Shared\" (MESSAGES)", "A7");
        assert_eq!(refused, "A7 NO refused\r\n");
    }
}
