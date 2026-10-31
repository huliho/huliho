// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! `Email/set` (RFC 8620 section 5.3, RFC 8621 section 4.6): updates of
//! `keywords` and `mailboxIds` and destroys. A call is read on the first
//! pass over its request, what it asks is done on the server between two
//! passes and the answer is rendered from what came of that on the next.
//! A create is forbidden per object.

mod patch;

use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::{Context, MAX_OBJECTS_IN_SET, MethodError, arguments};
use crate::runtime::folders::CreatedIds;

#[cfg(test)]
pub(super) use patch::patch as read_patch;
pub(super) use patch::{Edit, MAX_UPDATE_KEYWORD_BYTES, MailboxEdit, Patch};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SetArguments {
    account_id: String,
    if_in_state: Option<String>,
    create: Option<Map<String, Value>>,
    update: Option<Map<String, Value>>,
    destroy: Option<Vec<String>>,
}

/// Why one object was not written (RFC 8620 section 5.3, RFC 8621
/// section 4.6). The last two are the method-level words of RFC 8620
/// section 3.6.2 for one object: the server behind the account refused
/// the write, or could not be asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SetError {
    Forbidden,
    NotFound,
    InvalidPatch,
    /// The paths of the patch that cannot be applied, as it named them.
    InvalidProperties(Vec<String>),
    TooManyKeywords,
    /// The email would lie in more mailboxes than `maxMailboxesPerEmail`.
    TooManyMailboxes,
    /// The same call destroys the email.
    WillDestroy,
    ServerFail,
    ServerUnavailable,
}

impl SetError {
    fn object(&self) -> Value {
        let kind = match self {
            Self::Forbidden => "forbidden",
            Self::NotFound => "notFound",
            Self::InvalidPatch => "invalidPatch",
            Self::InvalidProperties(_) => "invalidProperties",
            Self::TooManyKeywords => "tooManyKeywords",
            Self::TooManyMailboxes => "tooManyMailboxes",
            Self::WillDestroy => "willDestroy",
            Self::ServerFail => "serverFail",
            Self::ServerUnavailable => "serverUnavailable",
        };
        match self {
            Self::InvalidProperties(properties) => {
                json!({ "type": kind, "properties": properties })
            }
            _ => json!({ "type": kind }),
        }
    }
}

/// What one call wants done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SetAsk {
    pub if_in_state: Option<String>,
    /// The updates whose patch reads, by email id as written.
    pub updates: Vec<(String, Patch)>,
    /// The emails to destroy, each once, in the order named.
    pub destroy: Vec<String>,
}

impl SetAsk {
    /// The ask with every mailbox a patch names by creation id replaced
    /// by the id it stands for; one the request made nothing under
    /// stays as written (RFC 8620 section 5.3).
    pub(super) fn resolved(&self, created: &CreatedIds) -> Self {
        let updates = self
            .updates
            .iter()
            .map(|(id, patch)| {
                let mut patch = patch.clone();
                if let Some(edit) = &mut patch.mailboxes {
                    edit.resolve(created);
                }
                (id.clone(), patch)
            })
            .collect();
        Self {
            if_in_state: self.if_in_state.clone(),
            updates,
            destroy: self.destroy.clone(),
        }
    }
}

/// One update done: the properties the server changed beyond what its
/// patch asked, `None` when there are none (RFC 8620 section 5.3).
pub(super) type Updated = Option<Map<String, Value>>;

/// What came of one ask.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Outcome {
    /// `ifInState` named another state than the account stood at.
    StateMismatch,
    Stored {
        old_state: u64,
        new_state: u64,
        /// Every update of the ask in its order.
        updated: Vec<(String, Result<Updated, SetError>)>,
        /// Every destroy of the ask in its order.
        destroyed: Vec<(String, Result<(), SetError>)>,
    },
}

/// A map of objects by id, null when it holds none (RFC 8620 section 5.3).
fn or_null(objects: Map<String, Value>) -> Value {
    if objects.is_empty() {
        Value::Null
    } else {
        Value::Object(objects)
    }
}

/// `Email/set`: on the first pass the updates that read and the destroys
/// are left for the request to do and the call answers
/// `serverUnavailable`, which stands when nothing could be done at all;
/// with the outcome at hand the call answers it. An update of an email
/// the same call destroys is skipped (RFC 8620 section 5.3).
pub(super) fn email(
    context: &Context<'_>,
    raw: &Map<String, Value>,
) -> Result<Map<String, Value>, MethodError> {
    let arguments: SetArguments = arguments(raw)?;
    context.account(&arguments.account_id)?;
    let create = arguments.create.unwrap_or_default();
    let update = arguments.update.unwrap_or_default();
    let mut destroy = arguments.destroy.unwrap_or_default();
    if create.len() + update.len() + destroy.len() > MAX_OBJECTS_IN_SET {
        return Err(MethodError::RequestTooLarge);
    }
    let mut seen = std::collections::HashSet::new();
    destroy.retain(|id| seen.insert(id.clone()));
    let forbidden = SetError::Forbidden.object();
    let not_created = create.keys().map(|id| (id.clone(), forbidden.clone()));
    let mut not_updated = Map::new();
    let mut patches = Vec::new();
    for (id, value) in &update {
        let read = if destroy.contains(id) {
            Err(SetError::WillDestroy)
        } else {
            patch::patch(value)
        };
        match read {
            Ok(patch) => patches.push((id.clone(), patch)),
            Err(error) => {
                not_updated.insert(id.clone(), error.object());
            }
        }
    }
    let written = context.written.borrow();
    let Some(outcome) = *written else {
        *context.set_ask.borrow_mut() = Some(SetAsk {
            if_in_state: arguments.if_in_state,
            updates: patches,
            destroy,
        });
        return Err(MethodError::ServerUnavailable);
    };
    let Outcome::Stored {
        old_state,
        new_state,
        updated: results,
        destroyed: gone,
    } = outcome
    else {
        return Err(MethodError::StateMismatch);
    };
    let mut updated = Map::new();
    for (id, result) in results {
        match result {
            Ok(changed) => updated.insert(
                id.clone(),
                changed.clone().map_or(Value::Null, Value::Object),
            ),
            Err(error) => not_updated.insert(id.clone(), error.object()),
        };
    }
    let mut destroyed = Vec::new();
    let mut not_destroyed = Map::new();
    for (id, result) in gone {
        match result {
            Ok(()) => destroyed.push(Value::from(id.as_str())),
            Err(error) => {
                not_destroyed.insert(id.clone(), error.object());
            }
        }
    }
    let destroyed = if destroyed.is_empty() {
        Value::Null
    } else {
        Value::Array(destroyed)
    };
    let answer = [
        ("accountId", Value::from(context.key.as_str())),
        ("oldState", Value::from(old_state.to_string())),
        ("newState", Value::from(new_state.to_string())),
        ("created", Value::Null),
        ("updated", or_null(updated)),
        ("destroyed", destroyed),
        ("notCreated", or_null(not_created.collect())),
        ("notUpdated", or_null(not_updated)),
        ("notDestroyed", or_null(not_destroyed)),
    ];
    Ok(answer
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect())
}

#[cfg(test)]
mod tests;
