// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! `EmailSubmission/set`, `/get` and `/changes` (RFC 8621 section 7). A
//! create is read on the first pass over its request and refused there
//! from the rows where it can be; between two passes its message goes
//! out over SMTP and the implicit `Email/set` of `onSuccessUpdateEmail`
//! and `onSuccessDestroyEmail` runs; the next pass answers both. Every
//! submission is final, so an update answers `cannotUnsend`. The record
//! of what went out lives in memory, as a basic SMTP proxy may keep it.

mod implicit;
mod read;
pub(super) mod sending;

use std::collections::HashSet;

use serde::Deserialize;
use serde_json::{Map, Value, json};

use self::implicit::Implicit;
use super::changes::{self, ChangesArguments};
use super::get::{self, GetArguments};
use super::set::or_null;
use super::{Context, MAX_OBJECTS_IN_SET, MethodError, arguments, object_of};
use crate::dates::utc_date;
use crate::store::SubmissionRow;

/// The properties of RFC 8621 section 7.
const PROPERTIES: [&str; 10] = [
    "id",
    "identityId",
    "emailId",
    "threadId",
    "envelope",
    "sendAt",
    "undoStatus",
    "deliveryStatus",
    "dsnBlobIds",
    "mdnBlobIds",
];

/// What `created` answers of a submission: every property the server
/// sets (RFC 8620 section 5.3), the envelope only where it derived one.
const SERVER_SET: [&str; 8] = [
    "id",
    "threadId",
    "envelope",
    "sendAt",
    "undoStatus",
    "deliveryStatus",
    "dsnBlobIds",
    "mdnBlobIds",
];

/// Every submission is sent at once and cannot be taken back (RFC 8621
/// section 7).
const FINAL: &str = "final";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SetArguments {
    account_id: String,
    if_in_state: Option<String>,
    create: Option<Map<String, Value>>,
    update: Option<Map<String, Value>>,
    destroy: Option<Vec<String>>,
    on_success_update_email: Option<Map<String, Value>>,
    on_success_destroy_email: Option<Vec<String>>,
}

/// The recipients a create's envelope named (RFC 8621 section 7); its
/// sender goes on the wire as the identity spells its address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Envelope {
    pub rcpt_to: Vec<String>,
}

/// One create whose object reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Submitting {
    /// The email by its id or by a creation id after `#`.
    pub email: String,
    pub envelope: Option<Envelope>,
}

/// What the implicit `Email/set` does to the email of a submission that
/// went out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum OnSuccess {
    Update(Value),
    Destroy,
}

/// What one call wants sent, then done to the emails it sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SubmitAsk {
    pub creates: Vec<(String, Submitting)>,
    /// What each creation id leads to once it went out.
    pub on_success: Vec<(String, OnSuccess)>,
    /// Whether the call gave either `onSuccess` argument, so the implicit
    /// call answers even where it did nothing (RFC 8621 section 7.5).
    pub asked: bool,
}

/// Why one submission did not go out or stays as it was (RFC 8620
/// section 5.3, RFC 8621 section 7.5). Nothing here carries a server's
/// words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SubmitError {
    InvalidProperties(Vec<String>),
    InvalidEmail(&'static str),
    ForbiddenFrom,
    ForbiddenMailFrom,
    ForbiddenToSend,
    NoRecipients,
    InvalidRecipients(Vec<String>),
    TooManyRecipients(usize),
    TooLarge(u64),
    CannotUnsend,
    Forbidden,
    NotFound,
    ServerFail,
    ServerUnavailable,
}

