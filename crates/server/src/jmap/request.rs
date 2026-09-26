// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What the proxy reads of a Request object before it forwards it:
//! whether an `Email/get` asks for body values, so the answer takes the
//! sanitizer pass, and whether that ask is one the pass can serve.

use serde_json::{Map, Value};
use thiserror::Error;

const EMAIL_GET: &str = "Email/get";
const EMAIL_PARSE: &str = "Email/parse";
const HTML_BODY: &str = "htmlBody";
const BODY_VALUES: &str = "bodyValues";
const FETCH_ALL: &str = "fetchAllBodyValues";

/// The arguments that put values into `bodyValues` (RFC 8621 section
/// 4.2).
const FETCH_FLAGS: [&str; 3] = ["fetchTextBodyValues", "fetchHTMLBodyValues", FETCH_ALL];

/// The arguments a result reference may stand in for (RFC 8620 section
/// 3.7); one of them referenced hides from the pass what the server
/// will answer.
const REFERENCED: [&str; 4] = [
    "#properties",
    "#fetchTextBodyValues",
    "#fetchHTMLBodyValues",
    "#fetchAllBodyValues",
];

/// What the request wants of its answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wants {
    /// Headers, ids and states: the answer passes as the server sent it.
    Headers,
    /// Body values: the answer takes the sanitizer pass.
    Bodies,
}

/// A body ask the pass cannot serve. Without `htmlBody` in the
/// `properties` list the pass cannot tell which values are HTML; with
/// `fetchAllBodyValues` a `text/html` attachment outside the body lists
/// would arrive too; with a result reference in place of one of those
/// arguments the pass cannot read the ask at all; an `Email/parse`
/// (RFC 8621 section 4.9) would carry body values the pass does not
/// walk.
#[derive(Debug, Error, PartialEq, Eq)]
#[error("the body request cannot be sanitized")]
pub struct Refused;

/// What the request wants; bytes that are no Request object want
/// headers, since no server runs a call of theirs. A duplicate key
/// counts by its last value, as the servers behind the proxy read it.
///
/// # Errors
///
/// Returns [`Refused`] for a body ask the pass cannot serve.
pub fn inspect(body: &[u8]) -> Result<Wants, Refused> {
    let Ok(request) = serde_json::from_slice::<Value>(body) else {
        return Ok(Wants::Headers);
    };
    let Some(calls) = request.get("methodCalls").and_then(Value::as_array) else {
        return Ok(Wants::Headers);
    };
    let mut wants = Wants::Headers;
    for (name, arguments) in calls.iter().filter_map(invocation) {
        let bodies = asks_bodies(arguments) || references_bodies(arguments);
        match name {
            EMAIL_GET if bodies => {
                if lacks_html_body(arguments)
                    || is_set(arguments.get(FETCH_ALL))
                    || references_bodies(arguments)
                {
                    return Err(Refused);
                }
                wants = Wants::Bodies;
            }
            EMAIL_PARSE if bodies => return Err(Refused),
            _ => {}
        }
    }
    Ok(wants)
}

/// A well-formed invocation (RFC 8620 section 3.2): its name and its
/// arguments; any other call is the server's to judge.
fn invocation(call: &Value) -> Option<(&str, &Map<String, Value>)> {
    let [name, arguments, _] = call.as_array()?.as_slice() else {
        return None;
    };
    Some((name.as_str()?, arguments.as_object()?))
}

/// Whether the answer will carry `bodyValues`: a fetch flag is set or
/// the property is asked. An omitted or null `properties` list asks
/// for the default set of RFC 8621 section 4.2, which holds it.
fn asks_bodies(arguments: &Map<String, Value>) -> bool {
    FETCH_FLAGS.iter().any(|flag| is_set(arguments.get(*flag)))
        || properties(arguments).is_none_or(|list| names(list, BODY_VALUES))
}

/// Whether a result reference stands in for a body argument.
fn references_bodies(arguments: &Map<String, Value>) -> bool {
    REFERENCED.iter().any(|key| arguments.contains_key(*key))
}

