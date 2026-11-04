// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The values a first sync records of its server: those of the opening
//! that wrote first, kept by every later opening and read as how far the
//! folder is synced once it is done.

use super::*;

/// The values an opening of the first sync read, `at` openings in.
fn opened(at: u32) -> Synced {
    Synced {
        uid_next: Some(NEXT_AT_OPEN + at),
        highest_modseq: Some(OPENED_AT_MODSEQ + u64::from(at)),
        messages: Some(at),
    }
}

fn down<'a>(folder: &'a MailboxId, emails: &'a [EmailFacts], at: u32, done: bool) -> Batch<'a> {
    Batch {
        folder,
        uid_validity: 1,
        emails,
        advance: Advance::Down {
            lowest_uid: emails.iter().map(|email| email.uid).min(),
            done,
            synced: opened(at),
        },
    }
}

#[test]
fn every_later_opening_keeps_the_values_of_the_first_that_wrote_rfc3501_2_3_1_1() {
    let (store, inbox) = listed();
    for (at, uid, done) in [(0, 30, false), (1, 20, false), (2, 10, true)] {
        let emails = [facts(uid, None)];
        let batch = down(&inbox, &emails, at, done);
        store.apply_batch(&key(), &batch, &SEALER).unwrap();
    }
    let progress = store.sync_progress(&key(), &inbox).unwrap();
    assert_eq!(
        (progress.lowest_synced_uid, progress.done),
        (Some(10), true)
    );
    assert_eq!(progress.synced, opened(0));
}

#[test]
fn the_three_values_stay_together_as_one_examine_gave_them_rfc7162_3_1_2_2() {
    let (store, inbox) = listed();
    let unknown = Synced {
        uid_next: None,
        highest_modseq: None,
        ..opened(0)
    };
    let first = [facts(30, None)];
    let batch = Batch {
        advance: Advance::Down {
            lowest_uid: Some(30),
            done: false,
            synced: unknown,
        },
        ..down(&inbox, &first, 0, false)
    };
    store.apply_batch(&key(), &batch, &SEALER).unwrap();
    let last = [facts(10, None)];
    store
        .apply_batch(&key(), &down(&inbox, &last, 1, true), &SEALER)
        .unwrap();
    assert_eq!(store.sync_progress(&key(), &inbox).unwrap().synced, unknown);
}

#[test]
fn a_folder_under_its_first_sync_reads_as_synced_nowhere() {
    let (store, inbox) = listed();
    let emails = [facts(30, None)];
    let batch = down(&inbox, &emails, 0, false);
    store.apply_batch(&key(), &batch, &SEALER).unwrap();
    let progress = store.sync_progress(&key(), &inbox).unwrap();
    assert_eq!(
        (progress.lowest_synced_uid, progress.done),
        (Some(30), false)
    );
    assert_eq!(progress.synced, Synced::default());
}

#[test]
fn a_row_that_holds_no_values_takes_those_of_the_opening_that_writes() {
    let (store, inbox) = listed();
    store
        .write(&key(), |transaction| {
            transaction.execute(
                "INSERT INTO bridge_sync (account_key, folder_id, lowest_synced_uid, done)
                 VALUES (?1, ?2, 30, 0)",
                params![key().as_str(), inbox],
            )?;
            Ok(())
        })
        .unwrap();
    let emails = [facts(10, None)];
    let batch = down(&inbox, &emails, 1, true);
    store.apply_batch(&key(), &batch, &SEALER).unwrap();
    assert_eq!(
        store.sync_progress(&key(), &inbox).unwrap().synced,
        opened(1)
    );
}
