// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The names beside the one asked for, read on Gmail whatever their case or width.

use proptest::prelude::*;

use super::*;
use crate::runtime::folders::plan::tests::{dovecot, row};
use crate::runtime::folders::step::Under;

/// Letters in every form that reads as one name on a Gmail account after a
/// pass: small and capital, inside ASCII at full width too. Outside ASCII an
/// e with an acute accent, an l with a stroke, two Cyrillic letters and two
/// Greek letters. Letters whose forms the reading keeps apart stay out: a
/// capital I with a dot, the sharp s and the sigma.
const LETTERS: [&[char]; 9] = [
    &['x', 'X', '\u{ff58}', '\u{ff38}'],
    &['y', 'Y', '\u{ff59}', '\u{ff39}'],
    &['z', 'Z', '\u{ff5a}', '\u{ff3a}'],
    &['\u{e9}', '\u{c9}'],
    &['\u{142}', '\u{141}'],
    &['\u{43f}', '\u{41f}'],
    &['\u{451}', '\u{401}'],
    &['\u{3b1}', '\u{391}'],
    &['\u{3c9}', '\u{3a9}'],
];

/// The most letters a drawn label holds: room for every kind in one name.
const LABEL_LETTERS: usize = 6;

/// Where a label may stand: at the top or under a mailbox of `dovecot`.
const PLACES: [Option<&str>; 3] = [None, Some("mwork"), Some("mlists")];

/// The mailbox that holds the drawn label.
const LABELED: &str = "mlabel";

/// A mailbox of `dovecot` a RENAME may move onto the label.
const ANOTHER: &str = "mlocked";

/// A mailbox without a row.
const GONE: &str = "mgone";

/// One letter of `LETTERS`: the form a label shows it in and the form a
/// variant shows it in.
fn letter() -> impl Strategy<Value = (usize, usize, usize)> {
    (0..LETTERS.len()).prop_flat_map(|group| {
        let forms = 0..LETTERS[group].len();
        (Just(group), forms.clone(), forms)
    })
}

/// A label and a variant of it: the same letters, one at least in another
/// case or width.
fn names() -> impl Strategy<Value = (String, String)> {
    prop::collection::vec(letter(), 1..=LABEL_LETTERS)
        .prop_filter("a variant differs somewhere", |letters| {
            letters.iter().any(|(_, shown, varied)| shown != varied)
        })
        .prop_map(|letters| {
            let label: String = letters
                .iter()
                .map(|&(group, shown, _)| LETTERS[group][shown])
                .collect();
            let variant: String = letters
                .iter()
                .map(|&(group, _, varied)| LETTERS[group][varied])
                .collect();
            (label, variant)
        })
}

fn place() -> impl Strategy<Value = Option<&'static str>> {
    prop::sample::select(PLACES.to_vec())
}

fn mailbox(id: &str) -> MailboxId {
    MailboxId::from(id.to_owned())
}

/// The rows of `dovecot` with the label held under `parent`. No name of
/// `dovecot` spells in these letters.
fn rows(label: &str, parent: Option<&str>) -> Vec<MailboxRow> {
    let mut rows = dovecot().rows;
    rows.push(row(LABELED, label, parent, None));
    rows
}

/// A name asked for under `parent`.
fn naming(name: &str, parent: Option<&str>) -> Naming {
    Naming {
        typed: name.to_owned(),
        leaf: name.to_owned(),
        under: parent.map_or(Under::Top, |parent| Under::Mailbox(mailbox(parent))),
    }
}

/// The CREATE of `name` under `parent` and the RENAME of `ANOTHER` onto it.
fn create_and_rename(name: &str, parent: Option<&str>) -> [Contested; 2] {
    let naming = naming(name, parent);
    [
        Contested::new(0, &naming, None),
        Contested::new(1, &naming, Some(mailbox(ANOTHER))),
    ]
}

proptest! {
    /// A label stands beside a variant of itself in another case or width,
    /// whether a CREATE or the RENAME of another mailbox asked for it.
    #[test]
    fn a_case_or_width_variant_of_a_label_beside_it_is_taken_on_gmail_rfc8621_2_5(
        (label, variant) in names(),
        parent in place(),
    ) {
        let asked = create_and_rename(&variant, parent);
        prop_assert_eq!(taken(&asked, &rows(&label, parent), true), vec![0, 1]);
    }

    /// Under any other parent neither the name nor a variant of it stands
    /// beside anything.
    #[test]
    fn a_variant_under_another_parent_is_not_taken_on_gmail_rfc8621_2_5(
        (label, variant) in names(),
        parent in place(),
    ) {
        let rows = rows(&label, parent);
        for other in PLACES.into_iter().filter(|other| *other != parent) {
            for name in [&label, &variant] {
                let asked = create_and_rename(name, other);
                prop_assert_eq!(taken(&asked, &rows, true), Vec::<usize>::new());
            }
        }
    }

    /// A mailbox renamed to a variant of its own name, with no other row
    /// holding it, stands beside nothing: its own row counts for nothing.
    #[test]
    fn a_rename_to_a_variant_of_its_own_name_is_not_taken_by_its_own_row(
        (label, variant) in names(),
        parent in place(),
    ) {
        let asked = [Contested::new(0, &naming(&variant, parent), Some(mailbox(LABELED)))];
        prop_assert_eq!(taken(&asked, &rows(&label, parent), true), Vec::<usize>::new());
    }

    /// A RENAME of a mailbox that lost its row stands beside nothing, the
    /// name itself included.
    #[test]
    fn a_rename_of_a_mailbox_that_lost_its_row_is_not_taken(
        (label, variant) in names(),
        parent in place(),
    ) {
        let asked = [
            Contested::new(0, &naming(&variant, parent), Some(mailbox(GONE))),
            Contested::new(1, &naming(&label, parent), Some(mailbox(GONE))),
        ];
        prop_assert_eq!(taken(&asked, &rows(&label, parent), true), Vec::<usize>::new());
    }

    /// On any other account the name itself is taken and a variant is not.
    #[test]
    fn without_gmail_the_name_itself_is_taken_and_a_variant_is_not_rfc8621_2_5(
        (label, variant) in names(),
        parent in place(),
    ) {
        let asked = [
            Contested::new(0, &naming(&variant, parent), None),
            Contested::new(1, &naming(&label, parent), None),
        ];
        prop_assert_eq!(taken(&asked, &rows(&label, parent), false), vec![1]);
    }
}
