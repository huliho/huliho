// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Names beside each other on a Gmail account, which keeps one label per
//! name whatever its case: a difference in ASCII case is refused before a
//! command, any other goes out (RFC 8621 section 2).

use proptest::prelude::*;

use super::*;

/// A label of ASCII letters that neither starts nor ends in white space, each
/// space alone, as Gmail keeps its labels.
const LABEL: &str = "[A-Za-z]( ?[A-Za-z]){1,5}";

/// The most characters such a label holds, spaces included.
const LABEL_CHARACTERS: usize = 11;

/// A Gmail account over the rows of `dovecot`, those without a role labels.
pub(super) fn gmail() -> MailboxSnapshot {
    let mut snapshot = dovecot();
    snapshot.gmail = true;
    snapshot.rights.clear();
    for row in &mut snapshot.rows {
        if row.facts.role.is_none() {
            row.facts.store = false;
            row.facts.gmail_label = Some(row.facts.imap_name.clone());
        }
    }
    snapshot
}

/// `Work` and `Locked` renamed, in that order.
fn renames(work: &str, locked: &str) -> Ask {
    Ask {
        updates: vec![
            Update {
                name: Some(work.to_owned()),
                ..update("mwork")
            },
            Update {
                name: Some(locked.to_owned()),
                ..update("mlocked")
            },
        ],
        ..Ask::default()
    }
}

#[test]
fn a_name_in_another_ascii_case_is_refused_on_gmail_alone_rfc8621_2() {
    let ask = Ask {
        creates: vec![
            create("a", "work", None),
            create("b", "q3", Some("mwork")),
            create("c", "q3", None),
        ],
        ..Ask::default()
    };
    let (refused, steps) = outcome(&gmail(), &ask);
    assert_eq!(refused, [(0, name()), (1, name())]);
    assert_eq!(steps, [2]);
    let (refused, steps) = outcome(&dovecot(), &ask);
    assert_eq!(refused, []);
    assert_eq!(steps, [0, 1, 2]);
}

#[test]
fn a_rename_on_gmail_may_change_the_case_alone_and_frees_the_name_it_held() {
    let ask = Ask {
        creates: vec![create("a", "WORK", None)],
        updates: vec![Update {
            name: Some("work".to_owned()),
            ..update("mwork")
        }],
        ..Ask::default()
    };
    let (refused, steps) = outcome(&gmail(), &ask);
    assert_eq!(refused, [(0, name())], "the create runs first");
    assert_eq!(steps, [1]);
    let freed = renames("Job", "WORK");
    assert_eq!(outcome(&gmail(), &freed), (Vec::new(), vec![0, 1]));
    let (refused, steps) = outcome(&gmail(), &renames("Job", "JOB"));
    assert_eq!(refused, [(1, name())], "the first rename holds the name");
    assert_eq!(steps, [0]);
}

proptest! {
    /// Every mix of ASCII case of a label is refused before a command, and a
    /// name that adds a letter outside ASCII goes out.
    #[test]
    fn any_ascii_case_of_a_label_is_refused_and_any_other_name_goes_out(
        label in LABEL,
        flips in proptest::collection::vec(any::<bool>(), LABEL_CHARACTERS),
    ) {
        let mut snapshot = gmail();
        snapshot.rows.push(row("mlabel", &label, None, None));
        let flipped: String = label
            .chars()
            .zip(&flips)
            .map(|(letter, flip)| {
                if *flip {
                    letter.to_ascii_uppercase()
                } else {
                    letter.to_ascii_lowercase()
                }
            })
            .collect();
        let beside = format!("{label}\u{3c3}");
        let ask = Ask {
            creates: vec![create("a", &flipped, None), create("b", &beside, None)],
            ..Ask::default()
        };
        let (refused, steps) = outcome(&snapshot, &ask);
        prop_assert_eq!(refused, vec![(0, name())]);
        prop_assert_eq!(steps, vec![1]);
    }
}
