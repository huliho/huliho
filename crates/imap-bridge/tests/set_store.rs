// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What one `Email/set` writes to the rows as one state: keywords, with
//! the Starred membership of a Gmail row following `$flagged`; labels,
//! with `$flagged` following the Starred membership; moves under their
//! new UIDs with the id kept; rows whose message left; a row a move took
//! where the write cannot follow it, which waits for the pass on Gmail
//! and leaves elsewhere; and nothing where a row is not where the server
//! found its message.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use huliho_imap_bridge::store::{
    AccountKey, Advance, Batch, ChangeKind, EmailFacts, EmailId, GmailFacts, GoneRow,
    KeywordChange, LabelChange, MailboxFacts, MailboxId, MoveChange, ObjectType, Personal,
    SetWrite, Store, Synced,
};
use huliho_imap_bridge::testing::seal::TestSealer;

/// The UIDVALIDITY of every store folder here.
const UIDVALIDITY: u32 = 7;

fn account() -> AccountKey {
    AccountKey::new("a1")
}

/// A store folder with a role, or a Gmail label mailbox.
fn mailbox(name: &str, role: Option<&str>, label: Option<&str>) -> MailboxFacts {
    MailboxFacts {
        role: role.map(str::to_owned),
        store: label.is_none(),
        gmail_label: label.map(str::to_owned),
        uid_validity: label.is_none().then_some(UIDVALIDITY),
        name: name.to_owned(),
        imap_name: name.to_owned(),
        parent_imap_name: None,
        sort_order: 0,
        subscribed: true,
        selectable: true,
        uid_next: None,
        highest_modseq: None,
        total_emails: 0,
        unread_emails: 0,
    }
}

/// An account listed in a fresh store, its mailboxes by name.
fn listed(found: &[MailboxFacts]) -> (Store, HashMap<String, MailboxId>) {
    let store = Store::in_memory().unwrap();
    store.apply_mailboxes(&account(), found).unwrap();
    let snapshot = store.mailbox_snapshot(&account()).unwrap();
    let ids = snapshot
        .rows
        .into_iter()
        .map(|row| (row.facts.name, row.id))
        .collect();
    (store, ids)
}

/// A Gmail account: the three stores and three labels.
fn gmail() -> (Store, HashMap<String, MailboxId>) {
    listed(&[
        mailbox("All", Some("archive"), None),
        mailbox("Spam", Some("junk"), None),
        mailbox("Trash", Some("trash"), None),
        mailbox("Inbox", Some("inbox"), Some("\\Inbox")),
        mailbox("Starred", Some("flagged"), Some("\\Flagged")),
        mailbox("Work", None, Some("Work")),
    ])
}

/// A message arrives in a folder under a UID, with its labels on Gmail;
/// its email id comes back.
fn arrive(store: &Store, folder: &MailboxId, uid: u32, labels: Option<&[&str]>) -> EmailId {
    let before = store.state(&account()).unwrap();
    let facts = EmailFacts {
        uid,
        keywords: BTreeMap::new(),
        size: 10,
        received_at: 5_000 + i64::from(uid),
        sent_at: None,
        has_attachment: false,
        gmail: labels.map(|labels| GmailFacts {
            labels: labels.iter().map(|label| (*label).to_owned()).collect(),
            msgid: 900 + u64::from(uid),
            thrid: 900 + u64::from(uid),
        }),
        personal: Personal::default(),
    };
    let batch = Batch {
        folder,
        uid_validity: UIDVALIDITY,
        emails: &[facts],
        advance: Advance::Down {
            lowest_uid: Some(uid),
            done: false,
            synced: Synced::default(),
        },
    };
    store
        .apply_batch(&account(), &batch, &TestSealer::default())
        .unwrap()
        .unwrap();
    let created = logged(store, ObjectType::Email, before);
    EmailId::from(created[0].0.clone())
}

fn logged(store: &Store, object: ObjectType, since: u64) -> Vec<(String, ChangeKind)> {
    let since = store.changes_since(&account(), object, since).unwrap();
    let changes = since.changes.unwrap().into_iter();
    changes.map(|change| (change.id, change.kind)).collect()
}

