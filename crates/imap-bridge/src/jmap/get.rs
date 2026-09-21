// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What `Email/get` and `Thread/get` share (RFC 8620 section 5.1): the
//! arguments, the ids of a `/get` that cannot answer for every object at
//! once, the properties it renders and the shape of its answer.

use std::collections::HashSet;

use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::{Context, MAX_OBJECTS_IN_GET, MethodError};

/// The arguments of a `/get` (RFC 8620 section 5.1).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct GetArguments {
    pub account_id: String,
    pub ids: Option<Vec<String>>,
    pub properties: Option<Vec<String>>,
}

/// The ids asked for, each once. Every object at once is more than one
/// answer holds, so null `ids` is `requestTooLarge`, as is a list past
/// `MAX_OBJECTS_IN_GET`.
pub(super) fn ids(ids: Option<&[String]>) -> Result<Vec<&str>, MethodError> {
    let ids = ids.ok_or(MethodError::RequestTooLarge)?;
    if ids.len() > MAX_OBJECTS_IN_GET {
        return Err(MethodError::RequestTooLarge);
    }
    // RFC 8620 section 5.1: an id sent more than once is answered once.
    let mut seen = HashSet::new();
    Ok(ids
        .iter()
        .map(String::as_str)
        .filter(|id| seen.insert(*id))
        .collect())
}

/// Every known property when none is named, the named ones plus `id`
/// otherwise; an unknown one is `invalidArguments`.
pub(super) fn properties(
    known: &'static [&'static str],
    named: Option<&[String]>,
) -> Result<Vec<&'static str>, MethodError> {
    let Some(named) = named else {
        return Ok(known.to_vec());
    };
    let mut wanted = vec!["id"];
    for name in named {
        let property = known.iter().copied().find(|known| *known == name).ok_or(
            MethodError::InvalidArguments("an unknown property was asked for"),
        )?;
        if !wanted.contains(&property) {
            wanted.push(property);
        }
    }
    Ok(wanted)
}

/// The answer of a `/get`: the account, the state the list belongs to,
/// the objects found and the asked ids that name nothing, in the order
/// asked.
pub(super) fn answer(
    context: &Context<'_>,
    (state, list): (u64, Vec<Value>),
    asked: &[&str],
    found: &HashSet<&str>,
) -> Map<String, Value> {
    let not_found: Vec<&str> = asked
        .iter()
        .copied()
        .filter(|id| !found.contains(id))
        .collect();
    let mut answer = Map::new();
    answer.insert("accountId".to_owned(), Value::from(context.key.as_str()));
    answer.insert("state".to_owned(), Value::from(state.to_string()));
    answer.insert("list".to_owned(), Value::Array(list));
    answer.insert("notFound".to_owned(), json!(not_found));
    answer
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ids_are_answered_once_and_null_or_too_many_are_too_large_rfc8620_5_1() {
        let twice = ["a".to_owned(), "b".to_owned(), "a".to_owned()];
        assert_eq!(ids(Some(&twice)).unwrap(), ["a", "b"]);
        assert_eq!(ids(None), Err(MethodError::RequestTooLarge));
        let many = vec!["x".to_owned(); MAX_OBJECTS_IN_GET + 1];
        assert_eq!(ids(Some(&many)), Err(MethodError::RequestTooLarge));
        assert!(ids(Some(&[])).unwrap().is_empty());
    }
}
