// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The JMAP side of the bridge: one Request object in, one Response
//! object out (RFC 8620 sections 3.3 and 3.4), the methods answered
//! from the store.

mod changes;
mod mailbox;
mod references;
mod session;

use std::collections::BTreeMap;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;

use crate::store::{AccountKey, Store, StoreError};

pub use session::{CORE_CAPABILITY, HULIHO_CAPABILITY, MAIL_CAPABILITY, Urls, session_object};

/// A Request object of one MiB at most, room for an `Email/set` with a
/// body once one exists.
pub const MAX_SIZE_REQUEST: usize = 1024 * 1024;

/// Requests in flight per account; the host's semaphore enforces it.
pub const MAX_CONCURRENT_REQUESTS: u32 = 4;

/// Method calls in one request.
pub const MAX_CALLS_IN_REQUEST: usize = 16;

/// Ids in one `/get`.
pub const MAX_OBJECTS_IN_GET: usize = 500;

/// Why a request was not run (RFC 8620 section 3.6.1); the host answers
/// a problem details object with status 400, or 500 for the store.
#[derive(Debug, Error)]
pub enum RequestError {
    #[error("a capability in `using` is not carried")]
    UnknownCapability,
    #[error("the body is not JSON")]
    NotJson,
    #[error("the JSON is not a Request object")]
    NotRequest,
    #[error("the request passes the limit {0}")]
    Limit(&'static str),
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl RequestError {
    /// The problem type of RFC 8620 section 3.6.1; the store's failure
    /// has none.
    #[must_use]
    pub fn problem_type(&self) -> Option<&'static str> {
        Some(match self {
            Self::UnknownCapability => "urn:ietf:params:jmap:error:unknownCapability",
            Self::NotJson => "urn:ietf:params:jmap:error:notJSON",
            Self::NotRequest => "urn:ietf:params:jmap:error:notRequest",
            Self::Limit(_) => "urn:ietf:params:jmap:error:limit",
            Self::Store(_) => return None,
        })
    }

    /// The limit a `limit` problem names.
    #[must_use]
    pub fn limit(&self) -> Option<&'static str> {
        match self {
            Self::Limit(limit) => Some(limit),
            _ => None,
        }
    }
}

/// A method call or a response: the name, the arguments and the call
/// id (RFC 8620 section 3.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Invocation(pub String, pub Map<String, Value>, pub String);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Request {
    using: Vec<String>,
    method_calls: Vec<Invocation>,
    created_ids: Option<BTreeMap<String, String>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Response {
    method_responses: Vec<Invocation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    created_ids: Option<BTreeMap<String, String>>,
    session_state: String,
}

/// The capabilities a request opted into (RFC 8620 section 1.8); a
/// method runs only under its own.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Using {
    core: bool,
    mail: bool,
    huliho: bool,
}

fn using(names: &[String]) -> Result<Using, RequestError> {
    let mut using = Using::default();
    for name in names {
        match name.as_str() {
            CORE_CAPABILITY => using.core = true,
            MAIL_CAPABILITY => using.mail = true,
            HULIHO_CAPABILITY => using.huliho = true,
            _ => return Err(RequestError::UnknownCapability),
        }
    }
    Ok(using)
}

/// What the calls of one request share: the rows every method runs
/// against and the budget of their result references.
pub(crate) struct Context<'a> {
    store: &'a Store,
    key: &'a AccountKey,
    using: Using,
    budget: references::Budget,
}

impl Context<'_> {
    /// The one account this endpoint serves.
    fn account(&self, id: &str) -> Result<(), MethodError> {
        if id == self.key.as_str() {
            Ok(())
        } else {
            Err(MethodError::AccountNotFound)
        }
    }
}

