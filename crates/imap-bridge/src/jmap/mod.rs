// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The JMAP side of the bridge: one Request object in, one Response
//! object out (RFC 8620 sections 3.3 and 3.4), the methods answered
//! from the store. A `/changes` call refreshes the account first when a
//! refresh is due; an `Email/get` that asks for previews the rows lack
//! or for bodies fetches them and answers again, with every call after
//! it; an `Email/set` and a `Mailbox/set` write to the server the same
//! way and an `Email/query` with a text or a sender has the server
//! searched.

mod bodies;
mod body;
mod changes;
mod email;
#[cfg(feature = "test-support")]
pub mod fuzzing;
mod get;
mod headers;
mod mailbox;
mod mailbox_set;
mod previews;
mod query;
mod quota;
mod references;
mod request;
mod session;
mod set;
mod thread;
mod values;
mod write;

use std::cell::RefCell;
use std::collections::BTreeMap;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;

use crate::runtime::folders;
use crate::runtime::search::{Failure, Hits, Plan};
use crate::seal::Sealer;
use crate::store::{AccountKey, EmailId, MailboxId, Store, StoreError};

pub use body::MAX_BODIES_IN_GET;
pub(crate) use body::leaf_of;
pub use headers::MAX_HEADER_FIELDS;
pub use previews::PREVIEW_BATCH;
pub use query::QUERY_LIMIT;
pub use request::handle;
pub use session::{
    CORE_CAPABILITY, HULIHO_CAPABILITY, MAIL_CAPABILITY, QUOTA_CAPABILITY, Urls, session_object,
};
pub use values::{MAX_BODY_VALUE_BYTES, MAX_BODY_WINDOWS};

/// A Request object of one MiB at most, room for an `Email/set` over
/// every object it may name and for one with a body once that exists.
pub const MAX_SIZE_REQUEST: usize = 1024 * 1024;

/// Requests in flight per account; the host's semaphore enforces it.
pub const MAX_CONCURRENT_REQUESTS: usize = 4;

/// Method calls in one request.
pub const MAX_CALLS_IN_REQUEST: usize = 16;

/// Ids in one `/get`.
pub const MAX_OBJECTS_IN_GET: usize = 500;

/// Objects in one `/set`, the floor RFC 8620 section 2 suggests: one
/// transaction writes a row per message and mailbox, the size of a sync
/// batch.
pub const MAX_OBJECTS_IN_SET: usize = 500;

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

/// The capabilities the bridge carries, in the order [`Using`] keeps
/// them.
const CARRIED: [&str; 4] = [
    CORE_CAPABILITY,
    MAIL_CAPABILITY,
    QUOTA_CAPABILITY,
    HULIHO_CAPABILITY,
];

/// The capabilities a request opted into (RFC 8620 section 1.8), one
/// flag per carried capability; a method runs only under its own.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Using([bool; CARRIED.len()]);

impl Using {
    /// Whether the request opted into the capability.
    pub(crate) fn has(self, capability: &str) -> bool {
        CARRIED
            .iter()
            .zip(self.0)
            .any(|(carried, opted)| opted && *carried == capability)
    }
}

