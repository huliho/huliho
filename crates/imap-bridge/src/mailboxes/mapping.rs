// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! From LIST and STATUS lines to mailbox facts: roles, names, the
//! hierarchy and the sort order, without a connection.

use std::collections::{HashMap, HashSet};

use crate::session::{ListEntry, StatusEntry};
use crate::store::MailboxFacts;
use crate::utf7;

/// The roles a mailbox can carry (RFC 8621 section 2: the IANA mailbox
/// name attributes with the backslash removed, lowercased); the first
/// six sort in front of the tree in this order.
pub const ROLES: [&str; 9] = [
    "inbox",
    "drafts",
    "sent",
    "archive",
    "junk",
    "trash",
    "all",
    "flagged",
    "important",
];

/// How many of `ROLES` sort in front; the rest share one position.
const PINNED: usize = 6;

/// The sort order of every mailbox outside the pinned roles; clients
/// sort equal values by name (RFC 8621 section 2).
const SORT_ORDER_REST: u32 = 6;

/// The attribute that names each role (RFC 6154 section 2, RFC 8457),
/// in the order that decides between two on one mailbox.
const ATTRIBUTE_ROLES: [(&str, &str); 8] = [
    ("\\ALL", "all"),
    ("\\ARCHIVE", "archive"),
    ("\\DRAFTS", "drafts"),
    ("\\FLAGGED", "flagged"),
    ("\\IMPORTANT", "important"),
    ("\\JUNK", "junk"),
    ("\\SENT", "sent"),
    ("\\TRASH", "trash"),
];

/// The names servers without attributes tend to use, compared without
/// regard to case against the leaf of the name.
const NAMED_ROLES: [(&str, &str); 13] = [
    ("Drafts", "drafts"),
    ("Sent", "sent"),
    ("Sent Items", "sent"),
    ("Sent Messages", "sent"),
    ("Junk", "junk"),
    ("Spam", "junk"),
    ("Junk E-mail", "junk"),
    ("Junk Email", "junk"),
    ("Trash", "trash"),
    ("Deleted Items", "trash"),
    ("Deleted Messages", "trash"),
    ("Archive", "archive"),
    ("Archives", "archive"),
];

/// The one name RFC 3501 section 5.1 fixes, in any case.
const INBOX: &str = "INBOX";
const NOSELECT: &str = "\\NOSELECT";
const NONEXISTENT: &str = "\\NONEXISTENT";
const SUBSCRIBED: &str = "\\SUBSCRIBED";

/// Where the subscriptions come from: the `\Subscribed` attribute of an
/// extended LIST or the names LSUB answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subscriptions {
    Attributes,
    Lsub(HashSet<String>),
}

/// Turns a listing into mailbox facts: one role per mailbox and one
/// mailbox per role, the hierarchy from the delimiter, the counts from
/// STATUS. The answer keeps the order of the listing.
#[must_use]
pub fn map(
    entries: &[ListEntry],
    statuses: &[StatusEntry],
    subscriptions: &Subscriptions,
) -> Vec<MailboxFacts> {
    let context = Context {
        statuses: statuses
            .iter()
            .map(|status| (status.mailbox.as_str(), status))
            .collect(),
        names: entries.iter().map(|entry| entry.name.as_str()).collect(),
        roles: assign_roles(entries),
        subscriptions,
    };
    entries.iter().map(|entry| facts(entry, &context)).collect()
}

struct Context<'a> {
    statuses: HashMap<&'a str, &'a StatusEntry>,
    names: HashSet<&'a str>,
    roles: HashMap<&'a str, &'static str>,
    subscriptions: &'a Subscriptions,
}

fn facts(entry: &ListEntry, context: &Context<'_>) -> MailboxFacts {
    let selectable = is_selectable(&entry.attributes);
    let status = context.statuses.get(entry.name.as_str());
    let subscribed = match context.subscriptions {
        Subscriptions::Attributes => entry
            .attributes
            .iter()
            .any(|attribute| attribute == SUBSCRIBED),
        Subscriptions::Lsub(names) => names.contains(&entry.name),
    };
    let (parent, leaf) = split(&entry.name, entry.delimiter);
    let role = context.roles.get(entry.name.as_str()).copied();
    MailboxFacts {
        name: utf7::decode(leaf),
        imap_name: entry.name.clone(),
        parent_imap_name: parent
            .filter(|parent| context.names.contains(parent))
            .map(str::to_owned),
        role: role.map(str::to_owned),
        sort_order: sort_order(role),
        subscribed,
        selectable,
        store: selectable,
        gmail_label: None,
        uid_validity: status.and_then(|status| status.uid_validity),
        uid_next: status.and_then(|status| status.uid_next),
        highest_modseq: status.and_then(|status| status.highest_modseq),
        total_emails: status.and_then(|status| status.messages).unwrap_or(0),
        unread_emails: status.and_then(|status| status.unseen).unwrap_or(0),
    }
}