impl SubmitError {
    fn object(&self) -> Value {
        match self {
            Self::InvalidProperties(properties) => {
                json!({ "type": "invalidProperties", "properties": properties })
            }
            Self::InvalidEmail(property) => {
                json!({ "type": "invalidEmail", "properties": [property] })
            }
            Self::InvalidRecipients(addresses) => {
                json!({ "type": "invalidRecipients", "invalidRecipients": addresses })
            }
            Self::TooManyRecipients(most) => {
                json!({ "type": "tooManyRecipients", "maxRecipients": most })
            }
            Self::TooLarge(most) => json!({ "type": "tooLarge", "maxSize": most }),
            Self::ForbiddenFrom => json!({ "type": "forbiddenFrom" }),
            Self::ForbiddenMailFrom => json!({ "type": "forbiddenMailFrom" }),
            Self::ForbiddenToSend => json!({ "type": "forbiddenToSend" }),
            Self::NoRecipients => json!({ "type": "noRecipients" }),
            Self::CannotUnsend => json!({ "type": "cannotUnsend" }),
            Self::Forbidden => json!({ "type": "forbidden" }),
            Self::NotFound => json!({ "type": "notFound" }),
            Self::ServerFail => json!({ "type": "serverFail" }),
            Self::ServerUnavailable => json!({ "type": "serverUnavailable" }),
        }
    }
}

/// What came of one ask between the passes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Submitted {
    pub old_state: String,
    pub new_state: String,
    /// Every create of the ask in its order.
    pub created: Vec<(String, Result<SubmissionRow, SubmitError>)>,
    /// The implicit `Email/set`, where the call gave either `onSuccess`
    /// argument.
    pub implicit: Option<Implicit>,
}

/// `EmailSubmission/set`: on the first pass the creates that read are
/// left for the request to send and the call answers
/// `serverUnavailable`, which stands when nothing could be sent at all;
/// with the outcome at hand the call answers it and hands the implicit
/// `Email/set` answer to the request, which puts it right after. That
/// answer comes whenever the call asked for it, with nothing done where
/// nothing went out (RFC 8621 section 7.5).
pub(super) fn set(
    context: &Context<'_>,
    raw: &Map<String, Value>,
) -> Result<Map<String, Value>, MethodError> {
    let arguments: SetArguments = arguments(raw)?;
    context.account(&arguments.account_id)?;
    let create = arguments.create.unwrap_or_default();
    let update = arguments.update.unwrap_or_default();
    let destroy = arguments.destroy.unwrap_or_default();
    if create.len() + update.len() + destroy.len() > MAX_OBJECTS_IN_SET {
        return Err(MethodError::RequestTooLarge);
    }
    let record = context.store.submissions(context.key);
    let submitted = context.submitted.borrow();
    // The state moves with the sends of the call, so a later pass judges
    // nothing against it.
    if submitted.is_none()
        && arguments
            .if_in_state
            .is_some_and(|state| state != record.state)
    {
        return Err(MethodError::StateMismatch);
    }
    let (readable, mut not_created) = read::creates(context, &create);
    let asked =
        arguments.on_success_update_email.is_some() || arguments.on_success_destroy_email.is_some();
    let on_success = read::on_success(
        arguments.on_success_update_email.unwrap_or_default(),
        arguments.on_success_destroy_email.unwrap_or_default(),
    )?;
    let known = |id: &str| record.rows.iter().any(|row| row.id.as_str() == id);
    let refused = |id: &str, known_error: SubmitError| {
        let error = if known(id) {
            known_error
        } else {
            SubmitError::NotFound
        };
        (id.to_owned(), error.object())
    };
    let not_updated: Map<String, Value> = update
        .keys()
        .map(|id| refused(id, SubmitError::CannotUnsend))
        .collect();
    let not_destroyed: Map<String, Value> = destroy
        .iter()
        .map(|id| refused(id, SubmitError::Forbidden))
        .collect();
    // On a later pass the outcome stands for every create that went to
    // the server, since what was sent changed the rows read again here.
    let (old_state, new_state, sent) = match *submitted {
        Some(outcome) => {
            let implicit = outcome.implicit.clone();
            *context.trailer.borrow_mut() = implicit.and_then(|implicit| implicit.answer(context));
            for (id, _) in &outcome.created {
                not_created.remove(id);
            }
            let states = (outcome.old_state.clone(), outcome.new_state.clone());
            (states.0, states.1, outcome.created.clone())
        }
        None if readable.is_empty() => {
            if asked {
                *context.trailer.borrow_mut() = Implicit::default().answer(context);
            }
            (record.state.clone(), record.state, Vec::new())
        }
        None => {
            *context.submit_ask.borrow_mut() = Some(SubmitAsk {
                creates: readable,
                on_success,
                asked,
            });
            return Err(MethodError::ServerUnavailable);
        }
    };
    let mut created = Map::new();
    for (id, result) in sent {
        match result {
            Ok(row) => {
                let object = row_object(&row, &answered(&create, &id));
                created.insert(id, object)
            }
            Err(error) => not_created.insert(id, error.object()),
        };
    }
    let answer = json!({
        "accountId": context.key.as_str(),
        "oldState": old_state,
        "newState": new_state,
        "created": or_null(created),
        "updated": null,
        "destroyed": null,
        "notCreated": or_null(not_created),
        "notUpdated": or_null(not_updated),
        "notDestroyed": or_null(not_destroyed),
    });
    Ok(object_of(answer))
}

