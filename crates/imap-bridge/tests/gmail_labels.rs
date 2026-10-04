// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The label model at the store: any label set on a row yields exactly
//! those mailboxes and back, a row in Spam carries its store alone, a
//! row whose UID left waits for another store to claim it, a message
//! without its items is stored as a folder account stores it and a
//! keyword written to a row updates every mailbox it is a member of.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use huliho_imap_bridge::store::{
    AccountKey, Advance, Batch, ChangeKind, EmailFacts, EmailId, FlagChange, GmailFacts,
    KeywordChange, MailboxFacts, MailboxId, ObjectType, Personal, Standing, Store, Synced,
};
use huliho_imap_bridge::testing::seal::TestSealer;
use proptest::prelude::*;

const SEALER: TestSealer = TestSealer { locked: false };

const ALL_MAIL: &str = "[Gmail]/All Mail";
const SPAM: &str = "[Gmail]/Spam";

/// Every label mailbox of the account, by the label its rows carry.
const LABELS: [&str; 5] = ["\\Inbox", "\\Sent", "\\Flagged", "Work", "Work/Q3"];

/// The X-GM-MSGID of the one message most cases hold.
const MSGID: u64 = 1_278_455_344_230_334_865;

/// An id past 63 bits, which the column holds as the signed value of
/// the same bits.
const HIGH_MSGID: u64 = u64::MAX - 7;

fn key() -> AccountKey {
    AccountKey::new("a1")
}

fn folder(
    imap_name: &str,
    (role, store, label): (Option<&str>, bool, Option<&str>),
) -> MailboxFacts {
    MailboxFacts {
        name: imap_name.to_owned(),
        imap_name: imap_name.to_owned(),
        parent_imap_name: None,
        role: role.map(str::to_owned),
        sort_order: 6,
        subscribed: true,
        selectable: true,
        store,
        gmail_label: label.map(str::to_owned),
        uid_validity: Some(1),
        uid_next: Some(1),
        highest_modseq: None,
        total_emails: 0,
        unread_emails: 0,
    }
}

/// A Gmail account listed: the two stores and a label mailbox per label,
/// with the ids by wire name.
fn listed() -> (Store, HashMap<String, MailboxId>) {
    let store = Store::in_memory().unwrap();
    let mut found = vec![
        folder(ALL_MAIL, (Some("archive"), true, None)),
        folder(SPAM, (Some("junk"), true, None)),
    ];
    found.extend(
        LABELS
            .iter()
            .map(|label| folder(label, (None, false, Some(label)))),
    );
    store.apply_mailboxes(&key(), &found).unwrap();
    let ids = store
        .mailbox_snapshot(&key())
        .unwrap()
        .rows
        .into_iter()
        .map(|row| (row.facts.imap_name, row.id))
        .collect();
    (store, ids)
}

fn facts(uid: u32, labels: &[&str], msgid: u64) -> EmailFacts {
    EmailFacts {
        uid,
        keywords: BTreeMap::from([("$seen".to_owned(), true)]),
        size: 100,
        received_at: 1_000,
        sent_at: None,
        has_attachment: false,
        gmail: Some(GmailFacts {
            labels: labels.iter().map(|label| (*label).to_owned()).collect(),
            msgid,
            thrid: 9,
        }),
        personal: Personal {
            subject: Some(format!("Message {uid}")),
            ..Personal::default()
        },
    }
}

/// One batch that leaves the folder done.
fn write(store: &Store, folder: &MailboxId, emails: &[EmailFacts]) -> u64 {
    let batch = Batch {
        folder,
        uid_validity: 1,
        emails,
        advance: Advance::Down {
            lowest_uid: emails.iter().map(|email| email.uid).min(),
            done: true,
            synced: Synced::default(),
        },
    };
    store.apply_batch(&key(), &batch, &SEALER).unwrap().unwrap()
}

fn logged(store: &Store, object: ObjectType, since: u64) -> Vec<(String, ChangeKind)> {
    store
        .changes_since(&key(), object, since)
        .unwrap()
        .changes
        .unwrap()
        .into_iter()
        .map(|change| (change.id, change.kind))
        .collect()
}

/// The one email the log names as created since a state.
fn created(store: &Store, since: u64) -> EmailId {
    let mut found: Vec<EmailId> = logged(store, ObjectType::Email, since)
        .into_iter()
        .filter(|(_, kind)| *kind == ChangeKind::Created)
        .map(|(id, _)| EmailId::from(id))
        .collect();
    assert_eq!(found.len(), 1, "one email created");
    found.remove(0)
}

/// The wire names of the mailboxes an email is a member of.
fn memberships(store: &Store, ids: &HashMap<String, MailboxId>, id: &EmailId) -> BTreeSet<String> {
    let names: HashMap<&MailboxId, &String> = ids.iter().map(|(name, id)| (id, name)).collect();
    let rows = store.emails(&key(), &[id.as_str()]).unwrap().rows;
    rows[0]
        .mailbox_ids
        .iter()
        .map(|id| names[id].clone())
        .collect()
}

