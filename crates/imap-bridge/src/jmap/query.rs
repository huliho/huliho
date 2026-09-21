// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! `Email/query` (RFC 8621 section 4.4) over the memberships of one
//! mailbox: the filter `inMailbox`, the sort `receivedAt` either way, a
//! window by position or by anchor, the total on request and one email
//! per thread on request. Any other filter or sort is refused as such.

use serde::Deserialize;
use serde_json::{Map, Value};

use super::{Context, MethodError, arguments};
use crate::store::{MailboxId, Query, Start, Window};

/// The most ids one `Email/query` answers; a larger or absent `limit`
/// is lowered to it and the answer says so (RFC 8620 section 5.5).
pub const QUERY_LIMIT: u32 = 200;

/// The one property a comparator may name.
const RECEIVED_AT: &str = "receivedAt";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QueryArguments {
    account_id: String,
    filter: Option<Value>,
    sort: Option<Vec<Comparator>>,
    #[serde(default)]
    position: i64,
    anchor: Option<String>,
    #[serde(default)]
    anchor_offset: i64,
    limit: Option<u64>,
    #[serde(default)]
    calculate_total: bool,
    #[serde(default)]
    collapse_threads: bool,
}

/// One comparator (RFC 8620 section 5.5); a collation is refused, since
/// a date has none.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Comparator {
    property: String,
    #[serde(default = "ascending_by_default")]
    is_ascending: bool,
    collation: Option<String>,
}

fn ascending_by_default() -> bool {
    true
}

/// The one filter shape served: a `FilterCondition` with `inMailbox`
/// alone (RFC 8621 section 4.4.1).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InMailbox {
    in_mailbox: String,
}

/// `Email/query` (RFC 8621 section 4.4).
pub(super) fn email(
    context: &Context<'_>,
    raw: &Map<String, Value>,
) -> Result<Map<String, Value>, MethodError> {
    let query_arguments: QueryArguments = arguments(raw)?;
    context.account(&query_arguments.account_id)?;
    let mailbox = mailbox_of(query_arguments.filter.as_ref())?;
    let ascending = ascending(query_arguments.sort.as_deref())?;
    let (limit, lowered) = limit(query_arguments.limit);
    let start = match &query_arguments.anchor {
        // RFC 8620 section 5.5: with an anchor the position is ignored.
        Some(id) => Start::Anchor {
            id,
            offset: query_arguments.anchor_offset,
        },
        None => Start::Position(query_arguments.position),
    };
    let query = Query {
        mailbox: &mailbox,
        ascending,
        collapse_threads: query_arguments.collapse_threads,
    };
    let window = Window {
        start,
        limit,
        calculate_total: query_arguments.calculate_total,
    };
    let queried = context
        .store
        .query(context.key, &query, &window)?
        .ok_or(MethodError::AnchorNotFound)?;
    *context.viewed.borrow_mut() = Some(MailboxId::from(mailbox));
    let mut answer = Map::new();
    answer.insert("accountId".to_owned(), Value::from(context.key.as_str()));
    answer.insert(
        "queryState".to_owned(),
        Value::from(queried.state.to_string()),
    );
    answer.insert("canCalculateChanges".to_owned(), Value::Bool(false));
    answer.insert("position".to_owned(), Value::from(queried.position));
    answer.insert(
        "ids".to_owned(),
        Value::Array(
            queried
                .ids
                .iter()
                .map(|id| Value::from(id.as_str()))
                .collect(),
        ),
    );
    if let Some(total) = queried.total {
        answer.insert("total".to_owned(), Value::from(total));
    }
    if lowered {
        answer.insert("limit".to_owned(), Value::from(QUERY_LIMIT));
    }
    Ok(answer)
}

/// The mailbox the filter names; anything else is `unsupportedFilter`,
/// a missing filter included, since no index orders a whole account.
fn mailbox_of(filter: Option<&Value>) -> Result<String, MethodError> {
    let filter = filter.ok_or(MethodError::UnsupportedFilter)?;
    let condition: InMailbox =
        serde_json::from_value(filter.clone()).map_err(|_| MethodError::UnsupportedFilter)?;
    Ok(condition.in_mailbox)
}

/// Whether the result runs oldest first: no sort or one comparator on
/// `receivedAt` without a collation; anything else is `unsupportedSort`.
fn ascending(sort: Option<&[Comparator]>) -> Result<bool, MethodError> {
    match sort {
        None | Some([]) => Ok(false),
        Some([one]) if one.property == RECEIVED_AT && one.collation.is_none() => {
            Ok(one.is_ascending)
        }
        Some(_) => Err(MethodError::UnsupportedSort),
    }
}

/// The limit the query runs with and whether it was lowered.
fn limit(asked: Option<u64>) -> (u32, bool) {
    match asked {
        Some(asked) if asked <= u64::from(QUERY_LIMIT) => {
            (u32::try_from(asked).unwrap_or(QUERY_LIMIT), false)
        }
        _ => (QUERY_LIMIT, true),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn comparators(value: Value) -> Vec<Comparator> {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn the_filter_is_in_mailbox_alone_rfc8621_4_4_1() {
        assert_eq!(
            mailbox_of(Some(&json!({ "inMailbox": "m1" }))).unwrap(),
            "m1"
        );
        for refused in [
            json!(null),
            json!({}),
            json!({ "inMailbox": "m1", "hasKeyword": "$seen" }),
            json!({ "operator": "AND", "conditions": [{ "inMailbox": "m1" }] }),
            json!({ "inMailbox": 7 }),
            json!("m1"),
        ] {
            assert_eq!(
                mailbox_of(Some(&refused)),
                Err(MethodError::UnsupportedFilter),
                "{refused}"
            );
        }
        assert_eq!(mailbox_of(None), Err(MethodError::UnsupportedFilter));
    }

    #[test]
    fn the_sort_is_received_at_either_way_rfc8620_5_5() {
        assert!(!ascending(None).unwrap());
        assert!(!ascending(Some(&[])).unwrap());
        let newest = comparators(json!([{ "property": "receivedAt", "isAscending": false }]));
        assert!(!ascending(Some(&newest)).unwrap());
        let oldest = comparators(json!([{ "property": "receivedAt" }]));
        assert!(ascending(Some(&oldest)).unwrap());
        for refused in [
            json!([{ "property": "subject" }]),
            json!([{ "property": "receivedAt" }, { "property": "size" }]),
            json!([{ "property": "receivedAt", "collation": "i;unicode-casemap" }]),
        ] {
            let sort = comparators(refused.clone());
            assert_eq!(
                ascending(Some(&sort)),
                Err(MethodError::UnsupportedSort),
                "{refused}"
            );
        }
        assert!(serde_json::from_value::<Comparator>(json!({ "isAscending": true })).is_err());
    }

    #[test]
    fn the_limit_is_lowered_to_the_server_s_and_the_answer_says_so_rfc8620_5_5() {
        assert_eq!(limit(Some(50)), (50, false));
        assert_eq!(limit(Some(u64::from(QUERY_LIMIT))), (QUERY_LIMIT, false));
        assert_eq!(limit(Some(u64::from(QUERY_LIMIT) + 1)), (QUERY_LIMIT, true));
        assert_eq!(limit(None), (QUERY_LIMIT, true));
        assert_eq!(limit(Some(0)), (0, false));
    }
}
