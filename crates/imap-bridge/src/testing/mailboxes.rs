// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The mailbox model of the scripted IMAP server: the folders a test
//! edits between passes, the extensions the script advertises and the
//! STORE of flags on a selected folder; LIST, LSUB and STATUS answer
//! from `listing`.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super::behavior::Behavior;
use super::fetch;
use super::folder::Folder;
use super::messages::Message;
use crate::session::Literals;

mod gmail;
mod listing;
mod moves;
mod quota;
#[cfg(test)]
mod tests;

pub(super) use gmail::labels_of;
pub use gmail::{ALL_MAIL, SPAM, TRASH};
pub(super) use moves::is_transfer;
pub use quota::{QuotaAnswer, quota_lines};

/// An extension the scripted server advertises and honors; a command
/// that needs one it does not advertise gets a BAD.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Extension {
    ListExtended,
    ListStatus,
    SpecialUse,
    Condstore,
    /// UID MOVE (RFC 6851).
    Move,
    /// UID EXPUNGE and the `COPYUID` code (RFC 4315).
    Uidplus,
    /// GETQUOTAROOT (RFC 9208).
    Quota,
    /// Every literal read without a continuation request (RFC 7888).
    LiteralPlus,
    /// One of 4096 bytes at most read so (RFC 7888).
    LiteralMinus,
    /// The three Gmail FETCH items and the two Gmail search keys.
    Gmail,
}

impl Extension {
    fn capability(self) -> &'static str {
        match self {
            Self::ListExtended => "LIST-EXTENDED",
            Self::ListStatus => "LIST-STATUS",
            Self::SpecialUse => "SPECIAL-USE",
            Self::Condstore => "CONDSTORE",
            Self::Move => "MOVE",
            Self::Uidplus => "UIDPLUS",
            Self::Quota => "QUOTA",
            Self::LiteralPlus => "LITERAL+",
            Self::LiteralMinus => "LITERAL-",
            Self::Gmail => "X-GM-EXT-1",
        }
    }

    /// The extensions Dovecot advertises as it ships. QUOTA takes a
    /// plugin and the Gmail items are Gmail's own, so both stay out.
    #[must_use]
    pub fn all() -> BTreeSet<Self> {
        [
            Self::ListExtended,
            Self::ListStatus,
            Self::SpecialUse,
            Self::Condstore,
            Self::Move,
            Self::Uidplus,
            Self::LiteralPlus,
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
    /// What a GETQUOTAROOT answers, shared with the test that edits it
    /// between passes.
    quota: Arc<Mutex<QuotaAnswer>>,
    /// The UID SEARCH commands every connection received.
    searches: Arc<AtomicUsize>,
}

/// Why a command was refused: BAD for a form outside the advertised
/// extensions, NO for a mailbox the command has no answer for.
pub(super) enum Refusal {
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
            quota: Arc::new(Mutex::new(QuotaAnswer::default())),
            searches: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// The UID SEARCH commands the server received so far, on every
    /// connection.
    #[must_use]
    pub fn searches(&self) -> usize {
        self.searches.load(Ordering::SeqCst)
    }

    /// Counts one UID SEARCH as it arrives.
    pub(super) fn searched(&self) {
        self.searches.fetch_add(1, Ordering::SeqCst);
    }

    /// A Dovecot with the English defaults: their attributes, an
    /// Archive and the extensions it ships with.
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

    /// A STORE on the folder of that name: the messages of the set gain
    /// or lose the flags and their mod-sequence moves past the
    /// folder's. Answers the line a server with CONDSTORE sends per
    /// message even to a silent STORE (RFC 7162 section 3.1.3); `None`
    /// when the folder does not keep one of the flags, with nothing
    /// changed.
    pub(super) fn store(
        &self,
        name: &str,
        (set, flags): (&str, &[&str]),
        add: bool,
    ) -> Option<String> {
        let mut lines = None;
        let (condstore, gmail) = (self.has(Extension::Condstore), self.has(Extension::Gmail));
        self.edit(name, |folder| {
            if !flags.iter().all(|flag| folder.keeps(flag)) {
                return;
            }
            let (top, modseq) = (folder.top(), folder.highest_modseq + 1);
            let mut answered = String::new();
            for (index, message) in folder.mail.iter_mut().enumerate() {
                if !fetch::in_set(set, message.uid, top) {
                    continue;
                }
                message
                    .flags
                    .retain(|held| !flags.iter().any(|flag| flag.eq_ignore_ascii_case(held)));
                if add {
                    message
                        .flags
                        .extend(flags.iter().map(|flag| (*flag).to_owned()));
                }
                if gmail {
                    gmail::star_by_flag(message);
                }
                message.modseq = modseq;
                if condstore {
                    let (number, uid) = (index + 1, message.uid);
                    let _ = write!(
                        answered,
                        "* {number} FETCH (UID {uid} MODSEQ ({modseq}))\r\n"
                    );
                }
            }
            lines = Some(answered);
        });
        lines
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

    /// The literals the server reads without a continuation request.
    pub(super) fn literals(&self) -> Literals {
        if self.has(Extension::LiteralPlus) {
            Literals::Plus
        } else if self.has(Extension::LiteralMinus) {
            Literals::Minus
        } else {
            Literals::Synchronizing
        }
    }

    /// The capability words the extensions add.
    pub(super) fn capabilities(&self) -> String {
        self.extensions
            .iter()
            .map(|extension| extension.capability())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// A quoted string with `\` and `"` escaped (RFC 9051 section 4.3); a
/// name holding a line break travels as a literal.
pub(super) fn quote(name: &str) -> String {
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
