// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What a batch writes, what it refuses to write and what a renumbering
//! and a vanished folder do to the rows.

use super::*;
use crate::store::{ChangesSince, MailboxFacts, REMATCH_LIMIT, Synced};
use crate::testing::seal::TestSealer;

const SEALER: TestSealer = TestSealer { locked: false };

/// The HIGHESTMODSEQ and the UIDNEXT every first sync of these tests
/// opened under.
const OPENED_AT_MODSEQ: u64 = 40;
const NEXT_AT_OPEN: u32 = 50;

fn key() -> AccountKey {
    AccountKey::new("a1")
}

fn folder_facts(imap_name: &str, uid_validity: u32) -> MailboxFacts {
    MailboxFacts {
        name: imap_name.to_owned(),
        imap_name: imap_name.to_owned(),
        parent_imap_name: None,
        role: None,
        sort_order: 6,
        subscribed: true,
        selectable: true,
        store: true,
        gmail_label: None,
        uid_validity: Some(uid_validity),
        uid_next: Some(1),
        highest_modseq: None,
        total_emails: 0,
        unread_emails: 0,
    }
}

/// A store holding one listed INBOX; its id.
fn listed() -> (Store, MailboxId) {
    let store = Store::in_memory().unwrap();
    store
        .apply_mailboxes(&key(), &[folder_facts("INBOX", 1)])
        .unwrap();
    let id = store.mailbox_snapshot(&key()).unwrap().rows[0].id.clone();
    (store, id)
}

fn facts(uid: u32, message_id: Option<&str>) -> EmailFacts {
    EmailFacts {
        uid,
        keywords: BTreeMap::from([("$seen".to_owned(), true)]),
        size: 100,
        received_at: 1_000,
        sent_at: Some(900),
        has_attachment: false,
        gmail: None,
        personal: Personal {
            subject: Some(format!("Message {uid}")),
            message_id: message_id.map(|id| vec![id.to_owned()]),
            ..Personal::default()
        },
    }
}

fn batch<'a>(folder: &'a MailboxId, emails: &'a [EmailFacts], done: bool) -> Batch<'a> {
    Batch {
        folder,
        uid_validity: 1,
        emails,
        advance: Advance::Down {
            lowest_uid: emails.iter().map(|email| email.uid).min(),
            done,
            synced: Synced {
                uid_next: Some(NEXT_AT_OPEN),
                highest_modseq: Some(OPENED_AT_MODSEQ),
                messages: u32::try_from(emails.len()).ok(),
            },
        },
    }
}

fn logged(store: &Store, object: ObjectType, since: u64) -> Vec<(String, ChangeKind)> {
    let ChangesSince { changes, .. } = store.changes_since(&key(), object, since).unwrap();
    changes
        .unwrap()
        .into_iter()
        .map(|change| (change.id, change.kind))
        .collect()
}

fn ids_of(changes: &[(String, ChangeKind)]) -> Vec<&str> {
    changes.iter().map(|(id, _)| id.as_str()).collect()
}

#[test]
fn a_blob_opens_on_its_own_row_and_under_its_own_account_only() {
    let (store, inbox) = listed();
    let emails = [facts(1, Some("m1@example.test")), facts(2, None)];
    store
        .apply_batch(&key(), &batch(&inbox, &emails, true), &SEALER)
        .unwrap();
    let created = logged(&store, ObjectType::Email, 1);
    let rows = store.emails(&key(), &ids_of(&created)).unwrap().rows;
    let (first, second) = (&rows[0], &rows[1]);
    let plain = SEALER.open(&key(), &first.id, &first.sealed).unwrap();
    let personal: Personal = serde_json::from_slice(&plain).unwrap();
    assert!(personal.subject.unwrap().starts_with("Message "));
    assert_eq!(SEALER.open(&key(), &second.id, &first.sealed), None);
    let other = AccountKey::new("a2");
    assert_eq!(SEALER.open(&other, &first.id, &first.sealed), None);
    assert_eq!(store.emails(&other, &ids_of(&created)).unwrap().rows, []);
}

#[test]
fn a_message_the_folder_holds_is_skipped_and_a_batch_without_news_is_no_state() {
    let (store, inbox) = listed();
    let emails = [facts(7, None), facts(8, None)];
    let first = store
        .apply_batch(&key(), &batch(&inbox, &emails, false), &SEALER)
        .unwrap();
    assert_eq!(first, Some(2));
    let again = store
        .apply_batch(&key(), &batch(&inbox, &emails, false), &SEALER)
        .unwrap();
    assert_eq!(again, Some(2));
    assert_eq!(logged(&store, ObjectType::Email, 1).len(), 2);
    let progress = store.sync_progress(&key(), &inbox).unwrap();
    assert_eq!(progress.lowest_synced_uid, Some(7));
    assert!(!progress.done);
}

