// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! `/changes` over the log (RFC 8620 section 5.2): the rows since a
//! state folded into the three lists, cut at a sequence when the client
//! caps the answer. A first sequence above the cap leaves no state to
//! cut at, which answers `cannotCalculateChanges`.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{Map, Value};

use super::{Context, MethodError, arguments};
use crate::store::{Change, ChangeKind, ObjectType};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ChangesArguments {
    account_id: String,
    since_state: String,
    max_changes: Option<u64>,
}

/// `Mailbox/changes` (RFC 8621 section 2.2): `updatedProperties` is
/// null, since the log names no property.
pub(super) fn mailbox(
    context: &Context<'_>,
    raw: &Map<String, Value>,
) -> Result<Map<String, Value>, MethodError> {
    let mut answer = changes(context, raw, ObjectType::Mailbox)?;
    answer.insert("updatedProperties".to_owned(), Value::Null);
    Ok(answer)
}

/// The `/changes` answer for one object type.
fn changes(
    context: &Context<'_>,
    raw: &Map<String, Value>,
    object: ObjectType,
) -> Result<Map<String, Value>, MethodError> {
    let arguments: ChangesArguments = arguments(raw)?;
    context.account(&arguments.account_id)?;
    if arguments.max_changes == Some(0) {
        return Err(MethodError::InvalidArguments(
            "maxChanges must be above zero",
        ));
    }
    // The log issues a state as the plain decimal text of its number.
    let since = arguments
        .since_state
        .parse::<u64>()
        .ok()
        .filter(|since| since.to_string() == arguments.since_state)
        .ok_or(MethodError::CannotCalculateChanges)?;
    let read = context.store.changes_since(context.key, object, since)?;
    let rows = read.changes.ok_or(MethodError::CannotCalculateChanges)?;
    let window = fold(&rows, arguments.max_changes, read.state)
        .ok_or(MethodError::CannotCalculateChanges)?;
    let answer = [
        ("accountId", Value::from(context.key.as_str())),
        ("oldState", Value::from(arguments.since_state)),
        ("newState", Value::from(window.new_state.to_string())),
        ("hasMoreChanges", Value::from(window.has_more)),
        ("created", Value::from(window.created)),
        ("updated", Value::from(window.updated)),
        ("destroyed", Value::from(window.destroyed)),
    ];
    Ok(answer
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect())
}

/// The three lists after the fold, the state they reach and whether the
/// log holds more.
#[derive(Debug, Default, PartialEq, Eq)]
struct Window {
    created: Vec<String>,
    updated: Vec<String>,
    destroyed: Vec<String>,
    new_state: u64,
    has_more: bool,
}

/// What one id went through since the old state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fate {
    Created,
    Updated,
    Destroyed,
    Gone,
}

impl Fate {
    fn start(kind: ChangeKind) -> Self {
        match kind {
            ChangeKind::Created => Self::Created,
            ChangeKind::Updated => Self::Updated,
            ChangeKind::Destroyed => Self::Destroyed,
        }
    }

    /// Whether one of the three lists names the id.
    fn listed(self) -> bool {
        self != Self::Gone
    }

    /// Created then updated stays created, created then destroyed leaves
    /// the answer (RFC 8620 section 5.2). A create after a destroy reads as
    /// created for an id that left, as updated for one the old state held.
    fn then(self, kind: ChangeKind) -> Self {
        match (self, kind) {
            (Self::Gone, ChangeKind::Created) => Self::Created,
            (Self::Gone, _) | (Self::Created, ChangeKind::Destroyed) => Self::Gone,
            (Self::Created, _) => Self::Created,
            (_, ChangeKind::Destroyed) => Self::Destroyed,
            (Self::Updated | Self::Destroyed, ChangeKind::Updated) => self,
            (Self::Updated | Self::Destroyed, ChangeKind::Created) => Self::Updated,
        }
    }
}

/// Folds the rows sequence by sequence; a sequence enters whole or not
/// at all. `None` when the first sequence alone lists more ids than
/// `max_changes`, since no state lies between the old one and it.
fn fold(rows: &[Change], max_changes: Option<u64>, current: u64) -> Option<Window> {
    let cut = max_changes.map_or(rows.len(), |max| cut(rows, max));
    let mut window = Window {
        new_state: current,
        ..Window::default()
    };
    if cut < rows.len() {
        // With no sequence entered there is no intermediate state.
        window.new_state = rows[..cut].last()?.sequence;
        window.has_more = true;
    }
    let mut fates = BTreeMap::new();
    for row in &rows[..cut] {
        advance(&mut fates, row);
    }
    for (id, fate) in fates {
        match fate {
            Fate::Created => window.created.push(id.to_owned()),
            Fate::Updated => window.updated.push(id.to_owned()),
            Fate::Destroyed => window.destroyed.push(id.to_owned()),
            Fate::Gone => {}
        }
    }
    Some(window)
}

