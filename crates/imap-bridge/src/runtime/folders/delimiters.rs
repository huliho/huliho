// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The delimiter a new name goes out under: the one the parent's LIST
//! line carries, the INBOX line's at the top or the first one listed
//! where INBOX carries none (RFC 3501 section 6.3.8, RFC 9051 section
//! 7.3.1). A name the call makes or moves is kept under its current
//! spelling.

use std::collections::HashMap;

use crate::mailboxes::INBOX;
use crate::session::ListEntry;

/// The delimiter of every level the server listed.
pub(super) struct Delimiters {
    by_name: HashMap<String, Option<char>>,
    /// The INBOX line's, else the first one any line carries.
    pub top: Option<char>,
}

impl Delimiters {
    pub(super) fn of(entries: Vec<ListEntry>) -> Self {
        let inbox = entries
            .iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(INBOX));
        let top = inbox
            .and_then(|entry| entry.delimiter)
            .or_else(|| entries.iter().find_map(|entry| entry.delimiter));
        let by_name = entries
            .into_iter()
            .map(|entry| (entry.name, entry.delimiter))
            .collect();
        Self { by_name, top }
    }

    /// The delimiter under the mailbox of that wire name; one the
    /// listing left out takes the top's.
    pub(super) fn under(&self, wire: &str) -> Option<char> {
        self.by_name.get(wire).copied().unwrap_or(self.top)
    }

    /// A mailbox the call made, under the delimiter its name went out
    /// with.
    pub(super) fn made(&mut self, wire: &str, delimiter: Option<char>) {
        self.by_name.insert(wire.to_owned(), delimiter);
    }

    /// Every name a RENAME moved, kept under its new spelling.
    pub(super) fn renamed(&mut self, (from, to): (&str, &str)) {
        let delimiter = self.under(from);
        let by_name = std::mem::take(&mut self.by_name);
        self.by_name = by_name
            .into_iter()
            .map(|(wire, level)| (moved(&wire, (from, to), delimiter).unwrap_or(wire), level))
            .collect();
    }
}

/// The name a RENAME of `from` to `to` gives `wire`: `to` for `from`
/// itself and `to` in front of what follows the delimiter under it;
/// `None` for a name elsewhere (RFC 3501 section 6.3.5).
pub(super) fn moved(
    wire: &str,
    (from, to): (&str, &str),
    delimiter: Option<char>,
) -> Option<String> {
    if wire == from {
        return Some(to.to_owned());
    }
    let rest = wire.strip_prefix(from)?;
    let delimiter = delimiter?;
    let rest = rest.strip_prefix(delimiter)?;
    Some(format!("{to}{delimiter}{rest}"))
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn listed(name: &str, delimiter: Option<char>) -> ListEntry {
        ListEntry {
            name: name.to_owned(),
            delimiter,
            attributes: Vec::new(),
        }
    }

    #[test]
    fn a_rename_moves_the_name_and_every_name_under_it_by_its_delimiter_rfc3501_6_3_5() {
        assert_eq!(
            moved("Work", ("Work", "Job"), Some('/')),
            Some("Job".to_owned())
        );
        assert_eq!(
            moved("Work/Q3/Plan", ("Work", "Job"), Some('/')),
            Some("Job/Q3/Plan".to_owned())
        );
        assert_eq!(moved("Work2", ("Work", "Job"), Some('/')), None);
        assert_eq!(moved("Work/Q3", ("Work", "Job"), None), None);
        assert_eq!(moved("Other", ("Work", "Job"), Some('/')), None);
    }

    /// A server may answer NIL for INBOX alone (RFC 9051 section 7.3.1);
    /// the top level then takes the delimiter the other lines carry.
    #[test]
    fn the_top_level_takes_the_delimiter_of_the_other_lines_where_inbox_carries_none_rfc9051_7_3_1()
    {
        let delimiters = Delimiters::of(vec![
            listed("INBOX", None),
            listed("Work", Some('/')),
            listed("Work/Q3", Some('/')),
        ]);
        assert_eq!(delimiters.top, Some('/'));
        assert_eq!(delimiters.under("INBOX"), None);
        let flat = Delimiters::of(vec![listed("INBOX", None), listed("Work", None)]);
        assert_eq!(flat.top, None);
    }

    #[test]
    fn the_delimiters_follow_a_rename_and_a_create_rfc3501_6_3_8() {
        let mut delimiters = Delimiters::of(vec![
            listed("INBOX", Some('/')),
            listed("Shared", Some('.')),
            listed("Shared.Team", Some('.')),
        ]);
        delimiters.renamed(("Shared", "Ours"));
        assert_eq!(delimiters.under("Ours.Team"), Some('.'));
        assert_eq!(delimiters.under("Shared.Team"), Some('/'));
        delimiters.made("Ours.Team.New", Some('.'));
        assert_eq!(delimiters.under("Ours.Team.New"), Some('.'));
    }

    /// A wire name over the characters that matter to the mapping: both
    /// delimiters, a character of several bytes and plain ones.
    fn wire() -> impl Strategy<Value = String> {
        "[ab2/.台]{1,6}"
    }

    fn delimiter() -> impl Strategy<Value = Option<char>> {
        prop_oneof![Just(Some('/')), Just(Some('.')), Just(None)]
    }

    proptest! {
        #[test]
        fn the_renamed_name_takes_the_new_one_under_any_delimiter_rfc3501_6_3_5(
            from in wire(),
            to in wire(),
            delimiter in delimiter(),
        ) {
            prop_assert_eq!(moved(&from, (&from, &to), delimiter), Some(to));
        }

        #[test]
        fn a_name_under_the_renamed_one_keeps_what_follows_the_delimiter_rfc3501_6_3_5(
            from in wire(),
            to in wire(),
            rest in wire(),
            delimiter in prop::sample::select(vec!['/', '.']),
        ) {
            let under = format!("{from}{delimiter}{rest}");
            prop_assert_eq!(
                moved(&under, (&from, &to), Some(delimiter)),
                Some(format!("{to}{delimiter}{rest}"))
            );
        }

        #[test]
        fn a_name_that_shares_the_prefix_without_the_delimiter_stays_rfc3501_6_3_5(
            from in wire(),
            to in wire(),
            rest in wire(),
            delimiter in delimiter(),
        ) {
            prop_assume!(delimiter.is_none_or(|delimiter| !rest.starts_with(delimiter)));
            let beside = format!("{from}{rest}");
            prop_assert_eq!(moved(&beside, (&from, &to), delimiter), None);
        }

        #[test]
        fn a_name_outside_the_renamed_one_stays_rfc3501_6_3_5(
            wire in wire(),
            from in wire(),
            to in wire(),
            delimiter in delimiter(),
        ) {
            prop_assume!(!wire.starts_with(&from));
            prop_assert_eq!(moved(&wire, (&from, &to), delimiter), None);
        }
    }
}
