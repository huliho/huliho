// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The pass over an `Email/get` answer that carries body values: every
//! `text/html` value that `htmlBody` names goes through the sanitizer
//! and every other value of the answer passes as the server sent it.

use serde_json::{Map, Value};
use thiserror::Error;

use super::sanitize::Sanitizer;

const EMAIL_GET: &str = "Email/get";
const HTML: &str = "text/html";

/// An answer that is not a Response object (RFC 8620 section 3.4), so
/// the pass has nothing to walk.
#[derive(Debug, Error, PartialEq, Eq)]
#[error("the answer is not a Response object")]
pub struct NotAResponse;

/// The answer with every HTML body value sanitized.
///
/// # Errors
///
/// Returns [`NotAResponse`] for bytes that do not parse as a Response
/// object.
pub fn clean_answer(sanitizer: &Sanitizer, answer: &[u8]) -> Result<Vec<u8>, NotAResponse> {
    let mut response: Value = serde_json::from_slice(answer).map_err(|_| NotAResponse)?;
    let calls = response
        .get_mut("methodResponses")
        .and_then(Value::as_array_mut)
        .ok_or(NotAResponse)?;
    for call in calls {
        clean_call(sanitizer, call);
    }
    serde_json::to_vec(&response).map_err(|_| NotAResponse)
}

fn clean_call(sanitizer: &Sanitizer, call: &mut Value) {
    let Some([name, arguments, _]) = call.as_array_mut().map(Vec::as_mut_slice) else {
        return;
    };
    if name.as_str() != Some(EMAIL_GET) {
        return;
    }
    let Some(list) = arguments.get_mut("list").and_then(Value::as_array_mut) else {
        return;
    };
    for email in list.iter_mut().filter_map(Value::as_object_mut) {
        clean_email(sanitizer, email);
    }
}

/// The values of the `text/html` parts `htmlBody` names, sanitized; a
/// value that is not a string becomes the empty string.
fn clean_email(sanitizer: &Sanitizer, email: &mut Map<String, Value>) {
    let parts = html_parts(email.get("htmlBody"));
    let Some(values) = email.get_mut("bodyValues").and_then(Value::as_object_mut) else {
        return;
    };
    for part in parts {
        let Some(entry) = values.get_mut(&part).and_then(Value::as_object_mut) else {
            continue;
        };
        let cleaned = entry
            .get("value")
            .and_then(Value::as_str)
            .map(|html| sanitizer.clean(html))
            .unwrap_or_default();
        entry.insert("value".to_owned(), Value::String(cleaned));
    }
}

/// The `partId`s of the `htmlBody` parts whose type is `text/html`,
/// its parameters and case aside.
fn html_parts(body: Option<&Value>) -> Vec<String> {
    body.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|part| is_html(part))
        .filter_map(|part| part.get("partId")?.as_str().map(str::to_owned))
        .collect()
}

fn is_html(part: &Value) -> bool {
    part.get("type")
        .and_then(Value::as_str)
        .and_then(|media_type| media_type.split(';').next())
        .is_some_and(|essence| essence.trim().eq_ignore_ascii_case(HTML))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const HOSTILE: &str = "<p onclick=\"top.__x=1\">Hi</p><script>top.__x=1</script>";

    fn answer(list: &Value) -> Vec<u8> {
        json!({
            "methodResponses": [
                ["Email/get", { "accountId": "u1", "state": "s1", "list": list, "notFound": [] }, "c1"],
                ["Email/query", { "ids": ["e1"] }, "c0"],
                ["error", { "type": "unknownMethod" }, "c2"]
            ],
            "sessionState": "75128aab4b1b"
        })
        .to_string()
        .into_bytes()
    }

    fn cleaned(list: &Value) -> Value {
        let bytes = clean_answer(&Sanitizer::new(None), &answer(list)).unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[test]
    fn the_html_values_html_body_names_are_cleaned_and_everything_else_stays() {
        let list = json!([
            {
                "id": "e1",
                "htmlBody": [{ "partId": "2", "type": "text/html" }, { "partId": "3", "type": "image/png" }],
                "textBody": [{ "partId": "1", "type": "text/plain" }],
                "bodyValues": {
                    "1": { "value": "a < b & <script>", "isEncodingProblem": false, "isTruncated": false },
                    "2": { "value": HOSTILE, "isEncodingProblem": false, "isTruncated": true },
                    "3": { "value": "raw" }
                }
            },
            { "id": "e2", "htmlBody": [], "bodyValues": {} }
        ]);
        let response = cleaned(&list);
        let email = &response["methodResponses"][0][1]["list"][0];
        assert_eq!(email["bodyValues"]["2"]["value"], "<p>Hi</p>");
        assert_eq!(email["bodyValues"]["2"]["isTruncated"], true);
        assert_eq!(email["bodyValues"]["1"]["value"], "a < b & <script>");
        assert_eq!(email["bodyValues"]["3"]["value"], "raw");
        assert_eq!(response["methodResponses"][0][1]["state"], "s1");
        assert_eq!(response["methodResponses"][1][1]["ids"][0], "e1");
        assert_eq!(response["methodResponses"][2][1]["type"], "unknownMethod");
        assert_eq!(response["sessionState"], "75128aab4b1b");
    }

    #[test]
    fn the_type_is_read_by_its_essence_whatever_its_case_or_parameters() {
        let list = json!([{
            "htmlBody": [{ "partId": "1", "type": "TEXT/HTML; charset=utf-8" }],
            "bodyValues": { "1": { "value": HOSTILE } }
        }]);
        let email = &cleaned(&list)["methodResponses"][0][1]["list"][0];
        assert_eq!(email["bodyValues"]["1"]["value"], "<p>Hi</p>");
    }

    #[test]
    fn a_value_that_is_not_a_string_becomes_empty_and_an_absent_one_stays_absent() {
        let list = json!([{
            "htmlBody": [
                { "partId": "1", "type": "text/html" },
                { "partId": "2", "type": "text/html" },
                { "partId": "3", "type": "text/html" }
            ],
            "bodyValues": { "1": { "value": 5 }, "3": "text" }
        }]);
        let email = &cleaned(&list)["methodResponses"][0][1]["list"][0];
        assert_eq!(email["bodyValues"]["1"]["value"], "");
        assert_eq!(email["bodyValues"].get("2"), None);
        assert_eq!(email["bodyValues"]["3"], "text");
    }

    #[test]
    fn a_part_without_an_id_or_a_list_without_shape_is_passed_over() {
        let list = json!([
            { "htmlBody": [{ "type": "text/html" }], "bodyValues": { "1": { "value": HOSTILE } } },
            { "htmlBody": "odd", "bodyValues": { "1": { "value": HOSTILE } } },
            "not an email"
        ]);
        let response = cleaned(&list);
        for index in [0, 1] {
            let email = &response["methodResponses"][0][1]["list"][index];
            assert_eq!(email["bodyValues"]["1"]["value"], HOSTILE);
        }
        assert_eq!(response["methodResponses"][0][1]["list"][2], "not an email");
    }

    #[test]
    fn an_answer_that_is_not_a_response_object_is_refused() {
        let sanitizer = Sanitizer::new(None);
        for bytes in [&b"[]"[..], b"{\"sessionState\":\"x\"}", b"not json", b""] {
            assert_eq!(clean_answer(&sanitizer, bytes), Err(NotAResponse));
        }
        let shapeless = b"{\"methodResponses\":[\"x\",[\"Email/get\"],[\"Email/get\",[],\"c\"]]}";
        assert!(clean_answer(&sanitizer, shapeless).is_ok());
    }
}
