// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Threads over the ids a header names: the partition holds for any
//! order of arrival, the log tells a client what a merge did and the
//! thread counts of a mailbox follow the partition.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use huliho_imap_bridge::store::{
    AccountKey, Advance, Batch, ChangeKind, EmailFacts, MailboxFacts, MailboxId, ObjectType,
    Personal, Store, Synced,
};
use huliho_imap_bridge::testing::seal::TestSealer;
use proptest::prelude::*;

const SEALER: TestSealer = TestSealer { locked: false };

/// The most messages one case holds.
const MAX_MESSAGES: usize = 12;

/// The ids outside the case a message may name, as a reply to mail the
/// account never received does.
const STRANGERS: usize = 3;

fn key() -> AccountKey {
    AccountKey::new("a1")
}

/// One message of a case: whether it carries an id of its own, the
/// messages and the strangers it refers to and whether it was read.
#[derive(Debug, Clone)]
struct Letter {
    has_id: bool,
    refers_to: Vec<usize>,
    strangers: Vec<usize>,
    seen: bool,
}

fn letter() -> impl Strategy<Value = Letter> {
    (
        prop::bool::weighted(0.9),
        prop::collection::vec(0..MAX_MESSAGES, 0..3),
        prop::collection::vec(0..STRANGERS, 0..2),
        any::<bool>(),
    )
        .prop_map(|(has_id, refers_to, strangers, seen)| Letter {
            has_id,
            refers_to,
            strangers,
            seen,
        })
}

/// A case with an order of arrival and where the batches end.
fn case() -> impl Strategy<Value = (Vec<Letter>, Vec<usize>, usize)> {
    prop::collection::vec(letter(), 1..=MAX_MESSAGES).prop_flat_map(|letters| {
        let count = letters.len();
        let order = Just((0..count).collect::<Vec<_>>()).prop_shuffle();
        (Just(letters), order, 1..=count)
    })
}

fn own_id(index: usize) -> String {
    format!("m{index}@example.test")
}

/// The ids a letter names beyond its own; one that points past the case
/// names a stranger instead.
fn named(letters: &[Letter], index: usize) -> Vec<String> {
    let letter = &letters[index];
    let known = letter.refers_to.iter().map(|other| {
        if *other < letters.len() {
            own_id(*other)
        } else {
            format!("x{other}@elsewhere.test")
        }
    });
    let strangers = letter
        .strangers
        .iter()
        .map(|stranger| format!("s{stranger}@elsewhere.test"));
    known.chain(strangers).collect()
}

/// The message of a letter; its size is its index, so a row tells which
/// letter it stands for.
fn facts(letters: &[Letter], index: usize) -> EmailFacts {
    let letter = &letters[index];
    let keywords = if letter.seen {
        BTreeMap::from([("$seen".to_owned(), true)])
    } else {
        BTreeMap::new()
    };
    let references = named(letters, index);
    EmailFacts {
        uid: u32::try_from(index).unwrap() + 1,
        keywords,
        size: u32::try_from(index).unwrap(),
        received_at: 1_000,
        sent_at: None,
        has_attachment: false,
        personal: Personal {
            message_id: letter.has_id.then(|| vec![own_id(index)]),
            references: (!references.is_empty()).then_some(references),
            ..Personal::default()
        },
    }
}

/// The root of a node in the test's own union-find.
fn find(parent: &mut [usize], mut node: usize) -> usize {
    while parent[node] != node {
        parent[node] = parent[parent[node]];
        node = parent[node];
    }
    node
}

/// The partition the ids imply: two letters share a part when a chain
/// of named ids joins them.
fn expected(letters: &[Letter]) -> BTreeSet<BTreeSet<usize>> {
    let mut part_of_id: HashMap<String, usize> = HashMap::new();
    let mut parent: Vec<usize> = (0..letters.len()).collect();
    for index in 0..letters.len() {
        let mut ids = named(letters, index);
        if letters[index].has_id {
            ids.push(own_id(index));
        }
        for id in ids {
            let other = *part_of_id.entry(id).or_insert(index);
            let (a, b) = (find(&mut parent, index), find(&mut parent, other));
            parent[a] = b;
        }
    }
    let mut parts: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
    for index in 0..letters.len() {
        let root = find(&mut parent, index);
        parts.entry(root).or_default().insert(index);
    }
    parts.into_values().collect()
}

fn folder_facts() -> MailboxFacts {
    MailboxFacts {
        name: "INBOX".to_owned(),
        imap_name: "INBOX".to_owned(),
        parent_imap_name: None,
        role: Some("inbox".to_owned()),
        sort_order: 0,
        subscribed: true,
        selectable: true,
        store: true,
        gmail_label: None,
        uid_validity: Some(1),
        uid_next: Some(1),
        highest_modseq: None,
        total_emails: 0,
        unread_emails: 0,
    }
}

fn listed() -> (Store, MailboxId) {
    let store = Store::in_memory().unwrap();
    store.apply_mailboxes(&key(), &[folder_facts()]).unwrap();
    let id = store.mailbox_snapshot(&key()).unwrap().rows[0].id.clone();
    (store, id)
}

fn write(store: &Store, folder: &MailboxId, emails: &[EmailFacts]) -> u64 {
    let batch = Batch {
        folder,
        uid_validity: 1,
        emails,
        advance: Advance::Down {
            lowest_uid: None,
            done: false,
            synced: Synced::default(),
        },
    };
    store.apply_batch(&key(), &batch, &SEALER).unwrap().unwrap()
}

