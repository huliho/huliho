// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Result references (RFC 8620 section 3.7): an argument written as
//! `#name` points into an earlier response through a JSON pointer.

use std::cell::Cell;

use serde::Deserialize;
use serde_json::{Map, Value};

use super::{Invocation, MAX_SIZE_REQUEST, MethodError};

/// The weight every result reference of one request may resolve to in
/// total. The size of the largest request is ample for the id lists
/// that references carry.
const MAX_REFERENCE_WEIGHT: usize = MAX_SIZE_REQUEST;

/// What a node weighs on top of its text: the size of a `Value`, which
/// keeps a weight close to the memory a copy takes.
const NODE_WEIGHT: usize = size_of::<Value>();

const NOTHING_THERE: MethodError =
    MethodError::InvalidResultReference("the path of a reference points at nothing");

const WRONG_NAME: MethodError =
    MethodError::InvalidResultReference("a reference names another method than the response");

const OVER_BUDGET: MethodError = MethodError::InvalidResultReference(
    "the references of the request resolve to more than the server allows",
);

/// What the result references of one request may still resolve to.
pub(super) struct Budget(Cell<usize>);

impl Budget {
    pub(super) fn full() -> Self {
        Self(Cell::new(MAX_REFERENCE_WEIGHT))
    }

    /// Takes the weight of `value`: `NODE_WEIGHT` for every node plus
    /// the bytes of every string and key. A value past the budget fails
    /// at the node that does not fit; what was taken stays taken.
    fn charge(&self, value: &Value) -> Result<(), MethodError> {
        match value {
            Value::String(text) => self.take(NODE_WEIGHT + text.len()),
            Value::Array(items) => {
                self.take(NODE_WEIGHT)?;
                items.iter().try_for_each(|item| self.charge(item))
            }
            Value::Object(map) => self.charge_object(map),
            _ => self.take(NODE_WEIGHT),
        }
    }

    fn charge_object(&self, map: &Map<String, Value>) -> Result<(), MethodError> {
        self.take(NODE_WEIGHT)?;
        map.iter().try_for_each(|(key, item)| {
            self.take(key.len())?;
            self.charge(item)
        })
    }