fn with_store(store: &str, labels: &[&str]) -> BTreeSet<String> {
    std::iter::once(store)
        .chain(labels.iter().copied())
        .map(str::to_owned)
        .collect()
}

fn subset() -> impl Strategy<Value = Vec<&'static str>> {
    prop::collection::btree_set(0..LABELS.len(), 0..=LABELS.len())
        .prop_map(|indexes| indexes.into_iter().map(|index| LABELS[index]).collect())
}

proptest! {
    #[test]
    fn any_label_set_on_a_row_yields_exactly_those_mailboxes_and_back(
        first in subset(),
        second in subset(),
    ) {
        let (store, ids) = listed();
        let all_mail = &ids[ALL_MAIL];
        let state = write(&store, all_mail, &[facts(1, &first, MSGID)]);
        let id = created(&store, 0);
        prop_assert_eq!(memberships(&store, &ids, &id), with_store(ALL_MAIL, &first));
        let standing = Standing {
            folder: all_mail,
            uid_validity: 1,
        };
        let change = FlagChange {
            uid: 1,
            keywords: facts(1, &[], MSGID).keywords,
            labels: Some(second.iter().map(|label| (*label).to_owned()).collect()),
        };
        store.apply_flags(&key(), &standing, &[change]).unwrap().unwrap();
        prop_assert_eq!(memberships(&store, &ids, &id), with_store(ALL_MAIL, &second));
        let emails = logged(&store, ObjectType::Email, state);
        let mailboxes: BTreeSet<String> = logged(&store, ObjectType::Mailbox, state)
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        let moved: BTreeSet<&str> = first
            .iter()
            .chain(second.iter())
            .filter(|label| first.contains(label) != second.contains(label))
            .copied()
            .collect();
        if moved.is_empty() {
            prop_assert!(emails.is_empty(), "nothing moved, so no state");
        } else {
            prop_assert_eq!(emails, vec![(id.to_string(), ChangeKind::Updated)]);
            for label in moved {
                prop_assert!(mailboxes.contains(ids[label].as_str()), "{label}");
            }
            prop_assert!(mailboxes.contains(all_mail.as_str()));
        }
    }
}

#[test]
fn a_row_in_spam_carries_its_store_alone_whatever_its_labels() {
    let (store, ids) = listed();
    write(&store, &ids[SPAM], &[facts(1, &["\\Inbox", "Work"], MSGID)]);
    let id = created(&store, 0);
    assert_eq!(memberships(&store, &ids, &id), with_store(SPAM, &[]));
}

#[test]
fn a_label_the_listing_does_not_name_maps_to_nothing() {
    let (store, ids) = listed();
    let labels = ["\\Inbox", "Nowhere", "\\Trash"];
    write(&store, &ids[ALL_MAIL], &[facts(1, &labels, MSGID)]);
    let id = created(&store, 0);
    assert_eq!(
        memberships(&store, &ids, &id),
        with_store(ALL_MAIL, &["\\Inbox"])
    );
}

#[test]
fn a_parked_row_is_claimed_by_another_store_and_a_lone_one_leaves_at_the_sweep() {
    let (store, ids) = listed();
    let (all_mail, spam) = (&ids[ALL_MAIL], &ids[SPAM]);
    write(&store, all_mail, &[facts(1, &["\\Inbox"], MSGID)]);
    let id = created(&store, 0);
    let before = write(&store, spam, &[]);
    let in_all_mail = Standing {
        folder: all_mail,
        uid_validity: 1,
    };
    assert_eq!(
        store.park_uids(&key(), &in_all_mail, &[1]).unwrap(),
        Some(())
    );
    assert_eq!(store.state(&key()).unwrap(), before, "parking is no state");
    assert_eq!(
        memberships(&store, &ids, &id),
        with_store(ALL_MAIL, &["\\Inbox"])
    );
    let claimed = write(&store, spam, &[facts(9, &["\\Inbox"], MSGID)]);
    assert_eq!(
        logged(&store, ObjectType::Email, before),
        [(id.to_string(), ChangeKind::Updated)]
    );
    assert_eq!(memberships(&store, &ids, &id), with_store(SPAM, &[]));
    let mailboxes: BTreeSet<String> = logged(&store, ObjectType::Mailbox, before)
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    for lost in [all_mail, &ids["\\Inbox"], spam] {
        assert!(mailboxes.contains(lost.as_str()), "{lost}");
    }
    assert_eq!(store.locate(&key(), &[id.as_str()]).unwrap()[0].uid, 9);
    let in_spam = Standing {
        folder: spam,
        uid_validity: 1,
    };
    store.park_uids(&key(), &in_spam, &[9]).unwrap();
    assert_eq!(store.sweep_parked(&key()).unwrap(), claimed + 1);
    assert_eq!(
        logged(&store, ObjectType::Email, claimed),
        [(id.to_string(), ChangeKind::Destroyed)]
    );
    assert_eq!(
        logged(&store, ObjectType::Thread, claimed),
        [("t9".to_owned(), ChangeKind::Destroyed)]
    );
    assert_eq!(
        logged(&store, ObjectType::Mailbox, claimed),
        [(spam.to_string(), ChangeKind::Updated)]
    );
    assert!(
        store
            .emails(&key(), &[id.as_str()])
            .unwrap()
            .rows
            .is_empty()
    );
    let swept = store.state(&key()).unwrap();
    assert_eq!(store.sweep_parked(&key()).unwrap(), swept, "nothing waits");
}

