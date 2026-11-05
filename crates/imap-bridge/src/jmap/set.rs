// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! `Email/set` (RFC 8620 section 5.3, RFC 8621 section 4.6): creates of a
//! draft, updates of `keywords` and `mailboxIds` and destroys. A call is
//! read on the first pass over its request, what it asks is done on the
//! server between two passes and the answer is rendered from what came of
//! that on the next.

mod patch;

use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::email_set::create;
use super::{Context, MAX_OBJECTS_IN_SET, MethodError, arguments};
use crate::message::Draft;
use crate::runtime::drafts::Built;
use crate::runtime::folders::{CreatedIds, creation_id};
use crate::store::{AccountKey, EmailId, ThreadId};

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
    /// The blob ids of a create no part of the account answers to.
    BlobNotFound(Vec<String>),
    /// The message would pass what the bridge or the server takes.
    TooLarge,
    ServerFail,
    ServerUnavailable,
}

impl SetError {
    pub(super) fn object(&self) -> Value {
        let kind = match self {
            Self::Forbidden => "forbidden",
            Self::NotFound => "notFound",
            Self::InvalidPatch => "invalidPatch",
            Self::InvalidProperties(_) => "invalidProperties",
            Self::TooManyKeywords => "tooManyKeywords",
            Self::TooManyMailboxes => "tooManyMailboxes",
            Self::WillDestroy => "willDestroy",
            Self::BlobNotFound(_) => "blobNotFound",
            Self::TooLarge => "tooLarge",
            Self::ServerFail => "serverFail",
            Self::ServerUnavailable => "serverUnavailable",
        };
        match self {
            Self::InvalidProperties(properties) => {
                json!({ "type": kind, "properties": properties })
            }
            Self::BlobNotFound(ids) => json!({ "type": kind, "notFound": ids }),
            _ => json!({ "type": kind }),
        }
    }
}

/// What `created` answers for one draft (RFC 8621 section 4.6). The blob
/// of a whole message goes by its email id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Created {
    pub id: EmailId,
    pub thread: ThreadId,
    pub size: u32,
}

impl Created {
    fn object(&self) -> Value {
        json!({
            "id": self.id,
            "blobId": self.id,
            "threadId": self.thread,
            "size": self.size,
        })
    }
}

/// What a create writes: a draft built from its object, or a message
/// built already that goes as it stands, as the Sent copy of a send does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Source {
    Draft(Draft),
    Copy(Copied),
}

/// A message that goes into a mailbox as it stands. The destroy of the
/// call waits on it wherever that email lies, so the copy and the
/// original are never both lost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Copied {
    pub mailbox: String,
    pub built: Built,
}

impl Source {
    /// The one mailbox it goes to, an id or a creation id after `#`.
    pub(super) fn mailbox(&self) -> &str {
        match self {
            Self::Draft(draft) => &draft.mailbox,
            Self::Copy(copied) => &copied.mailbox,
        }
    }
}

/// What one call wants done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SetAsk {
    pub if_in_state: Option<String>,
    /// The creates whose object reads, by creation id.
    pub creates: Vec<(String, Source)>,
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
        let drafts = self
            .creates
            .iter()
            .map(|(id, source)| {
                let mut source = source.clone();
                if let Source::Draft(draft) = &mut source
                    && let Some(mailbox) =
                        creation_id(&draft.mailbox).and_then(|creation| created.get(creation))
                {
                    draft.mailbox.clone_from(mailbox);
                }
                (id.clone(), source)
            })
            .collect();
        Self {
            if_in_state: self.if_in_state.clone(),
            creates: drafts,
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
        /// Every create of the ask in its order.
        created: Vec<(String, Result<Created, SetError>)>,
        /// Every update of the ask in its order.
        updated: Vec<(String, Result<Updated, SetError>)>,
        /// Every destroy of the ask in its order.
        destroyed: Vec<(String, Result<(), SetError>)>,
    },
}

/// A map of objects by id, null when it holds none (RFC 8620 section 5.3).
pub(super) fn or_null(objects: Map<String, Value>) -> Value {
    if objects.is_empty() {
        Value::Null
    } else {
        Value::Object(objects)
    }
}

