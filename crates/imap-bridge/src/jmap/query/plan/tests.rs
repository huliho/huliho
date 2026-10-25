// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The folders a scope searches on both kinds of account and the coarse
//! days, as examples and as a property over generated trees.

use std::collections::{HashMap, HashSet};

use proptest::prelude::*;
use time::Month;

use super::*;
use crate::session::Moving;
use crate::store::{MailboxFacts, MailboxId};

/// A row in tree order with a role, a label and a UIDVALIDITY.
fn row(order: u32, name: &str, role: Option<&str>, label: Option<&str>) -> MailboxRow {
    MailboxRow {
        id: MailboxId::from(format!("m-{name}")),
        parent_id: None,
        facts: MailboxFacts {
            name: name.to_owned(),
            imap_name: name.to_owned(),
            parent_imap_name: None,
            role: role.map(str::to_owned),
            sort_order: order,
            subscribed: true,
            selectable: true,
            store: label.is_none(),
            gmail_label: label.map(str::to_owned),
            uid_validity: Some(order + 1),
            uid_next: None,
            highest_modseq: None,
            total_emails: 0,
            unread_emails: 0,
        },
    }
}

fn snapshot(rows: Vec<MailboxRow>) -> MailboxSnapshot {
    let gmail = rows.iter().any(|row| row.facts.gmail_label.is_some());
    MailboxSnapshot {
        state: 1,
        rows,
        counts: HashMap::new(),
        done: HashSet::new(),
        gmail,
        moving: Moving::default(),
    }
}

/// A Dovecot tree with a folder below Archive.
fn dovecot() -> MailboxSnapshot {
    let mut rows = vec![
        row(0, "INBOX", Some("inbox"), None),
        row(1, "Drafts", Some("drafts"), None),
        row(2, "Sent", Some("sent"), None),
        row(3, "Archive", Some("archive"), None),
        row(4, "Junk", Some("junk"), None),
        row(5, "Trash", Some("trash"), None),
        row(6, "Receipts", None, None),
    ];
    rows[1].facts.selectable = false;
    snapshot(rows)
}

/// A Gmail tree: three stores and four labels.
fn gmail() -> MailboxSnapshot {
    snapshot(vec![
        row(0, "INBOX", Some("inbox"), Some("\\Inbox")),
        row(1, "[Gmail]/Starred", Some("flagged"), Some("\\Flagged")),
        row(2, "[Gmail]/All Mail", Some("archive"), None),
        row(3, "[Gmail]/Spam", Some("junk"), None),
        row(4, "[Gmail]/Trash", Some("trash"), None),
        row(5, "Receipts", None, Some("Receipts")),
    ])
}

fn names(plan: &Plan) -> Vec<(&str, Option<&str>)> {
    plan.folders
        .iter()
        .map(|folder| (folder.imap_name.as_str(), folder.label.as_deref()))
        .collect()
}

fn filter(in_mailbox: Option<&str>, other_than: Option<&[&str]>) -> Filter {
    Filter {
        in_mailbox: in_mailbox.map(str::to_owned),
        other_than: other_than.map(|ids| ids.iter().map(|id| (*id).to_owned()).collect()),
        text: "zoë".to_owned(),
        ..Filter::default()
    }
}

const BINS: &[&str] = &["m-Junk", "m-Trash"];
const GMAIL_BINS: &[&str] = &["m-[Gmail]/Spam", "m-[Gmail]/Trash"];

#[test]
fn a_folder_account_searches_the_folders_of_the_scope_with_junk_and_trash_last() {
    let tree = dovecot();
    let default = plan(&filter(None, Some(BINS)), &tree);
    assert_eq!(
        names(&default),
        [
            ("INBOX", None),
            ("Sent", None),
            ("Archive", None),
            ("Receipts", None)
        ]
    );
    let all = plan(&filter(None, None), &tree);
    let last: Vec<_> = names(&all).into_iter().rev().take(2).collect();
    assert_eq!(last, [("Trash", None), ("Junk", None)]);
    assert_eq!(
        names(&plan(&filter(Some("m-Junk"), None), &tree)),
        [("Junk", None)]
    );
    for nothing in [Some("m-nope"), Some("m-Drafts")] {
        assert!(plan(&filter(nothing, None), &tree).folders.is_empty());
    }
    assert!(!default.gmail);
}