/// Where the first sequence starts that carries the three lists past
/// `max` ids, the end of the rows when none does.
fn cut(rows: &[Change], max: u64) -> usize {
    let mut fates = BTreeMap::new();
    let mut listed = 0_u64;
    let mut start = 0;
    for (index, row) in rows.iter().enumerate() {
        if row.sequence != rows[start].sequence {
            start = index;
        }
        let (before, after) = advance(&mut fates, row);
        listed += u64::from(after.listed());
        listed -= u64::from(before.is_some_and(Fate::listed));
        let closes = rows
            .get(index + 1)
            .is_none_or(|next| next.sequence != row.sequence);
        if closes && listed > max {
            return start;
        }
    }
    rows.len()
}

/// Moves the fate of the row's id on by that row and answers the fate
/// before it and the fate after it.
fn advance<'rows>(
    fates: &mut BTreeMap<&'rows str, Fate>,
    row: &'rows Change,
) -> (Option<Fate>, Fate) {
    let before = fates.get(row.id.as_str()).copied();
    let after = before.map_or_else(|| Fate::start(row.kind), |fate| fate.then(row.kind));
    fates.insert(&row.id, after);
    (before, after)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(sequence: u64, id: &str, kind: ChangeKind) -> Change {
        Change {
            sequence,
            id: id.to_owned(),
            kind,
        }
    }

    #[test]
    fn the_fold_follows_rfc8620_5_2() {
        let rows = [
            change(1, "a", ChangeKind::Created),
            change(1, "b", ChangeKind::Created),
            change(2, "a", ChangeKind::Updated),
            change(2, "b", ChangeKind::Destroyed),
            change(2, "c", ChangeKind::Updated),
            change(3, "c", ChangeKind::Destroyed),
            change(3, "d", ChangeKind::Updated),
        ];
        let window = fold(&rows, None, 3).unwrap();
        assert_eq!(window.created, ["a"]);
        assert_eq!(window.updated, ["d"]);
        assert_eq!(window.destroyed, ["c"]);
        assert_eq!((window.new_state, window.has_more), (3, false));
    }

    #[test]
    fn an_id_created_again_after_it_left_is_created() {
        let rows = [
            change(1, "a", ChangeKind::Created),
            change(2, "a", ChangeKind::Destroyed),
            change(3, "a", ChangeKind::Created),
        ];
        let window = fold(&rows, None, 3).unwrap();
        assert_eq!(window.created, ["a"]);
        assert!(window.updated.is_empty() && window.destroyed.is_empty());
    }

    #[test]
    fn an_id_destroyed_then_created_is_updated() {
        let rows = [
            change(1, "a", ChangeKind::Destroyed),
            change(2, "a", ChangeKind::Created),
        ];
        let window = fold(&rows, None, 2).unwrap();
        assert_eq!(window.updated, ["a"]);
        assert!(window.created.is_empty() && window.destroyed.is_empty());
    }

    #[test]
    fn max_changes_cuts_at_a_sequence_and_a_first_one_above_it_has_no_window_rfc8620_5_2() {
        let rows = [
            change(1, "a", ChangeKind::Created),
            change(1, "b", ChangeKind::Created),
            change(2, "c", ChangeKind::Created),
            change(3, "d", ChangeKind::Created),
        ];
        assert_eq!(fold(&rows, Some(1), 3), None);
        let two = fold(&rows, Some(2), 3).unwrap();
        assert_eq!(two.created, ["a", "b"]);
        assert_eq!((two.new_state, two.has_more), (1, true));
        let three = fold(&rows, Some(3), 3).unwrap();
        assert_eq!(three.created, ["a", "b", "c"]);
        assert_eq!((three.new_state, three.has_more), (2, true));
        let four = fold(&rows, Some(4), 3).unwrap();
        assert_eq!((four.new_state, four.has_more), (3, false));
        let uncapped = fold(&rows, None, 3).unwrap();
        assert_eq!(uncapped.created, ["a", "b", "c", "d"]);
        assert_eq!((uncapped.new_state, uncapped.has_more), (3, false));
        let empty = fold(&[], Some(1), 7).unwrap();
        assert_eq!((empty.new_state, empty.has_more), (7, false));
    }
}
