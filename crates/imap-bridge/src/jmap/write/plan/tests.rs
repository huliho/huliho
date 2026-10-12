// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What a mailbox edit plans on a folder account and on a Gmail
//! account, read against fixed rows; `property` draws rows and edits.

use std::collections::BTreeMap;

use super::*;
use crate::jmap::set::read_patch;
use crate::session::MAX_WIRE_NAME_BYTES;
use crate::store::MailboxFacts;

mod property;

/// A mailbox row under its own id: a store folder, a Gmail label or a
/// placeholder nothing can select.
fn mailbox(id: &str, (role, store, label): (Option<&str>, bool, Option<&str>)) -> MailboxRow {
    MailboxRow {
        id: MailboxId::from(id.to_owned()),
        parent_id: None,
        facts: MailboxFacts {
            name: id.to_owned(),
            imap_name: id.to_owned(),
            parent_imap_name: None,
            role: role.map(str::to_owned),
            sort_order: 0,
            subscribed: true,
            selectable: id != "noselect",
            store,
            gmail_label: label.map(str::to_owned),
            uid_validity: Some(1),
            uid_next: Some(1),
            highest_modseq: None,
            total_emails: 0,
            unread_emails: 0,
        },
    }
}

fn rows(all: &[MailboxRow]) -> HashMap<MailboxId, MailboxRow> {
    all.iter()
        .map(|row| (row.id.clone(), row.clone()))
        .collect()
}

/// Inbox, Archive, Trash and a placeholder on a folder account.
fn folders() -> HashMap<MailboxId, MailboxRow> {
    rows(&[
        mailbox("inbox", (Some("inbox"), true, None)),
        mailbox("archive", (Some("archive"), true, None)),
        mailbox("trash", (Some("trash"), true, None)),
        mailbox("noselect", (None, false, None)),
    ])
}

/// The three stores and two labels of a Gmail account.
fn gmail() -> HashMap<MailboxId, MailboxRow> {
    rows(&[
        mailbox("all", (Some("archive"), true, None)),
        mailbox("spam", (Some("junk"), true, None)),
        mailbox("trash", (Some("trash"), true, None)),
        mailbox("inbox", (Some("inbox"), false, Some("\\Inbox"))),
        mailbox("work", (None, false, Some("Work"))),
    ])
}

fn row(folder: &str, mailboxes: &[&str]) -> KeywordRow {
    KeywordRow {
        id: crate::store::EmailId::from("e1".to_owned()),
        folder: MailboxId::from(folder.to_owned()),
        uid: Some(7),
        keywords: BTreeMap::from([("$seen".to_owned(), true)]),
        mailboxes: mailboxes
            .iter()
            .map(|id| MailboxId::from((*id).to_owned()))
            .collect(),
    }
}

/// An edit read from a patch.
fn edit(patch: &serde_json::Value) -> MailboxEdit {
    read_patch(patch).unwrap().mailboxes.unwrap()
}

fn ids(names: &[&str]) -> Vec<MailboxId> {
    names
        .iter()
        .map(|id| MailboxId::from((*id).to_owned()))
        .collect()
}

fn moving(to: &str, then: Option<(&[&str], &[&str])>) -> Plan {
    Plan::Move {
        to: MailboxId::from(to.to_owned()),
        then: then.map(|(add, remove)| Relabel {
            add: ids(add),
            remove: ids(remove),
        }),
    }
}

fn paths(names: &[&str]) -> SetError {
    SetError::InvalidProperties(names.iter().map(|path| (*path).to_owned()).collect())
}

#[test]
fn a_folder_account_moves_a_message_from_one_folder_to_another_rfc8621_4_6() {
    let (rows, inbox) = (folders(), row("inbox", &["inbox"]));
    let archive =
        edit(&serde_json::json!({ "mailboxIds/inbox": null, "mailboxIds/archive": true }));
    assert_eq!(
        mailboxes(&inbox, &archive, &rows, false),
        Ok(moving("archive", None))
    );
    let whole = edit(&serde_json::json!({ "mailboxIds": { "trash": true } }));
    assert_eq!(
        mailboxes(&inbox, &whole, &rows, false),
        Ok(moving("trash", None))
    );
    let same = edit(&serde_json::json!({ "mailboxIds": { "inbox": true } }));
    assert_eq!(mailboxes(&inbox, &same, &rows, false), Ok(Plan::Nothing));
    let absent = edit(&serde_json::json!({ "mailboxIds/trash": null }));
    assert_eq!(mailboxes(&inbox, &absent, &rows, false), Ok(Plan::Nothing));
}

#[test]
fn a_folder_account_holds_a_message_in_one_folder_and_never_in_none_rfc8621_1_3_1() {
    let (rows, inbox) = (folders(), row("inbox", &["inbox"]));
    let added = edit(&serde_json::json!({ "mailboxIds/archive": true }));
    assert_eq!(
        mailboxes(&inbox, &added, &rows, false),
        Err(SetError::TooManyMailboxes)
    );
    let two = edit(&serde_json::json!({ "mailboxIds": { "archive": true, "trash": true } }));
    assert_eq!(
        mailboxes(&inbox, &two, &rows, false),
        Err(SetError::TooManyMailboxes)
    );
    let emptied = edit(&serde_json::json!({ "mailboxIds/inbox": null }));
    assert_eq!(
        mailboxes(&inbox, &emptied, &rows, false),
        Err(paths(&["mailboxIds/inbox"]))
    );
}