/// The names of the mailboxes logged as updated since a state.
fn mailboxes_updated(
    store: &Store,
    ids: &HashMap<String, MailboxId>,
    since: u64,
) -> BTreeSet<String> {
    let names: HashMap<&str, &String> = ids.iter().map(|(name, id)| (id.as_str(), name)).collect();
    logged(store, ObjectType::Mailbox, since)
        .into_iter()
        .map(|(id, _)| names[id.as_str()].clone())
        .collect()
}

/// The names of the mailboxes the email is a member of.
fn members(store: &Store, ids: &HashMap<String, MailboxId>, email: &EmailId) -> BTreeSet<String> {
    let (_, rows) = store.keyword_rows(&account(), &[email.as_str()]).unwrap();
    let names: HashMap<&MailboxId, &String> = ids.iter().map(|(name, id)| (id, name)).collect();
    rows[0]
        .mailboxes
        .iter()
        .map(|id| names[id].clone())
        .collect()
}

fn named(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

fn keywords(store: &Store, email: &EmailId) -> Vec<String> {
    let (_, rows) = store.keyword_rows(&account(), &[email.as_str()]).unwrap();
    rows[0].keywords.keys().cloned().collect()
}

#[test]
fn a_keyword_write_is_one_state_and_a_starred_message_of_all_mail_joins_the_flagged_mailbox() {
    let (store, ids) = gmail();
    let id = arrive(&store, &ids["All"], 1, Some(&["\\Inbox", "Work"]));
    let flag = |uid: u32, add: &[&str], remove: &[&str]| KeywordChange {
        id: id.clone(),
        folder: ids["All"].clone(),
        uid,
        add: add.iter().map(|keyword| (*keyword).to_owned()).collect(),
        remove: remove.iter().map(|keyword| (*keyword).to_owned()).collect(),
    };
    let state = store.state(&account()).unwrap();
    let write = SetWrite {
        keywords: vec![flag(1, &["$flagged"], &[])],
        ..SetWrite::default()
    };
    let written = store.write_set(&account(), &write).unwrap();
    assert_eq!((written.before, written.after), (state, state + 1));
    assert!(written.stale.is_empty(), "{:?}", written.stale);
    assert_eq!(
        members(&store, &ids, &id),
        named(&["All", "Inbox", "Starred", "Work"])
    );
    assert_eq!(
        mailboxes_updated(&store, &ids, state),
        named(&["All", "Inbox", "Starred", "Work"])
    );
    assert_eq!(
        logged(&store, ObjectType::Email, state),
        [(id.to_string(), ChangeKind::Updated)]
    );
    let as_written = &written.emails[&id];
    assert!(as_written.mailboxes.contains(&ids["Starred"]));
    assert_eq!(as_written.keywords.keys().collect::<Vec<_>>(), ["$flagged"]);
    let cleared = SetWrite {
        keywords: vec![flag(1, &[], &["$flagged"])],
        ..SetWrite::default()
    };
    store.write_set(&account(), &cleared).unwrap();
    assert_eq!(members(&store, &ids, &id), named(&["All", "Inbox", "Work"]));
    // The same keywords again move no state; a row under another UID by
    // then is left as it is and named; another account writes nothing.
    let after = store.state(&account()).unwrap();
    let again = SetWrite {
        keywords: vec![flag(1, &[], &["$flagged"]), flag(2, &["x"], &[])],
        ..SetWrite::default()
    };
    let written = store.write_set(&account(), &again).unwrap();
    assert_eq!((written.before, written.after), (after, after));
    assert_eq!(written.stale, std::slice::from_ref(&id));
    let foreign = store
        .write_set(
            &AccountKey::new("a2"),
            &SetWrite {
                keywords: vec![flag(1, &["x"], &[])],
                ..SetWrite::default()
            },
        )
        .unwrap();
    assert_eq!((foreign.after, foreign.stale), (0, vec![id.clone()]));
    let kept = keywords(&store, &id);
    assert!(kept.is_empty(), "{kept:?}");
}

#[test]
fn labels_move_memberships_and_the_flagged_mailbox_stars_the_message() {
    let (store, ids) = gmail();
    let id = arrive(&store, &ids["All"], 1, Some(&["\\Inbox"]));
    let state = store.state(&account()).unwrap();
    let relabel = |add: &[&str], remove: &[&str]| LabelChange {
        id: id.clone(),
        folder: ids["All"].clone(),
        uid: 1,
        add: add.iter().map(|name| ids[*name].clone()).collect(),
        remove: remove.iter().map(|name| ids[*name].clone()).collect(),
    };
    let write = SetWrite {
        labels: vec![relabel(&["Work", "Starred"], &["Inbox"])],
        ..SetWrite::default()
    };
    let written = store.write_set(&account(), &write).unwrap();
    assert_eq!(written.after, state + 1);
    assert_eq!(
        members(&store, &ids, &id),
        named(&["All", "Starred", "Work"])
    );
    assert_eq!(keywords(&store, &id), ["$flagged"]);
    assert_eq!(
        mailboxes_updated(&store, &ids, state),
        named(&["Inbox", "Starred", "Work"])
    );
    let unstar = SetWrite {
        labels: vec![relabel(&[], &["Starred"])],
        ..SetWrite::default()
    };
    store.write_set(&account(), &unstar).unwrap();
    let kept = keywords(&store, &id);
    assert!(kept.is_empty(), "{kept:?}");
    assert_eq!(members(&store, &ids, &id), named(&["All", "Work"]));
}

/// A move of the email from where it lies to a folder under a UID.
fn moving(
    id: &EmailId,
    (from, from_uid): (&MailboxId, u32),
    (to, uid): (&MailboxId, u32),
) -> MoveChange {
    MoveChange {
        id: id.clone(),
        from: from.clone(),
        from_uid,
        to: to.clone(),
        uid_validity: UIDVALIDITY,
        uid,
        labels: None,
    }
}

#[test]
fn a_move_takes_the_row_to_its_new_folder_and_uid_and_keeps_the_id() {
    let (store, ids) = gmail();
    let id = arrive(&store, &ids["All"], 1, Some(&["\\Inbox", "Work"]));
    let state = store.state(&account()).unwrap();
    let to_trash = SetWrite {
        moves: vec![moving(&id, (&ids["All"], 1), (&ids["Trash"], 9))],
        ..SetWrite::default()
    };
    let written = store.write_set(&account(), &to_trash).unwrap();
    assert_eq!(written.after, state + 1);
    assert_eq!(members(&store, &ids, &id), named(&["Trash"]));
    let (_, rows) = store.keyword_rows(&account(), &[id.as_str()]).unwrap();
    assert_eq!((&rows[0].folder, rows[0].uid), (&ids["Trash"], Some(9)));
    assert_eq!(
        logged(&store, ObjectType::Email, state),
        [(id.to_string(), ChangeKind::Updated)]
    );
    assert!(
        logged(&store, ObjectType::Thread, state).is_empty(),
        "the thread is the same"
    );
    assert_eq!(
        mailboxes_updated(&store, &ids, state),
        named(&["All", "Inbox", "Trash", "Work"])
    );
    let restore = MoveChange {
        labels: Some(vec!["\\Inbox".to_owned(), "Work".to_owned()]),
        ..moving(&id, (&ids["Trash"], 9), (&ids["All"], 12))
    };
    let written = store
        .write_set(
            &account(),
            &SetWrite {
                moves: vec![restore],
                ..SetWrite::default()
            },
        )
        .unwrap();
    assert_eq!(members(&store, &ids, &id), named(&["All", "Inbox", "Work"]));
    assert_eq!(written.emails[&id].mailboxes.len(), 3);
}

#[test]
fn a_folder_account_moves_the_row_and_its_one_membership() {
    let (store, ids) = listed(&[
        mailbox("INBOX", Some("inbox"), None),
        mailbox("Archive", Some("archive"), None),
    ]);
    let id = arrive(&store, &ids["INBOX"], 4, None);
    let archive = SetWrite {
        moves: vec![moving(&id, (&ids["INBOX"], 4), (&ids["Archive"], 1))],
        ..SetWrite::default()
    };
    store.write_set(&account(), &archive).unwrap();
    assert_eq!(members(&store, &ids, &id), named(&["Archive"]));
}

#[test]
fn a_move_the_target_cannot_take_parks_the_row_on_gmail_and_destroys_it_elsewhere() {
    let (store, ids) = gmail();
    let renumbered = arrive(&store, &ids["All"], 1, Some(&["\\Inbox"]));
    let crowded = arrive(&store, &ids["All"], 2, Some(&["\\Inbox"]));
    arrive(&store, &ids["Trash"], 5, Some(&[]));
    let state = store.state(&account()).unwrap();
    let write = SetWrite {
        gmail: true,
        moves: vec![
            MoveChange {
                uid_validity: UIDVALIDITY + 1,
                ..moving(&renumbered, (&ids["All"], 1), (&ids["Trash"], 6))
            },
            moving(&crowded, (&ids["All"], 2), (&ids["Trash"], 5)),
        ],
        ..SetWrite::default()
    };
    let written = store.write_set(&account(), &write).unwrap();
    let outcome = (written.after, written.stale.len(), written.emails.len());
    assert_eq!(outcome, (state, 0, 0));
    let emails = logged(&store, ObjectType::Email, state);
    assert!(emails.is_empty(), "{emails:?}");
    for parked in [&renumbered, &crowded] {
        let (_, rows) = store.keyword_rows(&account(), &[parked.as_str()]).unwrap();
        assert_eq!(
            (&rows[0].folder, rows[0].uid),
            (&ids["All"], None),
            "{parked}"
        );
    }
    // The message turns up in Trash and the parked row keeps its id.
    let claimed = arrive(&store, &ids["Trash"], 1, Some(&[]));
    assert_eq!(claimed, renumbered);
    assert_eq!(members(&store, &ids, &renumbered), named(&["Trash"]));
    let (store, ids) = listed(&[
        mailbox("INBOX", Some("inbox"), None),
        mailbox("Archive", Some("archive"), None),
    ]);
    let moved = arrive(&store, &ids["INBOX"], 4, None);
    arrive(&store, &ids["Archive"], 1, None);
    let state = store.state(&account()).unwrap();
    let crowded = SetWrite {
        moves: vec![moving(&moved, (&ids["INBOX"], 4), (&ids["Archive"], 1))],
        ..SetWrite::default()
    };
    store.write_set(&account(), &crowded).unwrap();
    assert_eq!(
        logged(&store, ObjectType::Email, state),
        [(moved.to_string(), ChangeKind::Destroyed)]
    );
}

#[test]
fn a_row_the_sync_already_put_where_the_move_took_it_landed() {
    let (store, ids) = gmail();
    let id = arrive(&store, &ids["Trash"], 3, Some(&[]));
    let state = store.state(&account()).unwrap();
    let write = SetWrite {
        moves: vec![moving(&id, (&ids["All"], 1), (&ids["Trash"], 3))],
        ..SetWrite::default()
    };
    let written = store.write_set(&account(), &write).unwrap();
    assert_eq!((written.after, written.stale.len()), (state, 0));
    assert_eq!(
        written.emails[&id].mailboxes,
        BTreeSet::from([ids["Trash"].clone()])
    );
}

#[test]
fn a_gone_row_is_destroyed_with_its_thread_and_every_mailbox_it_was_in() {
    let (store, ids) = gmail();
    let id = arrive(&store, &ids["Trash"], 3, Some(&[]));
    let state = store.state(&account()).unwrap();
    let gone = GoneRow {
        id: id.clone(),
        folder: ids["Trash"].clone(),
        uid: 3,
    };
    let write = SetWrite {
        gone: vec![gone.clone()],
        ..SetWrite::default()
    };
    let written = store.write_set(&account(), &write).unwrap();
    assert_eq!(written.after, state + 1);
    assert_eq!(
        logged(&store, ObjectType::Email, state),
        [(id.to_string(), ChangeKind::Destroyed)]
    );
    assert_eq!(
        logged(&store, ObjectType::Thread, state),
        [("t903".to_owned(), ChangeKind::Destroyed)]
    );
    assert_eq!(mailboxes_updated(&store, &ids, state), named(&["Trash"]));
    let again = store.write_set(&account(), &write).unwrap();
    assert_eq!(again.after, state + 1, "a row gone already moves no state");
}
