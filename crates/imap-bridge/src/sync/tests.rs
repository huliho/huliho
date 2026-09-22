// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The slices of a walk and what a failed one narrows to, in both
//! directions.

use super::*;

fn sync(walk: Walk, remaining: Vec<u32>) -> FolderSync {
    let top = remaining.iter().copied().max().unwrap_or(0);
    FolderSync {
        folder: MailboxId::generate(),
        imap_name: "INBOX".to_owned(),
        uid_validity: 1,
        walk,
        gmail: false,
        remaining,
        narrowed: Vec::new(),
        budget: NARROWING_BUDGET,
        top,
        answered: 0,
    }
}

fn down(remaining: Vec<u32>) -> FolderSync {
    sync(
        Walk::Down {
            synced: Synced::default(),
        },
        remaining,
    )
}

fn uids(slice: &Slice) -> (Vec<u32>, bool) {
    (slice.uids.clone(), slice.structure)
}

#[test]
fn a_batch_is_the_top_of_what_remains() {
    let count = u32::try_from(SYNC_BATCH).unwrap();
    let mut sync = down((1..=count + 3).collect());
    let first = sync.next_slice().unwrap();
    assert_eq!(
        first.range(),
        Some(UidRange {
            low: 4,
            high: count + 3
        })
    );
    assert_eq!(first.uids.len(), SYNC_BATCH);
    let rest = sync.next_slice().unwrap();
    assert_eq!(rest.range(), Some(UidRange { low: 1, high: 3 }));
    assert!(sync.next_slice().is_none());
}

#[test]
fn a_failed_slice_halves_with_the_upper_half_first_down_to_one_message() {
    let mut sync = down(vec![1, 2, 3, 4, 5]);
    let whole = sync.next_slice().unwrap();
    assert_eq!(sync.narrow(whole), None);
    let upper = sync.next_slice().unwrap();
    assert_eq!(uids(&upper), (vec![3, 4, 5], true));
    assert_eq!(sync.narrow(upper), None);
    let top = sync.next_slice().unwrap();
    assert_eq!(uids(&top), (vec![4, 5], true));
    assert_eq!(sync.narrow(top), None);
    let lone = sync.next_slice().unwrap();
    assert_eq!(uids(&lone), (vec![5], true));
    assert_eq!(sync.narrow(lone), None);
    let bare = sync.next_slice().unwrap();
    assert_eq!(uids(&bare), (vec![5], false));
    assert_eq!(sync.narrow(bare), Some(5));
    let waiting: Vec<_> = sync.narrowed.iter().map(uids).collect();
    assert_eq!(
        waiting,
        [(vec![1, 2], true), (vec![3], true), (vec![4], true)]
    );
}

#[test]
fn a_walk_upward_takes_the_lowest_uids_first_and_narrows_to_the_lower_half() {
    let count = u32::try_from(SYNC_BATCH).unwrap();
    let mut sync = sync(Walk::Up, (1..=count + 2).rev().collect());
    let first = sync.next_slice().unwrap();
    assert_eq!(
        first.range(),
        Some(UidRange {
            low: 1,
            high: count
        })
    );
    let rest = sync.next_slice().unwrap();
    assert_eq!(uids(&rest), (vec![count + 1, count + 2], true));
    assert_eq!(sync.narrow(rest), None);
    assert_eq!(uids(&sync.next_slice().unwrap()), (vec![count + 1], true));
    assert_eq!(uids(&sync.next_slice().unwrap()), (vec![count + 2], true));
}

#[test]
fn a_walk_upward_widens_to_a_higher_top_and_refuses_a_gap_the_uid_list_should_take() {
    let mut sync = sync(Walk::Up, vec![7, 6, 5]);
    assert!(sync.extend_to(7), "nothing to add");
    assert!(sync.extend_to(9));
    assert_eq!(sync.remaining, [9, 8, 7, 6, 5]);
    assert_eq!(sync.top, 9);
    assert!(!sync.extend_to(9 + REFRESH_GAP));
    assert_eq!(sync.top, 9);
    let mut first = down(vec![1, 2]);
    assert!(first.extend_to(u32::MAX), "a walk down has no top to widen");
}

#[test]
fn past_the_budget_a_slice_goes_back_whole() {
    let mut sync = down(vec![1, 2, 3, 4]);
    sync.budget = 1;
    let whole = sync.next_slice().unwrap();
    assert_eq!(sync.narrow(whole), None);
    let upper = sync.next_slice().unwrap();
    assert_eq!(sync.narrow(upper), None);
    assert_eq!(uids(&sync.next_slice().unwrap()), (vec![3, 4], true));
    assert_eq!(sync.budget, 0);
}

#[test]
fn the_budget_passes_six_hostile_messages_in_one_batch() {
    let per_message = SYNC_BATCH.ilog2() as usize + 2;
    assert!(6 * per_message <= NARROWING_BUDGET);
    assert!(7 * per_message > NARROWING_BUDGET);
}
