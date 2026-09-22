// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The Gmail label model: three folders are stores, every other folder
//! shows the messages that carry its label (X-GM-LABELS), one message
//! is one row (X-GM-MSGID) and a thread is the server's (X-GM-THRID).
//! The rules here need no connection and no database.

use crate::session::{Capabilities, ListEntry};

/// The capability that carries the three FETCH items.
pub const CAPABILITY: &str = "X-GM-EXT-1";

/// The role the `\All` folder carries: archiving removes the Inbox label
/// and nothing else.
pub const ALL_MAIL_ROLE: &str = "archive";

/// The attributes of the folders whose messages the sync fetches; every
/// other folder is a label over the first of them.
const STORE_ATTRIBUTES: [&str; 3] = ["\\ALL", "\\JUNK", "\\TRASH"];

/// The label a folder with a special-use attribute shows.
const LABELED_ATTRIBUTES: [(&str, &str); 4] = [
    ("\\SENT", "\\Sent"),
    ("\\DRAFTS", "\\Drafts"),
    ("\\FLAGGED", "\\Flagged"),
    ("\\IMPORTANT", "\\Important"),
];

/// The label of the INBOX, the one folder named rather than marked.
const INBOX: &str = "INBOX";
const INBOX_LABEL: &str = "\\Inbox";

/// Every spelling a server uses for a system label and the one the
/// bridge keeps; Gmail writes the drafts label both ways.
const SYSTEM_LABELS: [(&str, &str); 7] = [
    ("\\Inbox", INBOX_LABEL),
    ("\\Sent", "\\Sent"),
    ("\\Drafts", "\\Drafts"),
    ("\\Draft", "\\Drafts"),
    ("\\Flagged", "\\Flagged"),
    ("\\Starred", "\\Flagged"),
    ("\\Important", "\\Important"),
];

/// Whether the host's word on the account holds for this server: the
/// capability confirms it.
#[must_use]
pub fn confirmed(gmail: bool, capabilities: &Capabilities) -> bool {
    gmail && capabilities.has(CAPABILITY)
}

/// Whether a selectable entry is a store: All Mail, Spam or Trash.
#[must_use]
pub fn is_store(attributes: &[String]) -> bool {
    attributes
        .iter()
        .any(|attribute| STORE_ATTRIBUTES.contains(&attribute.as_str()))
}

/// The role an attribute claims on Gmail: `\All` is the archive, since
/// no `\Archive` folder exists there.
#[must_use]
pub fn role(attribute_role: &'static str) -> &'static str {
    if attribute_role == "all" {
        ALL_MAIL_ROLE
    } else {
        attribute_role
    }
}

/// The label a selectable entry that is no store shows: the INBOX's, the
/// one its attribute names, else its own wire name, which is how a user
/// label travels in X-GM-LABELS.
#[must_use]
pub fn label_of(entry: &ListEntry) -> String {
    if entry.name.eq_ignore_ascii_case(INBOX) {
        return INBOX_LABEL.to_owned();
    }
    LABELED_ATTRIBUTES
        .iter()
        .find(|(attribute, _)| entry.attributes.iter().any(|found| found == attribute))
        .map_or_else(|| entry.name.clone(), |(_, label)| (*label).to_owned())
}

/// A label as the server sent it, in the bridge's spelling: a system
/// label in any case and either spelling folds to one, a user label
/// stays as it is.
#[must_use]
pub fn canonical(label: &str) -> String {
    if !label.starts_with('\\') {
        return label.to_owned();
    }
    SYSTEM_LABELS
        .iter()
        .find(|(spelling, _)| label.eq_ignore_ascii_case(spelling))
        .map_or_else(|| label.to_owned(), |(_, folded)| (*folded).to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, attributes: &[&str]) -> ListEntry {
        ListEntry {
            name: name.to_owned(),
            delimiter: Some('/'),
            attributes: attributes.iter().map(|word| (*word).to_owned()).collect(),
        }
    }

    #[test]
    fn the_three_stores_are_told_by_their_attributes() {
        for attribute in ["\\ALL", "\\JUNK", "\\TRASH"] {
            assert!(is_store(&[attribute.to_owned()]), "{attribute}");
        }
        for attribute in [
            "\\SENT",
            "\\DRAFTS",
            "\\FLAGGED",
            "\\IMPORTANT",
            "\\HASNOCHILDREN",
        ] {
            assert!(!is_store(&[attribute.to_owned()]), "{attribute}");
        }
        assert!(!is_store(&[]));
    }

    #[test]
    fn a_label_folder_shows_the_inbox_the_system_label_or_its_own_name() {
        assert_eq!(label_of(&entry("inbox", &[])), "\\Inbox");
        assert_eq!(
            label_of(&entry("[Gmail]/Sent Mail", &["\\HASNOCHILDREN", "\\SENT"])),
            "\\Sent"
        );
        assert_eq!(
            label_of(&entry("[Gmail]/Starred", &["\\FLAGGED"])),
            "\\Flagged"
        );
        assert_eq!(label_of(&entry("Work/Q3", &["\\HASNOCHILDREN"])), "Work/Q3");
        assert_eq!(label_of(&entry("&U,BTFw-", &[])), "&U,BTFw-");
    }

    #[test]
    fn both_spellings_of_a_system_label_fold_to_one_in_any_case() {
        assert_eq!(canonical("\\Draft"), "\\Drafts");
        assert_eq!(canonical("\\drafts"), "\\Drafts");
        assert_eq!(canonical("\\Starred"), "\\Flagged");
        assert_eq!(canonical("\\INBOX"), "\\Inbox");
        assert_eq!(canonical("\\Trash"), "\\Trash");
        assert_eq!(canonical("Work"), "Work");
        assert_eq!(canonical("inbox"), "inbox");
    }

    #[test]
    fn all_mail_claims_the_archive_role_and_every_other_role_stands() {
        assert_eq!(role("all"), "archive");
        assert_eq!(role("junk"), "junk");
        let none: Capabilities = Capabilities::default();
        assert!(!confirmed(true, &none));
        let gmail: Capabilities = ["x-gm-ext-1".to_owned()].into_iter().collect();
        assert!(confirmed(true, &gmail));
        assert!(!confirmed(false, &gmail));
    }
}