#[test]
fn a_message_without_its_items_is_stored_the_way_a_folder_account_stores_it() {
    let (store, ids) = listed();
    let plain = EmailFacts {
        gmail: None,
        ..facts(1, &[], MSGID)
    };
    write(&store, &ids[ALL_MAIL], &[plain]);
    let id = created(&store, 0);
    assert_eq!(memberships(&store, &ids, &id), with_store(ALL_MAIL, &[]));
    let row = &store.emails(&key(), &[id.as_str()]).unwrap().rows[0];
    assert_ne!(
        row.thread_id.as_str(),
        "t9",
        "the thread is the bridge's own"
    );
}

#[test]
fn an_id_another_live_uid_of_the_folder_holds_leaves_the_second_message_out() {
    let (store, ids) = listed();
    let all_mail = &ids[ALL_MAIL];
    let twins = [facts(1, &[], MSGID), facts(2, &[], MSGID)];
    write(&store, all_mail, &twins);
    let id = created(&store, 0);
    assert_eq!(store.locate(&key(), &[id.as_str()]).unwrap()[0].uid, 1);
    let high = [facts(3, &[], HIGH_MSGID), facts(4, &[], HIGH_MSGID + 1)];
    let state = write(&store, all_mail, &high);
    assert_eq!(logged(&store, ObjectType::Email, state - 1).len(), 2);
}

#[test]
fn a_keyword_write_is_one_state_with_the_email_and_every_mailbox_of_it_updated() {
    let (store, ids) = listed();
    let all_mail = &ids[ALL_MAIL];
    let state = write(&store, all_mail, &[facts(1, &["\\Inbox", "Work"], MSGID)]);
    let id = created(&store, 0);
    let (read_at, rows) = store
        .keyword_rows(&key(), &[id.as_str(), "e-none"])
        .unwrap();
    assert_eq!((read_at, rows.len()), (state, 1));
    assert_eq!((&rows[0].folder, rows[0].uid), (all_mail, Some(1)));
    let change = |uid: u32, add: &[&str], remove: &[&str]| KeywordChange {
        id: id.clone(),
        folder: all_mail.clone(),
        uid,
        add: add.iter().map(|keyword| (*keyword).to_owned()).collect(),
        remove: remove.iter().map(|keyword| (*keyword).to_owned()).collect(),
    };
    let written = store
        .write_keywords(&key(), &[change(1, &["$flagged"], &["$seen"])])
        .unwrap();
    assert_eq!((written.before, written.after), (state, state + 1));
    assert!(written.gone.is_empty());
    let kept = &store.keyword_rows(&key(), &[id.as_str()]).unwrap().1[0];
    assert_eq!(kept.keywords.keys().collect::<Vec<_>>(), ["$flagged"]);
    let updated = (id.to_string(), ChangeKind::Updated);
    assert_eq!(logged(&store, ObjectType::Email, state), [updated]);
    let mut mailboxes: Vec<String> = logged(&store, ObjectType::Mailbox, state)
        .into_iter()
        .map(|(mailbox, _)| mailbox)
        .collect();
    mailboxes.sort();
    let mut expected: Vec<String> = [ALL_MAIL, "\\Inbox", "Work"]
        .iter()
        .map(|name| ids[*name].to_string())
        .collect();
    expected.sort();
    assert_eq!(mailboxes, expected);
    // The same keywords again move no state; a row that lies under
    // another UID by then is left as it is and named.
    let again = store
        .write_keywords(
            &key(),
            &[change(1, &["$flagged"], &[]), change(2, &["x"], &[])],
        )
        .unwrap();
    assert_eq!((again.before, again.after), (state + 1, state + 1));
    assert_eq!(again.gone.as_slice(), std::slice::from_ref(&id));
    // Another account neither reads the row nor writes it.
    let other = AccountKey::new("a2");
    assert!(
        store
            .keyword_rows(&other, &[id.as_str()])
            .unwrap()
            .1
            .is_empty()
    );
    let foreign = store
        .write_keywords(&other, &[change(1, &["x"], &["$flagged"])])
        .unwrap();
    assert_eq!((foreign.after, foreign.gone), (0, vec![id.clone()]));
    let kept = &store.keyword_rows(&key(), &[id.as_str()]).unwrap().1[0];
    assert_eq!(kept.keywords.keys().collect::<Vec<_>>(), ["$flagged"]);
    assert_eq!(store.state(&key()).unwrap(), state + 1);
}
