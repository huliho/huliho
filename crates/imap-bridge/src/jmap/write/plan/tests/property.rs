// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What a mailbox edit plans over drawn rows and edits: a folder account
//! ends in one folder, in two it is too many and in none the paths the
//! edit wrote are named back; a Gmail account never names a store as a
//! label and leaves Spam or Trash before it labels.

use proptest::prelude::*;
use proptest::test_runner::TestCaseError;

use super::super::*;
use super::{mailbox, row, rows};

/// The folders of a folder account the properties draw from: the four
/// roles and a user folder.
const FOLDERS: [(&str, Option<&str>); 5] = [
    ("inbox", Some("inbox")),
    ("archive", Some("archive")),
    ("junk", Some("junk")),
    ("trash", Some("trash")),
    ("work", None),
];

/// The mailboxes of a Gmail account the properties draw from: the three
/// stores, All Mail first, then the labels.
const GMAIL: [(&str, Option<&str>, Option<&str>); 6] = [
    ("all", Some("archive"), None),
    ("spam", Some("junk"), None),
    ("trash", Some("trash"), None),
    ("inbox", Some("inbox"), Some("\\Inbox")),
    ("starred", Some("flagged"), Some("\\Flagged")),
    ("work", None, Some("Work")),
];

/// How many of `GMAIL` are stores.
const STORES: usize = 3;

/// The edits each property draws: enough to reach every arm of the plan
/// over these sets.
const CASES: u32 = 128;

/// The rows of either account the properties plan against.
fn drawn(gmail: bool) -> HashMap<MailboxId, MailboxRow> {
    if gmail {
        rows(&GMAIL.map(|(id, role, label)| mailbox(id, (role, label.is_none(), label))))
    } else {
        rows(&FOLDERS.map(|(id, role)| mailbox(id, (role, true, None))))
    }
}

/// A row of a folder account, in one of its folders.
fn folder_row() -> impl Strategy<Value = KeywordRow> {
    (0..FOLDERS.len()).prop_map(|index| {
        let id = FOLDERS[index].0;
        row(id, &[id])
    })
}

/// A row of a Gmail account: in All Mail under some of the labels, or in
/// Spam or Trash alone.
fn gmail_row() -> impl Strategy<Value = KeywordRow> {
    let labeled = prop::collection::btree_set(STORES..GMAIL.len(), 0..=GMAIL.len() - STORES)
        .prop_map(|labels| {
            let mut members = vec![GMAIL[0].0];
            members.extend(labels.into_iter().map(|index| GMAIL[index].0));
            row(GMAIL[0].0, &members)
        });
    // The stores after All Mail.
    let stored = (1..STORES).prop_map(|index| {
        let id = GMAIL[index].0;
        row(id, &[id])
    });
    prop_oneof![labeled, stored]
}

/// An edit over these mailboxes: a path per mailbox it enters or leaves,
/// or a whole object naming the mailboxes the email is left in.
fn edit_over(names: Vec<&'static str>) -> impl Strategy<Value = MailboxEdit> {
    let kept = names.clone();
    let paths =
        prop::collection::vec(any::<Option<bool>>(), names.len()).prop_map(move |touches| {
            let mut edit = MailboxEdit::default();
            for (name, touch) in names.iter().zip(touches) {
                let list = match touch {
                    Some(true) => &mut edit.add,
                    Some(false) => &mut edit.remove,
                    None => continue,
                };
                list.push((*name).to_owned());
                edit.named
                    .push(((*name).to_owned(), format!("mailboxIds/{name}")));
            }
            edit
        });
    let whole = prop::collection::btree_set(0..kept.len(), 1..=kept.len()).prop_map(move |left| {
        MailboxEdit {
            add: left
                .into_iter()
                .map(|index| kept[index].to_owned())
                .collect(),
            whole: true,
            ..MailboxEdit::default()
        }
    });
    prop_oneof![paths, whole]
}

