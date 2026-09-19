// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The mailbox mapping without a connection: roles, names, hierarchy
//! and sort order over LIST and STATUS lines, plus the name codec.

use std::collections::HashSet;

use huliho_imap_bridge::mailboxes::{ROLES, Subscriptions, map};
use huliho_imap_bridge::session::{ListEntry, StatusEntry};
use huliho_imap_bridge::store::MailboxFacts;
use huliho_imap_bridge::utf7;
use proptest::prelude::*;

fn entry(name: &str, attributes: &[&str]) -> ListEntry {
    ListEntry {
        name: name.to_owned(),
        delimiter: Some('/'),
        attributes: attributes
            .iter()
            .map(|attribute| (*attribute).to_owned())
            .collect(),
    }
}

fn mapped(entries: &[ListEntry]) -> Vec<MailboxFacts> {
    map(entries, &[], &Subscriptions::Attributes)
}

fn role_of<'a>(facts: &'a [MailboxFacts], imap_name: &str) -> Option<&'a str> {
    facts
        .iter()
        .find(|facts| facts.imap_name == imap_name)
        .and_then(|facts| facts.role.as_deref())
}

#[test]
fn inbox_is_a_role_by_its_name_in_any_case_rfc3501_5_1() {
    let facts = mapped(&[entry("inbox", &[])]);
    assert_eq!(role_of(&facts, "inbox"), Some("inbox"));
    assert_eq!(facts[0].sort_order, 0);
    assert_eq!(facts[0].name, "inbox");
}

#[test]
fn roles_from_attributes_beat_roles_from_names_rfc6154_2() {
    let facts = mapped(&[entry("Sent Items", &[]), entry("Outbox", &["\\SENT"])]);
    assert_eq!(role_of(&facts, "Outbox"), Some("sent"));
    assert_eq!(role_of(&facts, "Sent Items"), None);
    let important = mapped(&[entry("Priority", &["\\IMPORTANT", "\\HASNOCHILDREN"])]);
    assert_eq!(role_of(&important, "Priority"), Some("important"));
}

#[test]
fn one_mailbox_per_role_and_the_first_in_the_trees_order_wins() {
    let facts = mapped(&[
        entry("Sent Messages", &[]),
        entry("Sent", &[]),
        entry("INBOX/Sent", &[]),
    ]);
    assert_eq!(role_of(&facts, "Sent"), Some("sent"));
    assert_eq!(role_of(&facts, "Sent Messages"), None);
    assert_eq!(role_of(&facts, "INBOX/Sent"), None);
    let deep = mapped(&[entry("INBOX/Trash", &[]), entry("INBOX/Deleted Items", &[])]);
    assert_eq!(role_of(&deep, "INBOX/Deleted Items"), Some("trash"));
    assert_eq!(role_of(&deep, "INBOX/Trash"), None);
}

#[test]
fn the_name_table_matches_the_leaf_without_regard_to_case() {
    let facts = mapped(&[
        entry("INBOX", &[]),
        entry("INBOX/junk e-mail", &[]),
        entry("ARCHIVES", &[]),
    ]);
    assert_eq!(role_of(&facts, "INBOX/junk e-mail"), Some("junk"));
    assert_eq!(role_of(&facts, "ARCHIVES"), Some("archive"));
}

#[test]
fn a_mailbox_nothing_can_select_claims_no_role_and_holds_no_mail() {
    let facts = mapped(&[
        entry("Trash", &["\\NOSELECT", "\\HASCHILDREN"]),
        entry("Trash/Old", &[]),
        entry("Ghost", &["\\NONEXISTENT"]),
    ]);
    assert_eq!(role_of(&facts, "Trash"), None);
    assert!(!facts[0].selectable);
    assert!(!facts[0].store);
    assert!(!facts[2].selectable);
    assert!(!facts[2].store);
    assert_eq!(facts[1].parent_imap_name.as_deref(), Some("Trash"));
    assert_eq!(facts[1].name, "Old");
}

#[test]
fn a_parent_the_server_did_not_list_leaves_the_mailbox_at_the_top() {
    let facts = mapped(&[entry("Projects/Huliho", &[])]);
    assert_eq!(facts[0].parent_imap_name, None);
    assert_eq!(facts[0].name, "Huliho");
    let flat = map(
        &[ListEntry {
            name: "a/b".to_owned(),
            delimiter: None,
            attributes: Vec::new(),
        }],
        &[],
        &Subscriptions::Attributes,
    );
    assert_eq!(flat[0].name, "a/b");
}

#[test]
fn the_pinned_roles_sort_first_in_their_order_and_the_rest_share_one_value() {
    let facts = mapped(&[
        entry("INBOX", &[]),
        entry("Drafts", &[]),
        entry("Sent", &[]),
        entry("Archive", &[]),
        entry("Junk", &[]),
        entry("Trash", &[]),
        entry("Starred", &["\\FLAGGED"]),
        entry("Work", &[]),
    ]);
    let orders: Vec<u32> = facts.iter().map(|facts| facts.sort_order).collect();
    assert_eq!(orders, [0, 1, 2, 3, 4, 5, 6, 6]);
    assert_eq!(
        ROLES[..6],
        ["inbox", "drafts", "sent", "archive", "junk", "trash"]
    );
}