/// What the log says happened to each object of a type since a state,
/// folded as RFC 8620 section 5.2 asks.
fn folded(store: &Store, object: ObjectType, since: u64) -> BTreeMap<String, ChangeKind> {
    let mut fates: BTreeMap<String, Option<ChangeKind>> = BTreeMap::new();
    let changes = store.changes_since(&key(), object, since).unwrap().changes;
    for change in changes.unwrap() {
        let fate = fates.entry(change.id).or_insert(None);
        *fate = match (*fate, change.kind) {
            (None, kind) => Some(kind),
            (Some(ChangeKind::Created), ChangeKind::Destroyed) => None,
            (Some(ChangeKind::Created), _) => Some(ChangeKind::Created),
            (Some(_), ChangeKind::Destroyed) => Some(ChangeKind::Destroyed),
            (Some(before), _) => Some(before),
        };
    }
    fates
        .into_iter()
        .filter_map(|(id, fate)| fate.map(|kind| (id, kind)))
        .collect()
}

/// An email the store holds: its id and its thread.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Held {
    email: String,
    thread: String,
}

/// Every email the store holds, by the letter it stands for.
fn held(store: &Store) -> BTreeMap<usize, Held> {
    let created = folded(store, ObjectType::Email, 0);
    let ids: Vec<&str> = created.keys().map(String::as_str).collect();
    let rows = store.emails(&key(), &ids).unwrap().rows;
    rows.into_iter()
        .map(|row| {
            let held = Held {
                email: row.id.to_string(),
                thread: row.thread_id.to_string(),
            };
            (usize::try_from(row.size).unwrap(), held)
        })
        .collect()
}

fn partition(held: &BTreeMap<usize, Held>) -> BTreeSet<BTreeSet<usize>> {
    let mut parts: BTreeMap<&str, BTreeSet<usize>> = BTreeMap::new();
    for (letter, row) in held {
        parts.entry(&row.thread).or_default().insert(*letter);
    }
    parts.into_values().collect()
}

proptest! {
    #[test]
    fn any_order_of_arrival_gives_the_partition_the_ids_imply_rfc5256_3(
        (letters, order, first_batch) in case()
    ) {
        let (store, inbox) = listed();
        let arriving: Vec<EmailFacts> = order.iter().map(|index| facts(&letters, *index)).collect();
        let (early, late) = arriving.split_at(first_batch.min(arriving.len()));
        let between = write(&store, &inbox, early);
        let before = held(&store);
        if !late.is_empty() {
            write(&store, &inbox, late);
        }
        let after = held(&store);
        prop_assert_eq!(partition(&after), expected(&letters));

        // From the start every thread that stands reads as created and
        // none that a merge ended shows at all.
        let live: BTreeSet<&String> = after.values().map(|row| &row.thread).collect();
        let from_start = folded(&store, ObjectType::Thread, 0);
        prop_assert!(from_start.values().all(|kind| *kind == ChangeKind::Created));
        prop_assert_eq!(from_start.keys().collect::<BTreeSet<_>>(), live.clone());

        // Across the second batch an email that changed its thread reads
        // as updated and a thread that ended as destroyed.
        let emails = folded(&store, ObjectType::Email, between);
        let threads = folded(&store, ObjectType::Thread, between);
        for (letter, row) in &before {
            if after[letter].thread != row.thread {
                prop_assert_eq!(emails.get(&row.email), Some(&ChangeKind::Updated));
            }
            if !live.contains(&row.thread) {
                prop_assert_eq!(threads.get(&row.thread), Some(&ChangeKind::Destroyed));
            }
        }
    }

    #[test]
    fn the_thread_counts_of_a_mailbox_follow_the_partition_rfc8621_2(
        (letters, order, _) in case()
    ) {
        let (store, inbox) = listed();
        let arriving: Vec<EmailFacts> = order.iter().map(|index| facts(&letters, *index)).collect();
        write(&store, &inbox, &arriving);
        let parts = expected(&letters);
        let unread = parts
            .iter()
            .filter(|part| part.iter().any(|letter| !letters[*letter].seen))
            .count();
        let counts = store.mailbox_snapshot(&key()).unwrap().counts[&inbox];
        prop_assert_eq!(usize::try_from(counts.total_threads).unwrap(), parts.len());
        prop_assert_eq!(usize::try_from(counts.unread_threads).unwrap(), unread);
    }
}

#[test]
fn a_reply_that_names_two_threads_joins_them_and_the_larger_one_survives() {
    let (store, inbox) = listed();
    let referring = |refers_to: Vec<usize>| Letter {
        has_id: true,
        refers_to,
        strangers: Vec::new(),
        seen: true,
    };
    let letters = vec![
        referring(vec![]),
        referring(vec![0]),
        referring(vec![]),
        referring(vec![1, 2]),
    ];
    let first: Vec<EmailFacts> = (0..3).map(|index| facts(&letters, index)).collect();
    let state = write(&store, &inbox, &first);
    let before = held(&store);
    assert_eq!(before[&0].thread, before[&1].thread);
    assert_ne!(before[&0].thread, before[&2].thread);
    write(&store, &inbox, &[facts(&letters, 3)]);
    let after = held(&store);
    assert!(after.values().all(|row| row.thread == before[&0].thread));
    let threads = folded(&store, ObjectType::Thread, state);
    assert_eq!(threads[&before[&0].thread], ChangeKind::Updated);
    assert_eq!(threads[&before[&2].thread], ChangeKind::Destroyed);
    let emails = folded(&store, ObjectType::Email, state);
    let updated = emails
        .values()
        .filter(|kind| **kind == ChangeKind::Updated)
        .count();
    assert_eq!(updated, 1, "the one email of the thread that ended");
}