/// The creates that read by creation id, beside the refusal of each other.
fn read_creates(create: &Map<String, Value>) -> (Vec<(String, Source)>, Map<String, Value>) {
    let mut refused = Map::new();
    let mut drafts = Vec::new();
    for (id, object) in create {
        match create::read(object) {
            Ok(draft) => drafts.push((id.clone(), Source::Draft(draft))),
            Err(error) => {
                refused.insert(id.clone(), error.object());
            }
        }
    }
    (drafts, refused)
}

/// `Email/set`: on the first pass the creates and the updates that read
/// and the destroys are left for the request to do and the call answers
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
    let (drafts, not_created) = read_creates(&create);
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
    let mut not_destroyed = Map::new();
    // A replace whose new message never read keeps the old one. The
    // updates above are judged against the destroy as the request named it.
    if create.len() == 1 && drafts.is_empty() && destroy.len() == 1 {
        let held = destroy
            .drain(..)
            .map(|id| (id, SetError::ServerFail.object()));
        not_destroyed.extend(held);
    }
    let written = context.written.borrow();
    let Some(outcome) = *written else {
        *context.set_ask.borrow_mut() = Some(SetAsk {
            if_in_state: arguments.if_in_state,
            creates: drafts,
            updates: patches,
            destroy,
        });
        return Err(MethodError::ServerUnavailable);
    };
    let Outcome::Stored {
        old_state,
        new_state,
        created: drafted,
        updated: results,
        destroyed: gone,
    } = outcome
    else {
        return Err(MethodError::StateMismatch);
    };
    let mut lists = Lists {
        not_created,
        not_updated,
        not_destroyed,
        ..Lists::default()
    };
    for (id, result) in drafted {
        match result {
            Ok(draft) => lists.created.insert(id.clone(), draft.object()),
            Err(error) => lists.not_created.insert(id.clone(), error.object()),
        };
    }
    for (id, result) in results {
        lists.updated_as(id, result);
    }
    for (id, result) in gone {
        lists.destroyed_as(id, result);
    }
    Ok(answer(context.key, (*old_state, *new_state), lists))
}

/// The lists of a `/set` answer (RFC 8620 section 5.3).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct Lists {
    pub created: Map<String, Value>,
    pub updated: Map<String, Value>,
    pub destroyed: Vec<Value>,
    pub not_created: Map<String, Value>,
    pub not_updated: Map<String, Value>,
    pub not_destroyed: Map<String, Value>,
}

impl Lists {
    /// One update as it ended.
    pub(super) fn updated_as(&mut self, id: &str, result: &Result<Updated, SetError>) {
        match result {
            Ok(changed) => self.updated.insert(
                id.to_owned(),
                changed.clone().map_or(Value::Null, Value::Object),
            ),
            Err(error) => self.not_updated.insert(id.to_owned(), error.object()),
        };
    }

    /// One destroy as it ended.
    pub(super) fn destroyed_as(&mut self, id: &str, result: &Result<(), SetError>) {
        match result {
            Ok(()) => self.destroyed.push(Value::from(id)),
            Err(error) => {
                self.not_destroyed.insert(id.to_owned(), error.object());
            }
        }
    }
}

/// An `Email/set` answer of the account between the two states.
pub(super) fn answer(
    key: &AccountKey,
    (old_state, new_state): (u64, u64),
    lists: Lists,
) -> Map<String, Value> {
    let destroyed = if lists.destroyed.is_empty() {
        Value::Null
    } else {
        Value::Array(lists.destroyed)
    };
    let answer = [
        ("accountId", Value::from(key.as_str())),
        ("oldState", Value::from(old_state.to_string())),
        ("newState", Value::from(new_state.to_string())),
        ("created", or_null(lists.created)),
        ("updated", or_null(lists.updated)),
        ("destroyed", destroyed),
        ("notCreated", or_null(lists.not_created)),
        ("notUpdated", or_null(lists.not_updated)),
        ("notDestroyed", or_null(lists.not_destroyed)),
    ];
    answer
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect()
}

#[cfg(test)]
mod tests;