#[test]
fn counts_follow_status_and_a_missing_line_reads_zero() {
    let statuses = [StatusEntry {
        mailbox: "INBOX".to_owned(),
        messages: Some(17),
        unseen: Some(3),
        uid_next: Some(18),
        uid_validity: Some(5),
        highest_modseq: Some(9),
    }];
    let facts = map(
        &[entry("INBOX", &[]), entry("Work", &[])],
        &statuses,
        &Subscriptions::Attributes,
    );
    assert_eq!((facts[0].total_emails, facts[0].unread_emails), (17, 3));
    assert_eq!(
        (
            facts[0].uid_next,
            facts[0].uid_validity,
            facts[0].highest_modseq
        ),
        (Some(18), Some(5), Some(9))
    );
    assert_eq!(
        (
            facts[1].total_emails,
            facts[1].unread_emails,
            facts[1].uid_next
        ),
        (0, 0, None)
    );
}

#[test]
fn subscriptions_come_from_the_attribute_or_from_lsub_rfc5258_3_1() {
    let entries = [entry("INBOX", &["\\SUBSCRIBED"]), entry("Work", &[])];
    let by_attribute = map(&entries, &[], &Subscriptions::Attributes);
    assert_eq!(
        (by_attribute[0].subscribed, by_attribute[1].subscribed),
        (true, false)
    );
    let lsub: HashSet<String> = ["Work".to_owned()].into_iter().collect();
    let by_lsub = map(&entries, &[], &Subscriptions::Lsub(lsub));
    assert_eq!(
        (by_lsub[0].subscribed, by_lsub[1].subscribed),
        (false, true)
    );
}

#[test]
fn names_decode_from_modified_utf7_and_the_wire_name_stays_rfc3501_5_1_3() {
    let facts = mapped(&[entry("&U,BTFw-", &[]), entry("&U,BTFw-/&ZeVnLIqe-", &[])]);
    assert_eq!(facts[0].name, "台北");
    assert_eq!(facts[1].name, "日本語");
    assert_eq!(facts[1].parent_imap_name.as_deref(), Some("&U,BTFw-"));
    assert_eq!(facts[1].imap_name, "&U,BTFw-/&ZeVnLIqe-");
}

proptest! {
    /// Any delimiter, any depth: the parent is the prefix and the leaf
    /// the last segment.
    #[test]
    fn the_hierarchy_holds_for_any_delimiter(
        delimiter in prop::sample::select(vec!['/', '.', '|', '^', '~']),
        segments in prop::collection::vec("[a-z]{1,8}", 1..4),
    ) {
        let joiner = delimiter.to_string();
        let entries: Vec<ListEntry> = (1..=segments.len())
            .map(|depth| ListEntry {
                name: segments[..depth].join(&joiner),
                delimiter: Some(delimiter),
                attributes: Vec::new(),
            })
            .collect();
        let facts = map(&entries, &[], &Subscriptions::Attributes);
        let last = facts.last().unwrap();
        prop_assert_eq!(&last.imap_name, &segments.join(&joiner));
        prop_assert_eq!(&last.name, segments.last().unwrap());
        let parent = (segments.len() > 1).then(|| segments[..segments.len() - 1].join(&joiner));
        prop_assert_eq!(last.parent_imap_name.clone(), parent);
    }

    /// Whatever the listing, every role appears at most once, every role
    /// word is one of the nine and `inbox` goes where INBOX is listed.
    #[test]
    fn every_role_is_taken_at_most_once(
        names in prop::collection::hash_set(
            prop::sample::select(vec![
                "INBOX", "Sent", "Sent Items", "Drafts", "Trash", "Deleted Items",
                "Junk", "Spam", "Archive", "Work", "Old",
            ]),
            1..8,
        ),
    ) {
        let entries: Vec<ListEntry> = names.iter().map(|name| entry(name, &[])).collect();
        let facts = map(&entries, &[], &Subscriptions::Attributes);
        let roles: Vec<&str> = facts.iter().filter_map(|facts| facts.role.as_deref()).collect();
        let distinct: HashSet<&str> = roles.iter().copied().collect();
        prop_assert_eq!(roles.len(), distinct.len());
        prop_assert!(roles.iter().all(|role| ROLES.contains(role)));
        prop_assert_eq!(roles.contains(&"inbox"), names.contains("INBOX"));
    }

    /// The codec survives any name and every wire name is ASCII.
    #[test]
    fn utf7_names_decode_and_re_encode(name in ".*") {
        let wire = utf7::encode(&name);
        prop_assert!(wire.is_ascii());
        prop_assert_eq!(utf7::decode(&wire), name);
    }
}