/// Where the email would lie if the edit were done as written.
fn would_lie(row: &KeywordRow, edit: &MailboxEdit) -> BTreeSet<MailboxId> {
    let patch = Patch {
        mailboxes: Some(edit.clone()),
        ..Patch::default()
    };
    expect(row, &patch).mailboxes
}

/// Whether a refusal names paths the edit wrote and nothing else.
fn under_written_paths(paths: &[String], edit: &MailboxEdit) -> bool {
    let written: BTreeSet<&str> = if edit.whole {
        BTreeSet::from(["mailboxIds"])
    } else {
        edit.named.iter().map(|(_, path)| path.as_str()).collect()
    };
    !paths.is_empty() && paths.iter().all(|path| written.contains(path.as_str()))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(CASES))]

    #[test]
    fn a_folder_account_plans_one_folder_refuses_two_as_too_many_and_none_under_the_paths_written(
        row in folder_row(),
        edit in edit_over(FOLDERS.iter().map(|(id, _)| *id).collect()),
    ) {
        let plan = mailboxes(&row, &edit, &drawn(false), false);
        let left = would_lie(&row, &edit);
        match &plan {
            Ok(Plan::Move { to, then }) => {
                prop_assert!(then.is_none() && *to != row.folder, "{plan:?}");
                prop_assert_eq!(&left, &BTreeSet::from([to.clone()]));
            }
            Ok(Plan::Nothing) => prop_assert_eq!(&left, &BTreeSet::from([row.folder.clone()])),
            Err(SetError::TooManyMailboxes) => prop_assert!(left.len() > 1, "{left:?}"),
            Err(SetError::InvalidProperties(paths)) => {
                prop_assert!(left.is_empty(), "{left:?}");
                prop_assert!(under_written_paths(paths, &edit), "{plan:?}");
            }
            Ok(Plan::Labels(_)) | Err(_) => return Err(TestCaseError::fail(format!("{plan:?}"))),
        }
    }

    #[test]
    fn a_gmail_account_names_no_store_as_a_label_and_leaves_spam_or_trash_before_it_labels(
        row in gmail_row(),
        edit in edit_over(GMAIL.iter().map(|(id, _, _)| *id).collect()),
    ) {
        let rows = drawn(true);
        let store = |id: &MailboxId| rows[id].facts.store;
        let all_mail = MailboxId::from(GMAIL[0].0.to_owned());
        let in_store = row.folder != all_mail;
        let entered: BTreeSet<MailboxId> = edit
            .add
            .iter()
            .map(|id| MailboxId::from(id.clone()))
            .filter(|id| !store(id) && !row.mailboxes.contains(id))
            .collect();
        let plan = mailboxes(&row, &edit, &rows, true);
        match &plan {
            Ok(Plan::Labels(relabel)) => {
                prop_assert!(!in_store && *relabel != Relabel::default(), "{plan:?}");
                prop_assert!(!relabel.add.iter().chain(&relabel.remove).any(store), "{plan:?}");
                prop_assert_eq!(&relabel.add.iter().cloned().collect::<BTreeSet<_>>(), &entered);
            }
            Ok(Plan::Move { to, then }) => {
                prop_assert!(store(to) && *to != row.folder, "{plan:?}");
                prop_assert_eq!(then.is_some(), *to == all_mail, "{:?}", plan);
                match then {
                    Some(relabel) => {
                        prop_assert!(!relabel.add.iter().chain(&relabel.remove).any(store));
                        prop_assert_eq!(&relabel.add.iter().cloned().collect::<BTreeSet<_>>(), &entered);
                    }
                    None => prop_assert!(entered.is_empty(), "{plan:?}"),
                }
            }
            Ok(Plan::Nothing) => prop_assert!(entered.is_empty(), "{plan:?}"),
            Err(SetError::InvalidProperties(paths)) => {
                prop_assert!(under_written_paths(paths, &edit), "{plan:?}");
            }
            Err(_) => return Err(TestCaseError::fail(format!("{plan:?}"))),
        }
    }
}