#[test]
fn a_mailbox_the_account_does_not_hold_or_that_holds_no_mail_is_named_back_rfc8620_5_3() {
    let (rows, inbox) = (folders(), row("inbox", &["inbox"]));
    let unknown = edit(&serde_json::json!({ "mailboxIds/inbox": null, "mailboxIds/m-none": true }));
    assert_eq!(
        mailboxes(&inbox, &unknown, &rows, false),
        Err(paths(&["mailboxIds/m-none"]))
    );
    let placeholder = edit(&serde_json::json!({ "mailboxIds": { "noselect": true } }));
    assert_eq!(
        mailboxes(&inbox, &placeholder, &rows, false),
        Err(paths(&["mailboxIds"]))
    );
}

#[test]
fn gmail_archives_and_labels_on_the_store_the_row_lies_in() {
    let rows = gmail();
    let inboxed = row("all", &["all", "inbox"]);
    let archive = edit(&serde_json::json!({ "mailboxIds/inbox": null, "mailboxIds/all": true }));
    let plan = mailboxes(&inboxed, &archive, &rows, true);
    let relabel = Relabel {
        add: Vec::new(),
        remove: ids(&["inbox"]),
    };
    assert_eq!(plan, Ok(Plan::Labels(relabel)));
    let label = edit(&serde_json::json!({ "mailboxIds/work": true }));
    let relabel = Relabel {
        add: ids(&["work"]),
        remove: Vec::new(),
    };
    assert_eq!(
        mailboxes(&inboxed, &label, &rows, true),
        Ok(Plan::Labels(relabel))
    );
    let leave = edit(&serde_json::json!({ "mailboxIds/all": null }));
    assert_eq!(
        mailboxes(&inboxed, &leave, &rows, true),
        Err(paths(&["mailboxIds/all"]))
    );
}

#[test]
fn gmail_enters_spam_or_trash_by_a_move_and_leaves_them_for_all_mail() {
    let rows = gmail();
    let inboxed = row("all", &["all", "inbox"]);
    let trash = edit(&serde_json::json!({ "mailboxIds/inbox": null, "mailboxIds/trash": true }));
    assert_eq!(
        mailboxes(&inboxed, &trash, &rows, true),
        Ok(moving("trash", None))
    );
    let labeled = edit(&serde_json::json!({ "mailboxIds/trash": true, "mailboxIds/work": true }));
    assert_eq!(
        mailboxes(&inboxed, &labeled, &rows, true),
        Err(paths(&["mailboxIds/work"]))
    );
    let both = edit(&serde_json::json!({ "mailboxIds/trash": true, "mailboxIds/spam": true }));
    assert_eq!(
        mailboxes(&inboxed, &both, &rows, true),
        Err(paths(&["mailboxIds/spam", "mailboxIds/trash"]))
    );
    let junked = row("spam", &["spam"]);
    let not_spam = edit(&serde_json::json!({ "mailboxIds/spam": null, "mailboxIds/inbox": true }));
    assert_eq!(
        mailboxes(&junked, &not_spam, &rows, true),
        Ok(moving("all", Some((&["inbox"], &[]))))
    );
    let restore = edit(&serde_json::json!({ "mailboxIds/spam": null, "mailboxIds/all": true }));
    assert_eq!(
        mailboxes(&junked, &restore, &rows, true),
        Ok(moving("all", Some((&[], &[]))))
    );
    let to_trash = edit(&serde_json::json!({ "mailboxIds/spam": null, "mailboxIds/trash": true }));
    assert_eq!(
        mailboxes(&junked, &to_trash, &rows, true),
        Ok(moving("trash", None))
    );
    let emptied = edit(&serde_json::json!({ "mailboxIds/spam": null }));
    assert_eq!(
        mailboxes(&junked, &emptied, &rows, true),
        Err(paths(&["mailboxIds/spam"]))
    );
    let stays = edit(&serde_json::json!({ "mailboxIds/work": true }));
    assert_eq!(
        mailboxes(&junked, &stays, &rows, true),
        Err(paths(&["mailboxIds/work"]))
    );
}

#[test]
fn gmail_fails_an_update_whose_label_no_store_run_can_carry_before_anything_is_sent() {
    let wide = "w".repeat(MAX_WIRE_NAME_BYTES);
    let mut all = gmail();
    all.extend(rows(&[mailbox("wide", (None, false, Some(&wide)))]));
    let inboxed = row("all", &["all", "inbox"]);
    let label = edit(&serde_json::json!({ "mailboxIds/wide": true }));
    assert_eq!(
        mailboxes(&inboxed, &label, &all, true),
        Err(SetError::ServerFail)
    );
    let junked = row("spam", &["spam"]);
    let restore = edit(&serde_json::json!({ "mailboxIds/spam": null, "mailboxIds/wide": true }));
    assert_eq!(
        mailboxes(&junked, &restore, &all, true),
        Err(SetError::ServerFail)
    );
    let fitting = edit(&serde_json::json!({ "mailboxIds/work": true }));
    assert!(mailboxes(&inboxed, &fitting, &all, true).is_ok());
}

#[test]
fn the_expected_email_is_the_row_with_the_patch_applied_and_nothing_more() {
    let inboxed = row("all", &["all", "inbox"]);
    let read = read_patch(&serde_json::json!({
        "mailboxIds/inbox": null,
        "mailboxIds/trash": true,
        "keywords/$flagged": true,
    }))
    .unwrap();
    let expected = expect(&inboxed, &read);
    assert_eq!(
        expected.mailboxes,
        ids(&["all", "trash"]).into_iter().collect()
    );
    assert_eq!(
        expected.keywords.keys().collect::<Vec<_>>(),
        ["$flagged", "$seen"]
    );
    let whole = read_patch(&serde_json::json!({
        "mailboxIds": { "work": true },
        "keywords": {},
    }))
    .unwrap();
    let expected = expect(&inboxed, &whole);
    assert_eq!(expected.mailboxes, ids(&["work"]).into_iter().collect());
    assert!(expected.keywords.is_empty());
}
