// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What a listing pass writes to the mailbox rows and what the counts
//! read back from the memberships.

use super::*;
use crate::store::ChangesSince;

fn key() -> AccountKey {
    AccountKey::new("a1")
}

fn facts(imap_name: &str, parent: Option<&str>) -> MailboxFacts {
    MailboxFacts {
        name: imap_name.rsplit('/').next().unwrap().to_owned(),
        imap_name: imap_name.to_owned(),
        parent_imap_name: parent.map(str::to_owned),
        role: None,
        sort_order: 6,
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

fn changes(store: &Store, since: u64) -> Vec<(String, ChangeKind)> {
    let ChangesSince { changes, .. } = store
        .changes_since(&key(), ObjectType::Mailbox, since)
        .unwrap();
    changes
        .unwrap()
        .into_iter()
        .map(|change| (change.id, change.kind))
        .collect()
}

/// A server that moves mail with UIDPLUS alone.
const UIDPLUS: Moving = Moving {
    move_command: false,
    uidplus: true,
};

/// A server that offers MOVE and UIDPLUS.
const BOTH: Moving = Moving {
    move_command: true,
    uidplus: true,
};

/// The commands the snapshot says the server offers for moving mail.
fn moving(store: &Store) -> Moving {
    store.mailbox_snapshot(&key()).unwrap().moving
}

#[test]
fn whether_the_server_moves_mail_shows_in_the_rights_and_a_change_of_it_updates_every_row() {
    let store = Store::in_memory().unwrap();
    let found = [facts("INBOX", None), facts("Archive", None)];
    store.apply_mailboxes(&key(), &found).unwrap();
    assert_eq!(moving(&store), Moving::default());
    let learned = store.apply_pass(&key(), &found, UIDPLUS).unwrap();
    assert_eq!(learned, 2, "the first pass of the process logs its rights");
    let kinds: Vec<ChangeKind> = changes(&store, 1)
        .into_iter()
        .map(|(_, kind)| kind)
        .collect();
    assert_eq!(kinds, [ChangeKind::Updated, ChangeKind::Updated]);
    assert_eq!(moving(&store), UIDPLUS);
    assert_eq!(
        store.apply_pass(&key(), &found, UIDPLUS).unwrap(),
        2,
        "nothing moved"
    );
    assert_eq!(
        store.apply_pass(&key(), &found, BOTH).unwrap(),
        3,
        "one of the two words changed"
    );
    assert_eq!(moving(&store), BOTH);
    store.forget(&key());
    assert_eq!(moving(&store), Moving::default());
}

/// Which thread wins the connection is the scheduler's choice, so the
/// race runs several times.
const TRIALS: usize = 8;

/// A snapshot that lands while a pass writes sees the rows and the words
/// from before the pass or both from after it, never one of each.
#[test]
fn a_snapshot_during_a_pass_pairs_the_rows_with_the_words_they_belong_to() {
    for _ in 0..TRIALS {
        let store = Store::in_memory().unwrap();
        let found = [facts("INBOX", None)];
        store.apply_mailboxes(&key(), &found).unwrap();
        let seen = std::thread::scope(|scope| {
            // The connection held here parks both threads, so they meet.
            let held = store.lock().unwrap();
            let pass = scope.spawn(|| store.apply_pass(&key(), &found, BOTH));
            let snapshot = scope.spawn(|| store.mailbox_snapshot(&key()));
            drop(held);
            pass.join().unwrap().unwrap();
            snapshot.join().unwrap().unwrap()
        });
        assert_eq!(seen.moving == BOTH, seen.state == 2, "{seen:?}");
        assert_eq!(moving(&store), BOTH);
    }
}

#[test]
fn a_first_pass_creates_every_row_under_one_state_with_parents_by_id() {
    let store = Store::in_memory().unwrap();
    let found = [facts("INBOX", None), facts("INBOX/Work", Some("INBOX"))];
    assert_eq!(store.apply_mailboxes(&key(), &found).unwrap(), 1);
    let snapshot = store.mailbox_snapshot(&key()).unwrap();
    assert_eq!(snapshot.state, 1);
    assert_eq!(snapshot.rows.len(), 2);
    let inbox = &snapshot.rows[0];
    let work = &snapshot.rows[1];
    assert_eq!(inbox.facts.name, "INBOX");
    assert_eq!(work.parent_id.as_ref(), Some(&inbox.id));
    assert_eq!(work.facts.parent_imap_name, None);
    assert_eq!(snapshot.counts, HashMap::new());
    let logged = changes(&store, 0);
    assert_eq!(logged.len(), 2);
    assert!(logged.iter().all(|(_, kind)| *kind == ChangeKind::Created));
}

#[test]
fn a_second_pass_logs_the_difference_and_an_equal_pass_logs_nothing() {
    let store = Store::in_memory().unwrap();
    store
        .apply_mailboxes(&key(), &[facts("INBOX", None), facts("Old", None)])
        .unwrap();
    let ids: HashMap<String, MailboxId> = store
        .mailbox_snapshot(&key())
        .unwrap()
        .rows
        .into_iter()
        .map(|row| (row.facts.imap_name.clone(), row.id))
        .collect();
    let mut inbox = facts("INBOX", None);
    inbox.total_emails = 3;
    let state = store
        .apply_mailboxes(&key(), &[inbox.clone(), facts("New", None)])
        .unwrap();
    assert_eq!(state, 2);
    let logged = changes(&store, 1);
    assert!(logged.contains(&(ids["INBOX"].to_string(), ChangeKind::Updated)));
    assert!(logged.contains(&(ids["Old"].to_string(), ChangeKind::Destroyed)));
    assert_eq!(
        logged
            .iter()
            .filter(|(_, kind)| *kind == ChangeKind::Created)
            .count(),
        1
    );
    let again = store
        .apply_mailboxes(&key(), &[inbox, facts("New", None)])
        .unwrap();
    assert_eq!(again, 2);
    let rows = store.mailbox_snapshot(&key()).unwrap().rows;
    let kept = rows
        .iter()
        .find(|row| row.facts.imap_name == "INBOX")
        .unwrap();
    assert_eq!(kept.id, ids["INBOX"]);
    assert_eq!(kept.facts.total_emails, 3);
    assert_eq!(rows.len(), 2);
}

#[test]
fn two_accounts_never_see_each_other() {
    let store = Store::in_memory().unwrap();
    let other = AccountKey::new("a2");
    store
        .apply_mailboxes(&key(), &[facts("INBOX", None)])
        .unwrap();
    store
        .apply_mailboxes(&other, &[facts("INBOX", None)])
        .unwrap();
    assert_eq!(store.mailbox_snapshot(&other).unwrap().rows.len(), 1);
    store.apply_mailboxes(&other, &[]).unwrap();
    assert_eq!(store.mailbox_snapshot(&key()).unwrap().rows.len(), 1);
    assert_eq!(store.mailbox_snapshot(&other).unwrap().rows.len(), 0);
}

/// One email row with its membership: the id, the thread and the
/// keywords as one tuple. The id carries its own uid, so the rows stay
/// unique within the folder.
fn email(store: &Store, row: (&str, &str, &str), mailbox: &MailboxId) {
    let (id, thread, keywords) = row;
    let uid: u32 = id[1..].parse().unwrap();
    store
        .write(&key(), |transaction| {
            transaction.execute(
                "INSERT INTO bridge_emails
                 (account_key, id, folder_id, uid, thread_id, keywords, size,
                  received_at, has_attachment, sealed)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, 0, 0, X'00')",
                params![key().as_str(), id, mailbox, uid, thread, keywords],
            )?;
            transaction.execute(
                "INSERT INTO bridge_memberships (account_key, email_id, mailbox_id, received_at)
                 VALUES (?1, ?2, ?3, 0)",
                params![key().as_str(), id, mailbox],
            )?;
            Ok(())
        })
        .unwrap();
}

#[test]
fn thread_counts_follow_the_memberships_and_the_keyword_test_rfc8621_2() {
    let store = Store::in_memory().unwrap();
    store
        .apply_mailboxes(&key(), &[facts("INBOX", None)])
        .unwrap();
    let inbox = store.mailbox_snapshot(&key()).unwrap().rows[0].id.clone();
    email(&store, ("e1", "t1", r#"{"$seen":true}"#), &inbox);
    email(&store, ("e2", "t1", "{}"), &inbox);
    email(&store, ("e3", "t2", r#"{"$draft":true}"#), &inbox);
    email(
        &store,
        ("e4", "t3", r#"{"$seen":true,"$flagged":true}"#),
        &inbox,
    );
    let counts = store.mailbox_snapshot(&key()).unwrap().counts;
    assert_eq!(
        counts[&inbox],
        Counts {
            total_threads: 3,
            unread_threads: 1,
            synced_emails: 4,
            unread_emails: 1,
        }
    );
}