#[test]
fn the_finish_of_a_folder_is_logged_once_and_records_where_the_server_stood() {
    let (store, inbox) = listed();
    let done = store
        .apply_batch(&key(), &batch(&inbox, &[], true), &SEALER)
        .unwrap();
    assert_eq!(done, Some(2));
    let again = store
        .apply_batch(&key(), &batch(&inbox, &[], true), &SEALER)
        .unwrap();
    assert_eq!(again, Some(2));
    assert!(
        store
            .mailbox_snapshot(&key())
            .unwrap()
            .done
            .contains(&inbox)
    );
    let progress = store.sync_progress(&key(), &inbox).unwrap();
    assert_eq!(
        progress.synced,
        Synced {
            uid_next: Some(NEXT_AT_OPEN),
            highest_modseq: Some(OPENED_AT_MODSEQ),
            messages: Some(0),
        }
    );
}

#[test]
fn a_walk_upward_moves_the_recorded_uidnext_and_never_back() {
    let (store, inbox) = listed();
    store
        .apply_batch(&key(), &batch(&inbox, &[], true), &SEALER)
        .unwrap();
    let emails = [facts(60, None), facts(61, None)];
    let upward = |uid_next, arrived| Batch {
        folder: &inbox,
        uid_validity: 1,
        emails: &emails,
        advance: Advance::Up { uid_next, arrived },
    };
    store.apply_batch(&key(), &upward(62, 2), &SEALER).unwrap();
    let synced = store.sync_progress(&key(), &inbox).unwrap().synced;
    assert_eq!((synced.uid_next, synced.messages), (Some(62), Some(2)));
    store.apply_batch(&key(), &upward(55, 0), &SEALER).unwrap();
    let progress = store.sync_progress(&key(), &inbox).unwrap();
    assert_eq!(progress.synced.uid_next, Some(62));
    assert!(progress.done, "a walk upward leaves the first sync done");
    assert_eq!(logged(&store, ObjectType::Email, 2).len(), 2);
}

#[test]
fn a_count_never_learned_stays_unknown_under_a_walk_upward() {
    let (store, inbox) = listed();
    let opened = Batch {
        folder: &inbox,
        uid_validity: 1,
        emails: &[],
        advance: Advance::Down {
            lowest_uid: None,
            done: true,
            synced: Synced::default(),
        },
    };
    store.apply_batch(&key(), &opened, &SEALER).unwrap();
    let emails = [facts(60, None)];
    let upward = Batch {
        folder: &inbox,
        uid_validity: 1,
        emails: &emails,
        advance: Advance::Up {
            uid_next: 61,
            arrived: 1,
        },
    };
    store.apply_batch(&key(), &upward, &SEALER).unwrap();
    let synced = store.sync_progress(&key(), &inbox).unwrap().synced;
    assert_eq!((synced.uid_next, synced.messages), (Some(61), None));
}

#[test]
fn advance_keeps_a_value_it_did_not_learn_and_writes_nothing_for_a_renumbered_folder() {
    let (store, inbox) = listed();
    store
        .apply_batch(&key(), &batch(&inbox, &[], true), &SEALER)
        .unwrap();
    let standing = Standing {
        folder: &inbox,
        uid_validity: 1,
    };
    let learned = Synced {
        uid_next: Some(70),
        highest_modseq: None,
        messages: Some(9),
    };
    store.advance(&key(), &standing, learned).unwrap();
    assert_eq!(
        store.sync_progress(&key(), &inbox).unwrap().synced,
        Synced {
            uid_next: Some(70),
            highest_modseq: Some(OPENED_AT_MODSEQ),
            messages: Some(9),
        }
    );
    let stale = Standing {
        folder: &inbox,
        uid_validity: 2,
    };
    let later = Synced {
        uid_next: Some(80),
        ..learned
    };
    store.advance(&key(), &stale, later).unwrap();
    assert_eq!(
        store.sync_progress(&key(), &inbox).unwrap().synced.uid_next,
        Some(70)
    );
}

#[test]
fn a_batch_fetched_under_another_uidvalidity_writes_nothing() {
    let (store, inbox) = listed();
    let emails = [facts(1, None)];
    let stale = Batch {
        uid_validity: 2,
        ..batch(&inbox, &emails, true)
    };
    assert_eq!(store.apply_batch(&key(), &stale, &SEALER).unwrap(), None);
    assert_eq!(store.state(&key()).unwrap(), 1);
    assert!(!store.sync_progress(&key(), &inbox).unwrap().done);
}

#[test]
fn a_renumbering_shows_across_a_pass_that_learned_no_uidvalidity_rfc3501_2_3_1_1() {
    let (store, inbox) = listed();
    let emails = [facts(1, None)];
    store
        .apply_batch(&key(), &batch(&inbox, &emails, true), &SEALER)
        .unwrap();
    let unknown = MailboxFacts {
        uid_validity: None,
        ..folder_facts("INBOX", 1)
    };
    store.apply_mailboxes(&key(), &[unknown]).unwrap();
    assert!(store.sync_progress(&key(), &inbox).unwrap().done);
    store
        .apply_mailboxes(&key(), &[folder_facts("INBOX", 2)])
        .unwrap();
    assert!(!store.sync_progress(&key(), &inbox).unwrap().done);
}