fn using(names: &[String]) -> Result<Using, RequestError> {
    let mut using = Using::default();
    for name in names {
        let carried = CARRIED.iter().position(|carried| carried == name);
        using.0[carried.ok_or(RequestError::UnknownCapability)?] = true;
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
    /// What an `Email/get` wants of the messages on the server, noted
    /// on the first pass.
    body_ask: RefCell<Option<bodies::BodyAsk>>,
    /// What the server holds of them, set for the call on a later pass;
    /// `None` on the first.
    bodies: RefCell<Option<&'a bodies::Bodies>>,
    /// What an `Email/set` wants stored on the server, noted on the
    /// first pass.
    set_ask: RefCell<Option<set::SetAsk>>,
    /// What came of that, set for the call on a later pass.
    written: RefCell<Option<&'a set::Outcome>>,
    /// What a `Mailbox/set` wants done on the server, noted on the
    /// first pass.
    folder_ask: RefCell<Option<folders::Ask>>,
    /// What came of that, set for the call on a later pass.
    folders_done: RefCell<Option<&'a folders::Outcome>>,
    /// What an `Email/query` wants searched on the server, noted on the
    /// first pass.
    search_ask: RefCell<Option<Plan>>,
    /// What the search found, set for the call on a later pass.
    searched: RefCell<Option<&'a Result<Hits, Failure>>>,
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
    /// The account exists but has no data of the method's type, as an
    /// account without a quota has none for `Quota/get`.
    AccountNotSupportedByMethod,
    RequestTooLarge,
    CannotCalculateChanges,
    /// The filter reads but the bridge cannot apply it, with why.
    UnsupportedFilter(&'static str),
    UnsupportedSort,
    AnchorNotFound,
    /// `ifInState` named another state than the account stands at (RFC
    /// 8620 section 5.3).
    StateMismatch,
    ServerFail,
    /// The server behind the account did not answer in time; the same
    /// call may succeed later (RFC 8620 section 3.6.2).
    ServerUnavailable,
}

impl MethodError {
    fn object(&self) -> Map<String, Value> {
        let kind = match self {
            Self::UnknownMethod => "unknownMethod",
            Self::InvalidArguments(_) => "invalidArguments",
            Self::InvalidResultReference(_) => "invalidResultReference",
            Self::AccountNotFound => "accountNotFound",
            Self::AccountNotSupportedByMethod => "accountNotSupportedByMethod",
            Self::RequestTooLarge => "requestTooLarge",
            Self::CannotCalculateChanges => "cannotCalculateChanges",
            Self::UnsupportedFilter(_) => "unsupportedFilter",
            Self::UnsupportedSort => "unsupportedSort",
            Self::AnchorNotFound => "anchorNotFound",
            Self::StateMismatch => "stateMismatch",
            Self::ServerFail => "serverFail",
            Self::ServerUnavailable => "serverUnavailable",
        };
        let mut object = Map::new();
        object.insert("type".to_owned(), Value::from(kind));
        if let Self::InvalidArguments(why)
        | Self::InvalidResultReference(why)
        | Self::UnsupportedFilter(why) = self
        {
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
    let mail = context.using.has(MAIL_CAPABILITY);
    let quota = context.using.has(QUOTA_CAPABILITY);
    match name {
        "Core/echo" if context.using.has(CORE_CAPABILITY) => Ok(arguments.clone()),
        "Mailbox/get" if mail => mailbox::get(context, arguments),
        "Mailbox/changes" if mail => changes::mailbox(context, arguments),
        "Mailbox/set" if mail => mailbox_set::set(context, arguments),
        "Email/get" if mail => email::get(context, arguments),
        "Email/set" if mail => set::email(context, arguments),
        "Email/query" if mail => query::email(context, arguments),
        "Email/changes" if mail => changes::email(context, arguments),
        "Thread/get" if mail => thread::get(context, arguments),
        "Thread/changes" if mail => changes::thread(context, arguments),
        "Quota/get" if quota => quota::get(context, arguments),
        "Quota/changes" if quota => quota::changes(context, arguments),
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
    fn using_reads_the_four_capabilities_and_refuses_a_fifth_rfc8620_3_6_1() {
        let four = using(&CARRIED.map(str::to_owned)).unwrap();
        assert!(CARRIED.iter().all(|capability| four.has(capability)));
        let mail = using(&[MAIL_CAPABILITY.to_owned()]).unwrap();
        assert!(mail.has(MAIL_CAPABILITY) && !mail.has(QUOTA_CAPABILITY));
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
            (MethodError::UnsupportedFilter("why"), "unsupportedFilter"),
            (MethodError::UnsupportedSort, "unsupportedSort"),
            (MethodError::AnchorNotFound, "anchorNotFound"),
            (MethodError::StateMismatch, "stateMismatch"),
            (MethodError::ServerUnavailable, "serverUnavailable"),
            (
                MethodError::AccountNotSupportedByMethod,
                "accountNotSupportedByMethod",
            ),
        ] {
            assert_eq!(error.object()["type"], kind);
        }
    }
}
