// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! `Thread/get` (RFC 8621 section 3.1) over the rows: the emails of
//! each thread by `receivedAt`, oldest first, ties by id.

use std::collections::HashSet;

use serde_json::{Map, Value, json};

use super::get::{self, GetArguments};
use super::{Context, MethodError, arguments};

/// The two properties a Thread has (RFC 8621 section 3).
const PROPERTIES: [&str; 2] = ["id", "emailIds"];

/// `Thread/get`: the named threads, each cut to the wanted properties,
/// with the ids that name nothing.
pub(super) fn get(
    context: &Context<'_>,
    raw: &Map<String, Value>,
) -> Result<Map<String, Value>, MethodError> {
    let arguments: GetArguments = arguments(raw)?;
    context.account(&arguments.account_id)?;
    let ids = get::ids(arguments.ids.as_deref())?;
    let wanted = get::properties(&PROPERTIES, arguments.properties.as_deref())?;
    let snapshot = context.store.threads(context.key, &ids)?;
    let found: HashSet<&str> = snapshot.threads.iter().map(|(id, _)| id.as_str()).collect();
    let list: Vec<Value> = snapshot
        .threads
        .iter()
        .map(|(id, emails)| {
            let pairs = [("id", json!(id)), ("emailIds", json!(emails))];
            pairs
                .into_iter()
                .filter(|(name, _)| wanted.contains(name))
                .map(|(name, value)| (name.to_owned(), value))
                .collect::<Map<_, _>>()
                .into()
        })
        .collect();
    Ok(get::answer(context, (snapshot.state, list), &ids, &found))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thread_has_two_properties_and_id_always_rides_along_rfc8621_3() {
        assert_eq!(
            get::properties(&PROPERTIES, None).unwrap(),
            ["id", "emailIds"]
        );
        let named = ["emailIds".to_owned(), "emailIds".to_owned()];
        assert_eq!(
            get::properties(&PROPERTIES, Some(&named)).unwrap(),
            ["id", "emailIds"]
        );
        assert!(matches!(
            get::properties(&PROPERTIES, Some(&["subject".to_owned()])),
            Err(MethodError::InvalidArguments(_))
        ));
    }
}