#[test]
fn a_match_across_a_renumbering_needs_the_hash_the_date_and_the_size() {
    let (store, inbox) = listed();
    let emails = [
        facts(1, Some("same@example.test")),
        facts(2, Some("date@example.test")),
        facts(3, Some("size@example.test")),
        facts(4, None),
    ];
    store
        .apply_batch(&key(), &batch(&inbox, &emails, true), &SEALER)
        .unwrap();
    let before = logged(&store, ObjectType::Email, 1);
    store
        .apply_mailboxes(&key(), &[folder_facts("INBOX", 2)])
        .unwrap();
    assert!(!store.sync_progress(&key(), &inbox).unwrap().done);
    let mut fresh = [
        facts(11, Some("same@example.test")),
        facts(12, Some("date@example.test")),
        facts(13, Some("size@example.test")),
        facts(14, None),
    ];
    fresh[1].received_at += 1;
    fresh[2].size += 1;
    let renumbered = Batch {
        uid_validity: 2,
        ..batch(&inbox, &fresh, true)
    };
    let state = store.apply_batch(&key(), &renumbered, &SEALER).unwrap();
    let after = logged(&store, ObjectType::Email, state.unwrap() - 1);
    let count = |kind| after.iter().filter(|(_, found)| *found == kind).count();
    assert_eq!(count(ChangeKind::Updated), 2, "{after:?}");
    assert_eq!(count(ChangeKind::Created), 2);
    assert_eq!(count(ChangeKind::Destroyed), 2);
    for (id, kind) in &after {
        let known = before.iter().any(|(old, _)| old == id);
        assert_eq!(known, *kind != ChangeKind::Created, "{id}");
    }
    assert_eq!(
        store.mailbox_snapshot(&key()).unwrap().counts[&inbox].synced_emails,
        4
    );
}

/// `count` rows in the folder by one statement, as a real sync would
/// take minutes to write them.
fn fill(store: &Store, folder: &MailboxId, count: u32) {
    store
        .write(&key(), |transaction| {
            transaction.execute(
                "WITH RECURSIVE n(uid) AS (SELECT 1 UNION ALL SELECT uid + 1 FROM n WHERE uid < ?3)
                 INSERT INTO bridge_emails
                 (account_key, id, folder_id, uid, thread_id, keywords, size, received_at,
                  has_attachment, sealed)
                 SELECT ?1, 'e' || uid, ?2, uid, 't' || uid, '{}', 0, 0, 0, X'00' FROM n",
                params![key().as_str(), folder, count],
            )?;
            Ok(())
        })
        .unwrap();
}

#[test]
fn a_renumbered_folder_past_the_rematch_limit_leaves_whole() {
    let (store, inbox) = listed();
    fill(&store, &inbox, REMATCH_LIMIT + 1);
    store
        .apply_mailboxes(&key(), &[folder_facts("INBOX", 2)])
        .unwrap();
    let destroyed = logged(&store, ObjectType::Email, 1);
    assert_eq!(destroyed.len(), usize::try_from(REMATCH_LIMIT).unwrap() + 1);
    let waiting = store
        .read(|connection| folders::unmatched(connection, &key(), &inbox))
        .unwrap();
    assert!(waiting.is_empty());
}

#[test]
fn a_thread_that_keeps_an_email_elsewhere_is_updated_when_a_folder_vanishes() {
    let store = Store::in_memory().unwrap();
    let found = [folder_facts("INBOX", 1), folder_facts("Work", 1)];
    store.apply_mailboxes(&key(), &found).unwrap();
    let rows = store.mailbox_snapshot(&key()).unwrap().rows;
    let (inbox, work) = (rows[0].id.clone(), rows[1].id.clone());
    fill(&store, &inbox, 1);
    store
        .write(&key(), |transaction| {
            transaction.execute(
                "INSERT INTO bridge_emails
                 (account_key, id, folder_id, uid, thread_id, keywords, size, received_at,
                  has_attachment, sealed)
                 VALUES (?1, 'e9', ?2, 9, 't1', '{}', 0, 0, 0, X'00'),
                        (?1, 'e8', ?2, 8, 't8', '{}', 0, 0, 0, X'00')",
                params![key().as_str(), work],
            )?;
            Ok(())
        })
        .unwrap();
    store.apply_mailboxes(&key(), &found[..1]).unwrap();
    assert_eq!(
        logged(&store, ObjectType::Thread, 1),
        [
            ("t1".to_owned(), ChangeKind::Updated),
            ("t8".to_owned(), ChangeKind::Destroyed)
        ]
    );
    assert_eq!(logged(&store, ObjectType::Email, 1).len(), 2);
}