/// Whether the pass could not find the HTML parts: a `properties` list
/// without `htmlBody`, or one of a shape the pass cannot read.
fn lacks_html_body(arguments: &Map<String, Value>) -> bool {
    match arguments.get("properties") {
        None | Some(Value::Null) => false,
        Some(Value::Array(list)) => !names(list, HTML_BODY),
        Some(_) => true,
    }
}

/// The `properties` list; `None` for an omitted or null one.
fn properties(arguments: &Map<String, Value>) -> Option<&Vec<Value>> {
    arguments.get("properties").and_then(Value::as_array)
}

fn names(list: &[Value], property: &str) -> bool {
    list.iter().any(|name| name.as_str() == Some(property))
}

/// A flag set to anything but absent, null or false counts as set, so
/// a lenient server never runs an ask the pass did not see.
fn is_set(value: Option<&Value>) -> bool {
    !matches!(value, None | Some(Value::Null | Value::Bool(false)))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn request(calls: &Value) -> Vec<u8> {
        json!({ "using": ["urn:ietf:params:jmap:core"], "methodCalls": calls })
            .to_string()
            .into_bytes()
    }

    fn get(arguments: &Value) -> Value {
        json!(["Email/get", arguments, "c1"])
    }

    fn query() -> Value {
        json!(["Email/query", { "accountId": "u1", "limit": 10 }, "c0"])
    }

    #[test]
    fn a_query_and_a_header_window_want_headers() {
        let header_window =
            get(&json!({ "accountId": "u1", "ids": ["e1"], "properties": ["subject", "preview"] }));
        let referenced = get(&json!({
            "accountId": "u1",
            "#ids": { "resultOf": "c0", "name": "Email/query", "path": "/ids" },
            "properties": ["subject", "preview"]
        }));
        for calls in [
            json!([query()]),
            json!([query(), header_window]),
            json!([query(), referenced]),
        ] {
            assert_eq!(inspect(&request(&calls)), Ok(Wants::Headers), "{calls}");
        }
    }

    #[test]
    fn a_fetch_flag_body_values_or_an_omitted_list_wants_bodies() {
        for arguments in [
            json!({ "ids": ["e1"], "properties": ["htmlBody", "bodyValues"], "fetchHTMLBodyValues": true }),
            json!({ "ids": ["e1"], "properties": ["htmlBody"], "fetchTextBodyValues": true }),
            json!({ "ids": ["e1"], "properties": ["htmlBody", "bodyValues"] }),
            json!({ "ids": ["e1"] }),
            json!({ "ids": ["e1"], "properties": null }),
            json!({ "ids": ["e1"], "fetchAllBodyValues": false, "fetchHTMLBodyValues": true }),
            json!({
                "#ids": { "resultOf": "c0", "name": "Email/query", "path": "/ids" },
                "properties": ["htmlBody", "bodyValues"],
                "fetchHTMLBodyValues": true
            }),
        ] {
            let calls = json!([query(), get(&arguments)]);
            assert_eq!(inspect(&request(&calls)), Ok(Wants::Bodies), "{arguments}");
        }
    }

    #[test]
    fn a_body_ask_without_html_body_or_with_fetch_all_is_refused() {
        for arguments in [
            json!({ "ids": ["e1"], "properties": ["textBody", "bodyValues"], "fetchTextBodyValues": true }),
            json!({ "ids": ["e1"], "properties": ["subject"], "fetchHTMLBodyValues": true }),
            json!({ "ids": ["e1"], "properties": ["bodyValues"] }),
            json!({ "ids": ["e1"], "fetchAllBodyValues": true }),
            json!({ "ids": ["e1"], "properties": ["htmlBody"], "fetchAllBodyValues": "yes" }),
            json!({ "ids": ["e1"], "properties": "htmlBody", "fetchHTMLBodyValues": true }),
        ] {
            let calls = json!([query(), get(&arguments)]);
            assert_eq!(inspect(&request(&calls)), Err(Refused), "{arguments}");
        }
    }

    #[test]
    fn a_result_reference_in_place_of_a_body_argument_is_refused_rfc8620_3_7() {
        let reference = json!({ "resultOf": "c0", "name": "Core/echo", "path": "/list" });
        for arguments in [
            json!({ "ids": ["e1"], "#properties": reference, "fetchHTMLBodyValues": true }),
            json!({ "ids": ["e1"], "#properties": reference }),
            json!({ "ids": ["e1"], "properties": ["htmlBody"], "#fetchHTMLBodyValues": reference }),
            json!({ "ids": ["e1"], "properties": ["htmlBody"], "#fetchTextBodyValues": reference }),
            json!({ "ids": ["e1"], "properties": ["htmlBody"], "#fetchAllBodyValues": reference }),
        ] {
            let calls = json!([query(), get(&arguments)]);
            assert_eq!(inspect(&request(&calls)), Err(Refused), "{arguments}");
        }
    }

    #[test]
    fn an_email_parse_that_would_carry_body_values_is_refused_rfc8621_4_9() {
        let parse = |arguments: Value| json!(["Email/parse", arguments, "c1"]);
        for arguments in [
            json!({ "accountId": "u1", "blobIds": ["b1"], "fetchHTMLBodyValues": true }),
            json!({ "accountId": "u1", "blobIds": ["b1"], "properties": ["bodyValues"] }),
            json!({ "accountId": "u1", "blobIds": ["b1"] }),
            json!({ "accountId": "u1", "blobIds": ["b1"], "properties": ["subject"], "#fetchHTMLBodyValues": {} }),
        ] {
            let calls = json!([parse(arguments.clone())]);
            assert_eq!(inspect(&request(&calls)), Err(Refused), "{arguments}");
        }
        let headers =
            parse(json!({ "accountId": "u1", "blobIds": ["b1"], "properties": ["subject"] }));
        assert_eq!(inspect(&request(&json!([headers]))), Ok(Wants::Headers));
    }

    #[test]
    fn a_flag_set_to_null_or_false_is_not_set() {
        assert!(!is_set(None));
        assert!(!is_set(Some(&Value::Null)));
        assert!(!is_set(Some(&Value::Bool(false))));
        assert!(is_set(Some(&Value::Bool(true))));
        assert!(is_set(Some(&json!(1))));
        assert!(is_set(Some(&json!("false"))));
    }

    #[test]
    fn what_is_not_a_request_object_wants_headers_and_a_malformed_call_is_skipped() {
        for body in [
            &b"[]"[..],
            b"{}",
            b"{\"methodCalls\": \"none\"}",
            b"not json",
            b"",
        ] {
            assert_eq!(inspect(body), Ok(Wants::Headers));
        }
        let calls = json!([
            ["Email/get"],
            ["Email/get", [], "c1"],
            "x",
            5,
            [5, {}, "c2"]
        ]);
        assert_eq!(inspect(&request(&calls)), Ok(Wants::Headers));
        let calls = json!([
            ["Email/get"],
            get(&json!({ "ids": ["e1"], "fetchHTMLBodyValues": true }))
        ]);
        assert_eq!(inspect(&request(&calls)), Ok(Wants::Bodies));
    }

    #[test]
    fn a_duplicate_key_counts_by_its_last_value() {
        let header_window = get(&json!({ "ids": ["e1"], "properties": ["subject"] }));
        let body_ask = get(&json!({ "ids": ["e1"], "fetchHTMLBodyValues": true }));
        let twice =
            format!("{{\"methodCalls\": [{header_window}], \"methodCalls\": [{body_ask}]}}");
        assert_eq!(inspect(twice.as_bytes()), Ok(Wants::Bodies));
        let twice =
            format!("{{\"methodCalls\": [{body_ask}], \"methodCalls\": [{header_window}]}}");
        assert_eq!(inspect(twice.as_bytes()), Ok(Wants::Headers));
    }
}