/// `EmailSubmission/get`: the submissions this process sent of the last
/// `RECENT_SUBMISSIONS`, every one when `ids` is null.
pub(super) fn get(
    context: &Context<'_>,
    raw: &Map<String, Value>,
) -> Result<Map<String, Value>, MethodError> {
    let arguments: GetArguments = arguments(raw)?;
    context.account(&arguments.account_id)?;
    let record = context.store.submissions(context.key);
    let wanted = get::properties(&PROPERTIES, arguments.properties.as_deref())?;
    let asked = match arguments.ids.as_deref() {
        None => Vec::new(),
        named => get::ids(named)?,
    };
    let answered: Vec<&SubmissionRow> = if arguments.ids.is_none() {
        record.rows.iter().collect()
    } else {
        asked
            .iter()
            .filter_map(|id| record.rows.iter().find(|row| row.id.as_str() == *id))
            .collect()
    };
    let found: HashSet<&str> = answered.iter().map(|row| row.id.as_str()).collect();
    let list = answered
        .iter()
        .map(|row| row_object(row, &wanted))
        .collect();
    Ok(get::answer(
        context,
        (record.state.as_str(), list),
        &asked,
        &found,
    ))
}

/// `EmailSubmission/changes`: the record holds no history, so its own
/// state answers an empty change and any other state cannot be followed.
pub(super) fn changes(
    context: &Context<'_>,
    raw: &Map<String, Value>,
) -> Result<Map<String, Value>, MethodError> {
    let arguments: ChangesArguments = arguments(raw)?;
    context.account(&arguments.account_id)?;
    changes::checked(&arguments)?;
    let state = context.store.submissions(context.key).state;
    if arguments.since_state != state {
        return Err(MethodError::CannotCalculateChanges);
    }
    let answer = json!({
        "accountId": context.key.as_str(),
        "oldState": state,
        "newState": state,
        "hasMoreChanges": false,
        "created": [],
        "updated": [],
        "destroyed": [],
    });
    Ok(object_of(answer))
}

/// The names `created` answers of one create: those of `SERVER_SET`, the
/// envelope among them only where the create gave none.
fn answered(create: &Map<String, Value>, id: &str) -> Vec<&'static str> {
    let given = create
        .get(id)
        .and_then(|object| object.get("envelope"))
        .is_some_and(|envelope| !envelope.is_null());
    SERVER_SET
        .into_iter()
        .filter(|name| *name != "envelope" || !given)
        .collect()
}

/// One submission, cut to the wanted properties.
fn row_object(row: &SubmissionRow, wanted: &[&str]) -> Value {
    let address = |email: &str| json!({ "email": email, "parameters": null });
    let recipients: Vec<Value> = row.rcpt_to.iter().map(|email| address(email)).collect();
    let envelope = json!({ "mailFrom": address(&row.mail_from), "rcptTo": recipients });
    let pairs = [
        ("id", json!(row.id)),
        ("identityId", json!(row.identity_id)),
        ("emailId", json!(row.email_id)),
        ("threadId", json!(row.thread_id)),
        ("envelope", envelope),
        ("sendAt", json!(utc_date(row.send_at))),
        ("undoStatus", json!(FINAL)),
        ("deliveryStatus", Value::Null),
        ("dsnBlobIds", json!([])),
        ("mdnBlobIds", json!([])),
    ];
    pairs
        .into_iter()
        .filter(|(name, _)| wanted.contains(name))
        .map(|(name, value)| (name.to_owned(), value))
        .collect::<Map<_, _>>()
        .into()
}