/// A method-level error (RFC 8620 section 3.6.2); the store's failure
/// reads as `serverFail`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MethodError {
    UnknownMethod,
    InvalidArguments(&'static str),
    InvalidResultReference(&'static str),
    AccountNotFound,
    RequestTooLarge,
    CannotCalculateChanges,
    ServerFail,
}

impl MethodError {
    fn object(&self) -> Map<String, Value> {
        let kind = match self {
            Self::UnknownMethod => "unknownMethod",
            Self::InvalidArguments(_) => "invalidArguments",
            Self::InvalidResultReference(_) => "invalidResultReference",
            Self::AccountNotFound => "accountNotFound",
            Self::RequestTooLarge => "requestTooLarge",
            Self::CannotCalculateChanges => "cannotCalculateChanges",
            Self::ServerFail => "serverFail",
        };
        let mut object = Map::new();
        object.insert("type".to_owned(), Value::from(kind));
        if let Self::InvalidArguments(why) | Self::InvalidResultReference(why) = self {
            object.insert("description".to_owned(), Value::from(*why));
        }
        object
    }
}

impl From<StoreError> for MethodError {
    fn from(_error: StoreError) -> Self {
        Self::ServerFail
    }
}

/// Runs one Request object against the account's rows and answers the
/// Response object as JSON.
///
/// # Errors
///
/// Returns [`RequestError`] when the request cannot run at all; a
/// method that fails answers inside the Response object instead.
pub fn handle(store: &Store, key: &AccountKey, body: &[u8]) -> Result<Vec<u8>, RequestError> {
    if body.len() > MAX_SIZE_REQUEST {
        return Err(RequestError::Limit("maxSizeRequest"));
    }
    let value: Value = serde_json::from_slice(body).map_err(|_| RequestError::NotJson)?;
    let request: Request = serde_json::from_value(value).map_err(|_| RequestError::NotRequest)?;
    let using = using(&request.using)?;
    if request.method_calls.len() > MAX_CALLS_IN_REQUEST {
        return Err(RequestError::Limit("maxCallsInRequest"));
    }
    let session_state = store.state(key)?.to_string();
    let context = Context {
        store,
        key,
        using,
        budget: references::Budget::full(),
    };
    let mut responses = Vec::with_capacity(request.method_calls.len());
    for call in &request.method_calls {
        let response = run(&context, call, &responses);
        responses.push(response);
    }
    let response = Response {
        method_responses: responses,
        created_ids: request.created_ids,
        session_state,
    };
    serde_json::to_vec(&response).map_err(|error| RequestError::Store(StoreError::Encoding(error)))
}

/// One call: the references resolved, the method run, a failure folded
/// into the `error` response under the call's id.
fn run(context: &Context<'_>, call: &Invocation, previous: &[Invocation]) -> Invocation {
    let Invocation(name, arguments, call_id) = call;
    let outcome = references::resolve(arguments, previous, &context.budget)
        .and_then(|arguments| dispatch(context, name, &arguments));
    match outcome {
        Ok(result) => Invocation(name.clone(), result, call_id.clone()),
        Err(error) => Invocation("error".to_owned(), error.object(), call_id.clone()),
    }
}

/// The methods this bridge answers, each behind its capability.
fn dispatch(
    context: &Context<'_>,
    name: &str,
    arguments: &Map<String, Value>,
) -> Result<Map<String, Value>, MethodError> {
    match name {
        "Core/echo" if context.using.core => Ok(arguments.clone()),
        "Mailbox/get" if context.using.mail => mailbox::get(context, arguments),
        "Mailbox/changes" if context.using.mail => changes::mailbox(context, arguments),
        _ => Err(MethodError::UnknownMethod),
    }
}

/// The arguments of one call as a typed object; a wrong shape is
/// `invalidArguments`.
pub(crate) fn arguments<T: DeserializeOwned>(
    arguments: &Map<String, Value>,
) -> Result<T, MethodError> {
    serde_json::from_value(Value::Object(arguments.clone())).map_err(|_| {
        MethodError::InvalidArguments("an argument is missing, unknown or of the wrong type")
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn using_reads_the_three_capabilities_and_refuses_a_fourth_rfc8620_3_6_1() {
        let three = [CORE_CAPABILITY, MAIL_CAPABILITY, HULIHO_CAPABILITY].map(str::to_owned);
        assert_eq!(
            using(&three).unwrap(),
            Using {
                core: true,
                mail: true,
                huliho: true
            }
        );
        assert_eq!(using(&[]).unwrap(), Using::default());
        let other = ["urn:ietf:params:jmap:websocket".to_owned()];
        assert!(matches!(
            using(&other),
            Err(RequestError::UnknownCapability)
        ));
    }

    #[test]
    fn every_request_error_names_its_problem_type() {
        assert_eq!(
            RequestError::Limit("maxSizeRequest").problem_type(),
            Some("urn:ietf:params:jmap:error:limit")
        );
        assert_eq!(
            RequestError::Limit("maxSizeRequest").limit(),
            Some("maxSizeRequest")
        );
        assert_eq!(RequestError::NotJson.limit(), None);
        assert_eq!(
            RequestError::Store(StoreError::Poisoned).problem_type(),
            None
        );
    }

    #[test]
    fn a_method_error_renders_its_type_and_a_description_where_it_has_one() {
        let plain = MethodError::UnknownMethod.object();
        assert_eq!(plain["type"], "unknownMethod");
        assert_eq!(plain.get("description"), None);
        let described = MethodError::InvalidArguments("why").object();
        assert_eq!(described["type"], "invalidArguments");
        assert_eq!(described["description"], "why");
    }

    /// `Core/echo` calls: the first echoes 64 KiB of text, every later
    /// one echoes the call before it twice through the path `""`.
    fn doubling(calls: usize) -> Vec<Value> {
        let mut out = vec![json!(["Core/echo", { "text": "x".repeat(64 * 1024) }, "c0"])];
        for call in 1..calls {
            let before = json!({
                "resultOf": format!("c{}", call - 1),
                "name": "Core/echo",
                "path": ""
            });
            let twice = json!({ "#a": before, "#b": before });
            out.push(json!(["Core/echo", twice, format!("c{call}")]));
        }
        out
    }

    fn responses(store: &Store, calls: &[Value]) -> Vec<Invocation> {
        let body = json!({ "using": [CORE_CAPABILITY], "methodCalls": calls });
        let body = serde_json::to_vec(&body).unwrap();
        let answer = handle(store, &AccountKey::new("a1"), &body).unwrap();
        let mut answer: Value = serde_json::from_slice(&answer).unwrap();
        serde_json::from_value(answer["methodResponses"].take()).unwrap()
    }

    #[test]
    fn references_past_the_budget_fail_their_call_alone_rfc8620_3_6_2() {
        let store = Store::in_memory().unwrap();
        let mut calls = doubling(5);
        calls.push(json!(["Core/echo", { "after": true }, "c5"]));
        let responses = responses(&store, &calls);
        for Invocation(name, _, _) in &responses[..4] {
            assert_eq!(name, "Core/echo");
        }
        let Invocation(name, error, id) = &responses[4];
        assert_eq!((name.as_str(), id.as_str()), ("error", "c4"));
        assert_eq!(error["type"], "invalidResultReference");
        assert!(error["description"].is_string());
        let Invocation(name, after, id) = &responses[5];
        assert_eq!((name.as_str(), id.as_str()), ("Core/echo", "c5"));
        assert_eq!(after["after"], true);
    }

    #[test]
    fn a_chain_of_references_inside_the_budget_resolves_rfc8620_3_7() {
        let store = Store::in_memory().unwrap();
        let ids = |call: &str| json!({ "resultOf": call, "name": "Core/echo", "path": "/ids" });
        let calls = [
            json!(["Core/echo", { "ids": ["e1", "e2"] }, "c0"]),
            json!(["Core/echo", { "#ids": ids("c0") }, "c1"]),
            json!(["Core/echo", { "#ids": ids("c1") }, "c2"]),
        ];
        for Invocation(name, arguments, _) in responses(&store, &calls) {
            assert_eq!(name, "Core/echo");
            assert_eq!(arguments["ids"], json!(["e1", "e2"]));
        }
    }

    #[test]
    fn every_request_starts_with_a_full_budget() {
        let store = Store::in_memory().unwrap();
        for _ in 0..2 {
            for Invocation(name, _, _) in responses(&store, &doubling(4)) {
                assert_eq!(name, "Core/echo");
            }
        }
    }
}