    fn take(&self, weight: usize) -> Result<(), MethodError> {
        let left = self.0.get().checked_sub(weight).ok_or(OVER_BUDGET)?;
        self.0.set(left);
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResultReference {
    result_of: String,
    name: String,
    path: String,
}

/// Replaces every `#name` argument by the value its reference points
/// at, charged to the budget before it is copied. A reference that
/// points at nothing or passes the budget is `invalidResultReference`;
/// an argument given both plainly and as a reference is
/// `invalidArguments`.
pub(super) fn resolve(
    arguments: &Map<String, Value>,
    previous: &[Invocation],
    budget: &Budget,
) -> Result<Map<String, Value>, MethodError> {
    let mut resolved = Map::new();
    for (name, value) in arguments {
        let Some(plain) = name.strip_prefix('#') else {
            resolved.insert(name.clone(), value.clone());
            continue;
        };
        if arguments.contains_key(plain) {
            return Err(MethodError::InvalidArguments(
                "an argument was given both plainly and as a reference",
            ));
        }
        let reference: ResultReference = serde_json::from_value(value.clone()).map_err(|_| {
            MethodError::InvalidResultReference("a reference is not a ResultReference object")
        })?;
        // RFC 8620 section 3.7: the first response under the call id, then its name.
        let Invocation(method, response, _) = previous
            .iter()
            .find(|Invocation(_, _, id)| *id == reference.result_of)
            .ok_or(MethodError::InvalidResultReference(
                "a reference names no earlier response",
            ))?;
        if *method != reference.name {
            return Err(WRONG_NAME);
        }
        let found = pointer(response, &reference.path, budget)?;
        resolved.insert(plain.to_owned(), found);
    }
    Ok(resolved)
}

/// A JSON pointer (RFC 6901) into a response, with the `*` of RFC 8620
/// section 3.7: the rest of the path applies to every item of an array
/// and arrays among the results flatten once.
fn pointer(
    response: &Map<String, Value>,
    path: &str,
    budget: &Budget,
) -> Result<Value, MethodError> {
    if path.is_empty() {
        budget.charge_object(response)?;
        return Ok(Value::Object(response.clone()));
    }
    let tokens: Vec<String> = path
        .strip_prefix('/')
        .ok_or(NOTHING_THERE)?
        .split('/')
        .map(|token| token.replace("~1", "/").replace("~0", "~"))
        .collect();
    let (first, rest) = tokens.split_first().ok_or(NOTHING_THERE)?;
    let value = response.get(first.as_str()).ok_or(NOTHING_THERE)?;
    walk(value, rest, budget)
}

/// Every copy is charged first: a target before its clone, the array a
/// `*` builds before its first item.
fn walk(value: &Value, tokens: &[String], budget: &Budget) -> Result<Value, MethodError> {
    let Some((token, rest)) = tokens.split_first() else {
        budget.charge(value)?;
        return Ok(value.clone());
    };
    let next = match value {
        Value::Array(items) if token == "*" => {
            budget.take(NODE_WEIGHT)?;
            let mut out = Vec::new();
            for item in items {
                match walk(item, rest, budget)? {
                    Value::Array(inner) => out.extend(inner),
                    other => out.push(other),
                }
            }
            return Ok(Value::Array(out));
        }
        // RFC 6901 section 4: an index is `0` or digits without a leading zero.
        Value::Array(items) => token
            .parse()
            .ok()
            .filter(|index: &usize| index.to_string() == *token)
            .and_then(|index| items.get(index)),
        Value::Object(map) => map.get(token.as_str()),
        _ => None,
    };
    walk(next.ok_or(NOTHING_THERE)?, rest, budget)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn earlier() -> Vec<Invocation> {
        let Value::Object(query) = json!({ "ids": ["e1", "e2"] }) else {
            unreachable!()
        };
        let Value::Object(get) = json!({
            "list": [
                { "id": "e1", "threadId": "t1", "emailIds": ["e1", "e3"] },
                { "id": "e2", "threadId": "t2", "emailIds": ["e2"] }
            ]
        }) else {
            unreachable!()
        };
        vec![
            Invocation("Email/query".to_owned(), query, "q".to_owned()),
            Invocation("Email/get".to_owned(), get, "g".to_owned()),
        ]
    }

    fn reference(result_of: &str, name: &str, path: &str) -> Map<String, Value> {
        let Value::Object(arguments) = json!({
            "accountId": "a1",
            "#ids": { "resultOf": result_of, "name": name, "path": path }
        }) else {
            unreachable!()
        };
        arguments
    }

    #[test]
    fn the_three_paths_resolve_and_a_fourth_is_refused_rfc8620_3_7() {
        let previous = earlier();
        let budget = Budget::full();
        let resolved = |path| resolve(&reference("g", "Email/get", path), &previous, &budget);
        let ids = resolve(&reference("q", "Email/query", "/ids"), &previous, &budget).unwrap();
        assert_eq!(ids["ids"], json!(["e1", "e2"]));
        assert_eq!(ids["accountId"], "a1");
        assert_eq!(ids.get("#ids"), None);
        let threads = resolved("/list/*/threadId").unwrap();
        assert_eq!(threads["ids"], json!(["t1", "t2"]));
        let emails = resolved("/list/*/emailIds").unwrap();
        assert_eq!(emails["ids"], json!(["e1", "e3", "e2"]));
        assert_eq!(resolved("/list/*/nope"), Err(NOTHING_THERE));
    }

    #[test]
    fn an_unknown_call_a_wrong_name_or_a_broken_reference_is_refused() {
        let previous = earlier();
        let budget = Budget::full();
        for (result_of, name) in [("x", "Email/query"), ("q", "Email/get")] {
            let outcome = resolve(&reference(result_of, name, "/ids"), &previous, &budget);
            assert!(
                matches!(outcome, Err(MethodError::InvalidResultReference(_))),
                "{result_of} {name}"
            );
        }
        let Value::Object(odd) = json!({ "#ids": { "resultOf": "q" } }) else {
            unreachable!()
        };
        assert!(matches!(
            resolve(&odd, &previous, &budget),
            Err(MethodError::InvalidResultReference(_))
        ));
    }

    #[test]
    fn a_call_id_used_twice_resolves_to_its_first_response_alone_rfc8620_3_7() {
        let mut previous = earlier();
        previous[1].2 = "q".to_owned();
        let budget = Budget::full();
        let first = resolve(&reference("q", "Email/query", "/ids"), &previous, &budget);
        assert_eq!(first.unwrap()["ids"], json!(["e1", "e2"]));
        let second = resolve(&reference("q", "Email/get", "/list"), &previous, &budget);
        assert_eq!(second, Err(WRONG_NAME));
    }

    #[test]
    fn both_forms_of_one_argument_are_invalid_arguments() {
        let mut both = reference("q", "Email/query", "/ids");
        both.insert("ids".to_owned(), json!([]));
        assert!(matches!(
            resolve(&both, &earlier(), &Budget::full()),
            Err(MethodError::InvalidArguments(_))
        ));
    }

    #[test]
    fn the_pointer_walks_indexes_escapes_and_the_root_rfc6901() {
        let Value::Object(value) = json!({ "a/b": [ { "~": 1 }, { "~": 2 } ], "list": [] }) else {
            unreachable!()
        };
        let budget = Budget::full();
        assert_eq!(pointer(&value, "/a~1b/0/~0", &budget), Ok(json!(1)));
        assert_eq!(pointer(&value, "/a~1b/1/~0", &budget), Ok(json!(2)));
        assert_eq!(pointer(&value, "/a~1b/*/~0", &budget), Ok(json!([1, 2])));
        for refused in ["/a~1b/9", "/a~1b/+1", "/a~1b/01", "/a~1b/0/~0/*"] {
            assert_eq!(
                pointer(&value, refused, &budget),
                Err(NOTHING_THERE),
                "{refused}"
            );
        }
        assert_eq!(pointer(&value, "/list/*/x", &budget), Ok(json!([])));
        assert_eq!(pointer(&value, "a", &budget), Err(NOTHING_THERE));
        let whole = Value::Object(value.clone());
        assert_eq!(pointer(&value, "", &budget), Ok(whole));
        let Value::Object(nested) = json!({
            "l": [ { "m": [ { "x": 1 }, { "x": 2 } ] }, { "m": [ { "x": 3 } ] } ]
        }) else {
            unreachable!()
        };
        assert_eq!(
            pointer(&nested, "/l/*/m/*/x", &budget),
            Ok(json!([1, 2, 3]))
        );
    }

    #[test]
    fn a_value_weighs_its_nodes_its_strings_and_its_keys() {
        let budget = Budget::full();
        budget.charge(&json!(["ab", { "k": null }])).unwrap();
        let weight = MAX_REFERENCE_WEIGHT - budget.0.get();
        assert_eq!(weight, 4 * NODE_WEIGHT + "ab".len() + "k".len());
    }

    #[test]
    fn a_star_walk_fits_the_budget_to_the_byte_and_stops_at_the_item_past_it() {
        let previous = earlier();
        let emails = reference("g", "Email/get", "/list/*/emailIds");
        let full = Budget::full();
        resolve(&emails, &previous, &full).unwrap();
        let weight = MAX_REFERENCE_WEIGHT - full.0.get();
        let exact = Budget(Cell::new(weight));
        let resolved = resolve(&emails, &previous, &exact).unwrap();
        assert_eq!(resolved["ids"], json!(["e1", "e3", "e2"]));
        assert_eq!(exact.0.get(), 0);
        let short = Budget(Cell::new(weight - 1));
        assert_eq!(resolve(&emails, &previous, &short), Err(OVER_BUDGET));
        assert_eq!(short.0.get(), NODE_WEIGHT + "e2".len() - 1);
    }
}
