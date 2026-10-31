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

/// One pass of a server without ACL.
fn pass(store: &Store, found: &[MailboxFacts], moving: Moving) -> Result<u64, StoreError> {
    let rights = None;
    store.apply_pass(
        &key(),
        Pass {
            found,
            moving,
            rights,
        },
    )
}

#[test]
fn whether_the_server_moves_mail_shows_in_the_rights_and_a_change_of_it_updates_every_row() {
    let store = Store::in_memory().unwrap();
    let found = [facts("INBOX", None), facts("Archive", None)];
    store.apply_mailboxes(&key(), &found).unwrap();
    assert_eq!(moving(&store), Moving::default());
    let learned = pass(&store, &found, UIDPLUS).unwrap();
    assert_eq!(learned, 2, "the first pass of the process logs its rights");
    let kinds: Vec<ChangeKind> = changes(&store, 1)
        .into_iter()
        .map(|(_, kind)| kind)
        .collect();
    assert_eq!(kinds, [ChangeKind::Updated, ChangeKind::Updated]);
    assert_eq!(moving(&store), UIDPLUS);
    assert_eq!(pass(&store, &found, UIDPLUS).unwrap(), 2, "nothing moved");
    assert_eq!(
        pass(&store, &found, BOTH).unwrap(),
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
            let passing = scope.spawn(|| pass(&store, &found, BOTH));
            let snapshot = scope.spawn(|| store.mailbox_snapshot(&key()));
            drop(held);
            passing.join().unwrap().unwrap();
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

#[test]
fn the_letters_a_pass_reads_are_kept_logged_once_and_lost_when_left_out_rfc4314_3_5() {
    let store = Store::in_memory().unwrap();
    let found = [facts("INBOX", None), facts("Locked", None)];
    let read = HashMap::from([("Locked".to_owned(), "lrs".to_owned())]);
    let with = |rights| Pass {
        found: &found,
        moving: BOTH,
        rights,
    };
    let first = store.apply_pass(&key(), with(Some(&read))).unwrap();
    let snapshot = store.mailbox_snapshot(&key()).unwrap();
    let locked = snapshot.rows[1].id.clone();
    assert_eq!(
        snapshot.rights,
        HashMap::from([(locked.clone(), "lrs".to_owned())])
    );
    assert_eq!(store.pass_inputs(&key()).unwrap().rights, read);
    assert_eq!(
        store.apply_pass(&key(), with(Some(&read))).unwrap(),
        first,
        "the same letters log nothing"
    );
    let none = HashMap::new();
    let lost = store.apply_pass(&key(), with(Some(&none))).unwrap();
    assert_eq!(
        changes(&store, first),
        [(locked.to_string(), ChangeKind::Updated)],
        "a name left out loses its letters"
    );
    assert!(store.mailbox_snapshot(&key()).unwrap().rights.is_empty());
    let wider = HashMap::from([("Locked".to_owned(), "lrsw".to_owned())]);
    let after = store.apply_pass(&key(), with(Some(&wider))).unwrap();
    assert_eq!(
        changes(&store, lost),
        [(locked.to_string(), ChangeKind::Updated)]
    );
    store.apply_pass(&key(), with(None)).unwrap();
    assert_eq!(
        changes(&store, after),
        [(locked.to_string(), ChangeKind::Updated)]
    );
    assert!(store.mailbox_snapshot(&key()).unwrap().rights.is_empty());
}

#[test]
fn a_set_writes_its_changes_as_one_state_and_a_rename_carries_the_rows_under_it_rfc8620_5_3() {
    let store = Store::in_memory().unwrap();
    let mut label = facts("Work/Q3", Some("Work"));
    label.gmail_label = Some("Work/Q3".to_owned());
    let found = [
        facts("INBOX", None),
        facts("Work", None),
        label,
        facts("Old", None),
    ];
    store.apply_mailboxes(&key(), &found).unwrap();
    let id = |name: &str| store_id(&store, name);
    let (work, q3, old, inbox) = (id("Work"), id("Work/Q3"), id("Old"), id("INBOX"));
    let made = MailboxRow {
        id: MailboxId::generate(),
        parent_id: Some(inbox.clone()),
        facts: facts("INBOX/New", None),
    };
    let changes_made = [
        MailboxChange::Created {
            row: made.clone(),
            role: Some(RoleOverride::Role("archive".to_owned())),
        },
        MailboxChange::Renamed {
            id: work.clone(),
            imap_name: "Job".to_owned(),
            name: "Job".to_owned(),
            parent_id: None,
        },
        MailboxChange::Subscribed {
            id: work.clone(),
            subscribed: false,
        },
        MailboxChange::Role {
            id: inbox.clone(),
            role: RoleOverride::NoRole,
            sort_order: 6,
        },
        MailboxChange::Destroyed { id: old.clone() },
    ];
    let written = store.write_mailboxes(&key(), &changes_made).unwrap();
    assert_eq!((written.before, written.after), (1, 2));
    let mut logged = changes(&store, 1);
    logged.sort_by(|left, right| left.0.cmp(&right.0));
    let mut expected = vec![
        (made.id.to_string(), ChangeKind::Created),
        (work.to_string(), ChangeKind::Updated),
        (q3.to_string(), ChangeKind::Updated),
        (inbox.to_string(), ChangeKind::Updated),
        (old.to_string(), ChangeKind::Destroyed),
    ];
    expected.sort_by(|left, right| left.0.cmp(&right.0));
    assert_eq!(logged, expected);
    let rows = store.mailbox_snapshot(&key()).unwrap().rows;
    let by_id = |id: &MailboxId| rows.iter().find(|row| row.id == *id).unwrap();
    assert_eq!(by_id(&q3).facts.imap_name, "Job/Q3");
    assert_eq!(by_id(&q3).facts.gmail_label.as_deref(), Some("Job/Q3"));
    assert_eq!(by_id(&q3).parent_id.as_ref(), Some(&work));
    assert!(!by_id(&work).facts.subscribed);
    assert_eq!(by_id(&inbox).facts.role, None);
    assert!(rows.iter().all(|row| row.id != old));
    let overrides = store.pass_inputs(&key()).unwrap().overrides;
    assert_eq!(overrides["INBOX"], RoleOverride::NoRole);
    assert_eq!(
        overrides["INBOX/New"],
        RoleOverride::Role("archive".to_owned())
    );
    let nothing = store.write_mailboxes(&key(), &[]).unwrap();
    assert_eq!((nothing.before, nothing.after), (2, 2));
}

/// The id of the row with that wire name.
fn store_id(store: &Store, imap_name: &str) -> MailboxId {
    let rows = store.mailbox_snapshot(&key()).unwrap().rows;
    let row = rows
        .into_iter()
        .find(|row| row.facts.imap_name == imap_name);
    row.unwrap().id
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