/// Whether the mailbox can be selected: neither `\Noselect` (RFC 3501
/// section 7.2.2) nor `\NonExistent` (RFC 5258 section 3.1).
pub(super) fn is_selectable(attributes: &[String]) -> bool {
    !attributes
        .iter()
        .any(|attribute| attribute == NOSELECT || attribute == NONEXISTENT)
}

/// The parent's wire name and the leaf; a name without a delimiter
/// inside it is its own leaf at the top.
fn split(name: &str, delimiter: Option<char>) -> (Option<&str>, &str) {
    match delimiter.and_then(|delimiter| name.rsplit_once(delimiter)) {
        Some((parent, leaf)) if !parent.is_empty() && !leaf.is_empty() => (Some(parent), leaf),
        _ => (None, name),
    }
}

/// Where a role sits in the tree: the pinned six in their order, every
/// other mailbox at `SORT_ORDER_REST`.
fn sort_order(role: Option<&str>) -> u32 {
    role.and_then(|role| ROLES[..PINNED].iter().position(|pinned| *pinned == role))
        .and_then(|position| u32::try_from(position).ok())
        .unwrap_or(SORT_ORDER_REST)
}

/// A claim on a role, ordered the way a tie is broken: an attribute
/// before a name, then the smaller depth, then the decoded name without
/// regard to case, then the wire name.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct Claim<'a> {
    by_name: bool,
    depth: usize,
    folded: String,
    name: &'a str,
    role: &'static str,
}

/// One role per mailbox and one mailbox per role.
fn assign_roles(entries: &[ListEntry]) -> HashMap<&str, &'static str> {
    let mut claims: Vec<Claim<'_>> = entries.iter().filter_map(claim).collect();
    claims.sort();
    let mut taken = HashSet::new();
    let mut assigned = HashMap::new();
    for claim in claims {
        if taken.insert(claim.role) {
            assigned.insert(claim.name, claim.role);
        }
    }
    assigned
}

/// The role an entry claims: INBOX by its full name, then the
/// attributes in their fixed order, then the name table on the leaf. A
/// mailbox nothing can select claims nothing.
fn claim(entry: &ListEntry) -> Option<Claim<'_>> {
    if !is_selectable(&entry.attributes) {
        return None;
    }
    let (_, leaf) = split(&entry.name, entry.delimiter);
    let leaf = utf7::decode(leaf);
    let by_attribute = ATTRIBUTE_ROLES
        .iter()
        .find(|(attribute, _)| entry.attributes.iter().any(|found| found == attribute));
    let (role, by_name) = if entry.name.eq_ignore_ascii_case(INBOX) {
        ("inbox", false)
    } else if let Some((_, role)) = by_attribute {
        (*role, false)
    } else {
        let (_, role) = NAMED_ROLES
            .iter()
            .find(|(name, _)| leaf.eq_ignore_ascii_case(name))?;
        (*role, true)
    };
    Some(Claim {
        by_name,
        depth: entry
            .delimiter
            .map_or(0, |delimiter| entry.name.matches(delimiter).count()),
        folded: leaf.to_lowercase(),
        name: &entry.name,
        role,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_splits_at_its_last_delimiter_or_not_at_all() {
        assert_eq!(split("a/b/c", Some('/')), (Some("a/b"), "c"));
        assert_eq!(split("INBOX", Some('/')), (None, "INBOX"));
        assert_eq!(split("a/b", None), (None, "a/b"));
        assert_eq!(split("a/", Some('/')), (None, "a/"));
        assert_eq!(split("/a", Some('/')), (None, "/a"));
    }

    #[test]
    fn the_pinned_roles_take_their_position_and_the_rest_one_value() {
        assert_eq!(sort_order(Some("inbox")), 0);
        assert_eq!(sort_order(Some("trash")), 5);
        assert_eq!(sort_order(Some("flagged")), SORT_ORDER_REST);
        assert_eq!(sort_order(None), SORT_ORDER_REST);
        assert_eq!(usize::try_from(SORT_ORDER_REST).unwrap(), PINNED);
    }

    #[test]
    fn claims_order_attribute_first_then_depth_then_name() {
        let entries = [
            ListEntry {
                name: "INBOX/Sent".to_owned(),
                delimiter: Some('/'),
                attributes: Vec::new(),
            },
            ListEntry {
                name: "Sent Messages".to_owned(),
                delimiter: Some('/'),
                attributes: Vec::new(),
            },
            ListEntry {
                name: "Outbox".to_owned(),
                delimiter: Some('/'),
                attributes: vec!["\\SENT".to_owned()],
            },
        ];
        let mut claims: Vec<Claim<'_>> = entries.iter().filter_map(claim).collect();
        claims.sort();
        let names: Vec<&str> = claims.iter().map(|claim| claim.name).collect();
        assert_eq!(names, ["Outbox", "Sent Messages", "INBOX/Sent"]);
    }
}
