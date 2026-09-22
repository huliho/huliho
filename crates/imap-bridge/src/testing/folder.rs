// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! One folder of the scripted IMAP server: what LIST and STATUS say
//! about it and the mail the selected-mailbox commands answer from.

use super::messages::Message;

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
    /// The label the folder shows on a Gmail account; its STATUS counts
    /// then follow the messages of All Mail that carry it.
    pub label: Option<String>,
    pub messages: u32,
    pub unseen: u32,
    pub uid_next: u32,
    pub uid_validity: u32,
    pub highest_modseq: u64,
    /// The mail EXAMINE, UID SEARCH and UID FETCH answer from; the
    /// counts above are what STATUS says and may differ on purpose.
    pub mail: Vec<Message>,
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
            label: None,
            messages: 0,
            unseen: 0,
            uid_next: 1,
            uid_validity: 1,
            highest_modseq: 1,
            mail: Vec::new(),
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

    /// A label folder of a Gmail account: no mail of its own, the counts
    /// from the messages of All Mail under the label.
    #[must_use]
    pub fn labeled(name: &str, label: &str) -> Self {
        Self {
            label: Some(label.to_owned()),
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

    /// The same folder holding this mail, the counts following it: a
    /// message without `\Seen` is unseen.
    #[must_use]
    pub fn with_mail(self, mail: Vec<Message>) -> Self {
        let mut folder = Self { mail, ..self };
        folder.recount();
        folder
    }

    /// The counts and UIDNEXT as the mail has them; the highest
    /// mod-sequence of any message where it passes the folder's.
    pub(super) fn recount(&mut self) {
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        self.messages = count(self.mail.len());
        self.unseen = count(
            self.mail
                .iter()
                .filter(|message| !message.flags.iter().any(|flag| flag == "\\Seen"))
                .count(),
        );
        let top = self.mail.iter().map(|message| message.uid).max();
        self.uid_next = self.uid_next.max(top.map_or(1, |uid| uid + 1));
        let modseq = self.mail.iter().map(|message| message.modseq).max();
        self.highest_modseq = self.highest_modseq.max(modseq.unwrap_or(0));
    }

    pub(super) fn is_selectable(&self) -> bool {
        !self
            .attributes
            .iter()
            .any(|attribute| attribute.eq_ignore_ascii_case("\\Noselect"))
    }

    /// The highest UID the folder holds.
    pub(super) fn top(&self) -> u32 {
        self.mail
            .iter()
            .map(|message| message.uid)
            .max()
            .unwrap_or(0)
    }
}
