// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The Gmail shape of the scripted server: three stores, the system
//! folders as labels over All Mail, the counts of a label folder from
//! the labels of All Mail's messages and the edits a second client
//! makes to labels and stores.

use super::{Extension, Mailboxes};
use crate::testing::folder::Folder;
use crate::testing::messages::Message;

/// The wire name of the All Mail store, as Gmail lists it.
pub const ALL_MAIL: &str = "[Gmail]/All Mail";
/// The wire name of the Spam store.
pub const SPAM: &str = "[Gmail]/Spam";
/// The wire name of the Trash store.
pub const TRASH: &str = "[Gmail]/Trash";

/// The attribute of the store the labels are read over.
const ALL: &str = "\\All";

/// The label every message loses when it leaves All Mail for Spam or
/// Trash.
const INBOX_LABEL: &str = "\\Inbox";

impl Mailboxes {
    /// A Gmail account as it lists itself: INBOX, the `[Gmail]` folders
    /// with their attributes, the given mail in All Mail and every
    /// extension Gmail advertises. A label folder's counts follow the
    /// labels of that mail.
    #[must_use]
    pub fn gmail(mail: Vec<Message>) -> Self {
        let mut extensions = Extension::all();
        extensions.insert(Extension::Gmail);
        let system = |name: &str, attribute: &'static str, label: &str| Folder {
            label: Some(label.to_owned()),
            ..Folder::special(name, attribute)
        };
        Self::new(
            vec![
                Folder::labeled("INBOX", INBOX_LABEL),
                Folder::noselect("[Gmail]"),
                Folder::special(ALL_MAIL, ALL).with_mail(mail),
                system("[Gmail]/Drafts", "\\Drafts", "\\Drafts"),
                system("[Gmail]/Important", "\\Important", "\\Important"),
                system("[Gmail]/Sent Mail", "\\Sent", "\\Sent"),
                Folder::special(SPAM, "\\Junk"),
                system("[Gmail]/Starred", "\\Flagged", "\\Flagged"),
                Folder::special(TRASH, "\\Trash"),
            ],
            extensions,
        )
    }

    /// The same account with a user label, which messages carry by that
    /// wire name.
    #[must_use]
    pub fn with_label(self, name: &str) -> Self {
        self.push(Folder::labeled(name, name));
        self
    }

    /// Replaces the labels of the message of that UID in `name`; its
    /// mod-sequence moves past the folder's, as a label change does on
    /// Gmail.
    pub fn relabel(&self, name: &str, uid: u32, labels: &[&str]) {
        self.edit(name, |folder| {
            let modseq = folder.highest_modseq + 1;
            if let Some(message) = folder.mail.iter_mut().find(|message| message.uid == uid) {
                message.labels = labels.iter().map(|label| (*label).to_owned()).collect();
                message.modseq = modseq;
            }
        });
    }

    /// Moves the message of that UID from one store to another, as
    /// marking it spam or trashing it does: a new UID in the target, the
    /// same X-GM-MSGID and X-GM-THRID, the Inbox label gone.
    pub fn move_to(&self, (from, uid): (&str, u32), to: &str) {
        let mut moved = None;
        self.edit(from, |folder| {
            if let Some(index) = folder.mail.iter().position(|message| message.uid == uid) {
                moved = Some(folder.mail.remove(index));
            }
        });
        let Some(message) = moved else {
            return;
        };
        self.edit(to, |folder| {
            let labels = message
                .labels
                .iter()
                .filter(|label| !label.eq_ignore_ascii_case(INBOX_LABEL))
                .cloned()
                .collect();
            folder.mail.push(Message {
                uid: folder.uid_next,
                modseq: folder.highest_modseq + 1,
                labels,
                ..message
            });
        });
    }

    /// The counts STATUS answers for a label folder while the extension
    /// is on: the messages of All Mail under its label and the unseen
    /// ones among them. `None` for a folder with mail of its own.
    pub(super) fn label_counts(&self, folder: &Folder) -> Option<(u32, u32)> {
        let label = folder
            .label
            .as_deref()
            .filter(|_| self.has(Extension::Gmail))?;
        let folders = self.folders();
        let all_mail = folders
            .iter()
            .find(|folder| folder.special_use == Some(ALL))?;
        let labeled: Vec<&Message> = all_mail
            .mail
            .iter()
            .filter(|message| message.labels.iter().any(|found| found == label))
            .collect();
        let unseen = labeled
            .iter()
            .filter(|message| !message.flags.iter().any(|flag| flag == "\\Seen"))
            .count();
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        Some((count(labeled.len()), count(unseen)))
    }
}
