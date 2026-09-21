// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The JMAP side of the bridge: one Request object in, one Response
//! object out (RFC 8620 sections 3.3 and 3.4), the methods answered
//! from the store. A `/changes` call refreshes the account first when a
//! refresh is due; an `Email/get` that asks for previews the rows lack
//! fetches them and answers again.

mod changes;
mod email;
mod get;
mod mailbox;
mod previews;
mod query;
mod references;
mod request;
mod session;
mod thread;

use std::cell::RefCell;
use std::collections::BTreeMap;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;

use crate::seal::Sealer;
use crate::store::{AccountKey, EmailId, MailboxId, Store, StoreError};

pub use previews::PREVIEW_BATCH;
pub use query::QUERY_LIMIT;
pub use request::handle;
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
/// a problem details object with status 400, or 500 for the store and
/// the task.
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
    #[error("the task answering the request ended early")]
    Task,
}

impl RequestError {
    /// The problem type of RFC 8620 section 3.6.1; the store's and the
    /// task's failures have none.
    #[must_use]
    pub fn problem_type(&self) -> Option<&'static str> {
        Some(match self {
            Self::UnknownCapability => "urn:ietf:params:jmap:error:unknownCapability",
            Self::NotJson => "urn:ietf:params:jmap:error:notJSON",
            Self::NotRequest => "urn:ietf:params:jmap:error:notRequest",
            Self::Limit(_) => "urn:ietf:params:jmap:error:limit",
            Self::Store(_) | Self::Task => return None,
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
/// against, the budget of their result references and what the calls
/// leave for the request to act on.
pub(crate) struct Context<'a> {
    store: &'a Store,
    sealer: &'a dyn Sealer,
    key: &'a AccountKey,
    using: Using,
    budget: references::Budget,
    /// The mailbox the last query ranged over.
    viewed: RefCell<Option<MailboxId>>,
    /// The emails an `Email/get` wanted a preview for that the rows do
    /// not hold yet.
    missing_previews: RefCell<Vec<EmailId>>,
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

/// A method-level error (RFC 8620 section 3.6.2, RFC 8621 section 4.4);
/// the store's failure reads as `serverFail`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MethodError {
    UnknownMethod,
    InvalidArguments(&'static str),
    InvalidResultReference(&'static str),
    AccountNotFound,
    RequestTooLarge,
    CannotCalculateChanges,
    UnsupportedFilter,
    UnsupportedSort,
    AnchorNotFound,
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
            Self::UnsupportedFilter => "unsupportedFilter",
            Self::UnsupportedSort => "unsupportedSort",
            Self::AnchorNotFound => "anchorNotFound",
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
        "Email/get" if context.using.mail => email::get(context, arguments),
        "Email/query" if context.using.mail => query::email(context, arguments),
        "Email/changes" if context.using.mail => changes::email(context, arguments),
        "Thread/get" if context.using.mail => thread::get(context, arguments),
        "Thread/changes" if context.using.mail => changes::thread(context, arguments),
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
        assert_eq!(RequestError::Task.problem_type(), None);
    }

    #[test]
    fn a_method_error_renders_its_type_and_a_description_where_it_has_one() {
        let plain = MethodError::UnknownMethod.object();
        assert_eq!(plain["type"], "unknownMethod");
        assert_eq!(plain.get("description"), None);
        let described = MethodError::InvalidArguments("why").object();
        assert_eq!(described["type"], "invalidArguments");
        assert_eq!(described["description"], "why");
        for (error, kind) in [
            (MethodError::UnsupportedFilter, "unsupportedFilter"),
            (MethodError::UnsupportedSort, "unsupportedSort"),
            (MethodError::AnchorNotFound, "anchorNotFound"),
        ] {
            assert_eq!(error.object()["type"], kind);
        }
    }
}