#[test]
fn gmail_searches_all_mail_with_a_label_and_spam_or_trash_where_the_scope_keeps_them() {
    let tree = gmail();
    assert_eq!(
        names(&plan(&filter(None, Some(GMAIL_BINS)), &tree)),
        [("[Gmail]/All Mail", None)]
    );
    assert_eq!(
        names(&plan(&filter(None, None), &tree)),
        [
            ("[Gmail]/All Mail", None),
            ("[Gmail]/Spam", None),
            ("[Gmail]/Trash", None)
        ]
    );
    let all_mail = "m-[Gmail]/All Mail";
    assert_eq!(
        names(&plan(&filter(None, Some(&[all_mail, "m-INBOX"])), &tree)),
        [
            ("[Gmail]/All Mail", None),
            ("[Gmail]/Spam", None),
            ("[Gmail]/Trash", None)
        ]
    );
    for (mailbox, expected) in [
        ("m-INBOX", ("[Gmail]/All Mail", Some("\\Inbox"))),
        ("m-[Gmail]/Starred", ("[Gmail]/All Mail", Some("\\Starred"))),
        ("m-Receipts", ("[Gmail]/All Mail", Some("Receipts"))),
        (all_mail, ("[Gmail]/All Mail", None)),
        ("m-[Gmail]/Spam", ("[Gmail]/Spam", None)),
    ] {
        let found = plan(&filter(Some(mailbox), None), &tree);
        assert_eq!(names(&found), [expected], "{mailbox}");
        assert!(found.gmail);
    }
}

#[test]
fn the_keywords_and_the_days_become_the_coarse_keys_a_day_wider_each_side() {
    let day = |year, month, day| Date::from_calendar_date(year, month, day).ok();
    let midnight = 1_790_812_800;
    let found = plan(
        &Filter {
            not_keyword: Some("$seen".to_owned()),
            has_keyword: Some("$flagged".to_owned()),
            after: Some(midnight),
            before: Some(midnight),
            ..filter(None, None)
        },
        &dovecot(),
    );
    assert!(found.unseen && found.flagged);
    assert_eq!(found.since, day(2026, Month::September, 30));
    assert_eq!(found.before, day(2026, Month::October, 2));
    let noon = midnight + 12 * 3600;
    assert_eq!(super::before(noon), day(2026, Month::October, 3));
    assert_eq!(super::since(i64::MIN), None);
    assert_eq!(super::before(253_402_300_799), None);
    let other = plan(
        &Filter {
            has_keyword: Some("$seen".to_owned()),
            ..filter(None, None)
        },
        &dovecot(),
    );
    assert!(!other.unseen && !other.flagged);
}

/// A tree of folders with random roles, some not selectable, some
/// without a UIDVALIDITY.
fn trees() -> impl Strategy<Value = Vec<MailboxRow>> {
    let role = prop_oneof![
        Just(None),
        Just(Some("inbox")),
        Just(Some("junk")),
        Just(Some("trash")),
        Just(Some("archive")),
    ];
    prop::collection::vec((role, any::<bool>(), any::<bool>()), 1..10).prop_map(|folders| {
        folders
            .into_iter()
            .enumerate()
            .map(|(order, (role, selectable, validity))| {
                let order = u32::try_from(order).unwrap();
                let mut found = row(order, &format!("f{order}"), role, None);
                found.facts.selectable = selectable;
                found.facts.uid_validity = found.facts.uid_validity.filter(|_| validity);
                found
            })
            .collect()
    })
}

proptest! {
    #[test]
    fn a_folder_account_searches_every_folder_of_the_scope_once_and_the_bins_last(
        rows in trees(),
        left_out in prop::collection::vec(0..10_usize, 0..4),
        named in prop::option::of(0..12_usize),
    ) {
        let ids: Vec<String> = rows.iter().map(|row| row.id.to_string()).collect();
        let other_than: Vec<String> = left_out.iter().filter_map(|index| ids.get(*index).cloned()).collect();
        let in_mailbox = named.map(|index| ids.get(index).cloned().unwrap_or_else(|| "m-nope".to_owned()));
        let asked = Filter {
            in_mailbox: in_mailbox.clone(),
            other_than: Some(other_than.clone()),
            ..filter(None, None)
        };
        let found = plan(&asked, &snapshot(rows.clone()));
        let searched: Vec<&str> = found.folders.iter().map(|folder| folder.id.as_str()).collect();
        let expected: HashSet<&str> = rows.iter()
            .filter(|row| row.facts.selectable && row.facts.uid_validity.is_some())
            .map(|row| row.id.as_str())
            .filter(|id| match &in_mailbox {
                Some(named) => id == named,
                None => !other_than.iter().any(|left| left == id),
            })
            .collect();
        prop_assert_eq!(searched.iter().copied().collect::<HashSet<_>>(), expected);
        prop_assert_eq!(searched.len(), found.folders.iter().map(|folder| &folder.id).collect::<HashSet<_>>().len());
        let role = |id: &str| rows.iter().find(|row| row.id.as_str() == id).and_then(|row| row.facts.role.as_deref());
        let bin = |id: &str| role(id).is_some_and(|role| LAST_ROLES.contains(&role));
        let first_bin = searched.iter().position(|id| bin(id)).unwrap_or(searched.len());
        prop_assert!(searched[first_bin..].iter().all(|id| bin(id)), "{searched:?}");
        let order = |id: &str| rows.iter().position(|row| row.id.as_str() == id);
        prop_assert!(searched[..first_bin].windows(2).all(|pair| order(pair[0]) < order(pair[1])));
    }
}
