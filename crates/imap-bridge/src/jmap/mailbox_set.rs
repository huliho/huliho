// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! `Mailbox/set` (RFC 8620 section 5.3, RFC 8621 section 2.5): creates,
//! updates of `name`, `parentId`, `role` and `isSubscribed` and destroys
//! with `onDestroyRemoveEmails`. A call is read on the first pass over
//! its request. Between two passes the runtime checks every object
//! against the rows and does the rest on the server; the answer is
//! rendered from what came of that on the next pass.

use std::collections::HashSet;

use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::mailbox::{object, properties};
use super::{Context, HULIHO_CAPABILITY, MAX_OBJECTS_IN_SET, MethodError, arguments};
use crate::runtime::folders::{Ask, Create, Nullable, Outcome, Refusal, Update};
use crate::store::MailboxSnapshot;

/// The properties a client sets (RFC 8621 section 2); every other one
/// is the server's.
const SETTABLE: [&str; 4] = ["name", "parentId", "role", "isSubscribed"];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SetArguments {
    account_id: String,
    if_in_state: Option<String>,
    create: Option<Map<String, Value>>,
    update: Option<Map<String, Value>>,
    destroy: Option<Vec<String>>,
    on_destroy_remove_emails: Option<bool>,
}

/// The type of a `SetError` and its object (RFC 8620 section 5.3, RFC
/// 8621 section 2.5).
fn set_error(refusal: &Refusal) -> Value {
    let kind = match refusal {
        Refusal::Forbidden => "forbidden",
        Refusal::NotFound => "notFound",
        Refusal::InvalidProperties(_) => "invalidProperties",
        Refusal::MailboxHasChild => "mailboxHasChild",
        Refusal::MailboxHasEmail => "mailboxHasEmail",
        Refusal::WillDestroy => "willDestroy",
        Refusal::ServerFail => "serverFail",
        Refusal::ServerUnavailable => "serverUnavailable",
    };
    match refusal {
        Refusal::InvalidProperties(properties) => {
            json!({ "type": kind, "properties": properties })
        }
        _ => json!({ "type": kind }),
    }
}

/// The properties an object or a patch names that the client may not
/// set; a patch path into a property counts as its property.
fn unsettable(named: &Map<String, Value>) -> Vec<String> {
    named
        .keys()
        .filter(|key| !SETTABLE.contains(&key.as_str()))
        .cloned()
        .collect()
}

/// A string or null; anything else is invalid.
fn nullable(value: &Value) -> Result<Option<String>, ()> {
    match value {
        Value::Null => Ok(None),
        Value::String(text) => Ok(Some(text.clone())),
        _ => Err(()),
    }
}

/// One create object as the runtime takes it; the properties that do
/// not read where it does not.
fn create(creation_id: &str, object: &Value) -> Result<Create, Vec<String>> {
    let Some(object) = object.as_object() else {
        return Err(Vec::new());
    };
    let mut bad = unsettable(object);
    let name = object.get("name").and_then(Value::as_str);
    let parent = nullable(object.get("parentId").unwrap_or(&Value::Null));
    let role = object.get("role").map(nullable);
    let subscribed = object.get("isSubscribed").map(Value::as_bool);
    for (property, reads) in [
        ("name", name.is_some()),
        ("parentId", parent.is_ok()),
        ("role", role.as_ref().is_none_or(Result::is_ok)),
        ("isSubscribed", subscribed.is_none_or(|read| read.is_some())),
    ] {
        if !reads {
            bad.push(property.to_owned());
        }
    }
    match (name, parent, bad.is_empty()) {
        (Some(name), Ok(parent), true) => Ok(Create {
            creation_id: creation_id.to_owned(),
            name: name.to_owned(),
            parent,
            role: role.and_then(Result::ok).map(Nullable::from),
            subscribed: subscribed.flatten(),
        }),
        _ => Err(bad),
    }
}

/// One update patch as the runtime takes it; the properties that do not
/// read where it does not.
fn update(id: &str, patch: &Value) -> Result<Update, Vec<String>> {
    let Some(patch) = patch.as_object() else {
        return Err(Vec::new());
    };
    let mut bad = unsettable(patch);
    let name = patch
        .get("name")
        .map(|value| value.as_str().map(str::to_owned));
    let parent = patch.get("parentId").map(nullable);
    let role = patch.get("role").map(nullable);
    let subscribed = patch.get("isSubscribed").map(Value::as_bool);
    for (property, reads) in [
        ("name", name.as_ref().is_none_or(Option::is_some)),
        ("parentId", parent.as_ref().is_none_or(Result::is_ok)),
        ("role", role.as_ref().is_none_or(Result::is_ok)),
        ("isSubscribed", subscribed.is_none_or(|read| read.is_some())),
    ] {
        if !reads {
            bad.push(property.to_owned());
        }
    }
    if !bad.is_empty() {
        return Err(bad);
    }
    Ok(Update {
        id: id.to_owned(),
        name: name.flatten(),
        parent: parent.and_then(Result::ok).map(Nullable::from),
        role: role.and_then(Result::ok).map(Nullable::from),
        subscribed: subscribed.flatten(),
    })
}

fn invalid(properties: &[String]) -> Value {
    json!({ "type": "invalidProperties", "properties": properties })
}

/// A map of objects by id, null when it holds none (RFC 8620 section 5.3).
fn or_null(objects: Map<String, Value>) -> Value {
    if objects.is_empty() {
        Value::Null
    } else {
        Value::Object(objects)
    }
}

/// `Mailbox/set`: on the first pass the objects that read are left for
/// the request to do and the call answers `serverUnavailable`, which
/// stands when nothing could be done at all; with the outcome at hand
/// the call answers it. An update or a destroy that landed answers
/// under the id of its mailbox, one that failed under the id as written
/// (RFC 8620 section 5.3).
pub(super) fn set(
    context: &Context<'_>,
    raw: &Map<String, Value>,
) -> Result<Map<String, Value>, MethodError> {
    let arguments: SetArguments = arguments(raw)?;
    context.account(&arguments.account_id)?;
    let objects = arguments.create.unwrap_or_default();
    let patches = arguments.update.unwrap_or_default();
    let mut destroys = arguments.destroy.unwrap_or_default();
    if objects.len() + patches.len() + destroys.len() > MAX_OBJECTS_IN_SET {
        return Err(MethodError::RequestTooLarge);
    }
    let mut seen = HashSet::new();
    destroys.retain(|id| seen.insert(id.clone()));
    let mut answer = Answer::default();
    let mut ask = Ask {
        if_in_state: arguments.if_in_state,
        destroys,
        remove_emails: arguments.on_destroy_remove_emails.unwrap_or(false),
        ..Ask::default()
    };
    for (creation_id, object) in &objects {
        match create(creation_id, object) {
            Ok(create) => ask.creates.push(create),
            Err(bad) => {
                answer
                    .not_created
                    .insert(creation_id.clone(), invalid(&bad));
            }
        }
    }
    for (id, patch) in &patches {
        match update(id, patch) {
            Ok(update) => ask.updates.push(update),
            Err(bad) => {
                answer.not_updated.insert(id.clone(), invalid(&bad));
            }
        }
    }
    let done = context.folders_done.borrow();
    let Some(outcome) = *done else {
        *context.folder_ask.borrow_mut() = Some(ask);
        return Err(MethodError::ServerUnavailable);
    };
    let Outcome::Done {
        before,
        new_state,
        created,
        updated,
        destroyed,
    } = outcome
    else {
        return Err(MethodError::StateMismatch);
    };
    let snapshot = context.store.mailbox_snapshot(context.key)?;
    let huliho = context.using.has(HULIHO_CAPABILITY);
    for (creation_id, result) in created {
        match result {
            Ok(id) => {
                let sent = objects[creation_id].as_object();
                let mut object = rendered(&snapshot, id.as_str(), huliho)?;
                // The row is gone when the same call destroyed it; the id
                // is answered in any case (RFC 8620 section 5.3).
                object
                    .entry("id")
                    .or_insert_with(|| Value::from(id.as_str()));
                answer
                    .created
                    .insert(creation_id.clone(), beyond(object, sent, None));
            }
            Err(refusal) => {
                answer
                    .not_created
                    .insert(creation_id.clone(), set_error(refusal));
            }
        }
    }
    for (key, result) in updated {
        match result {
            Ok(id) => {
                let sent = patches[key].as_object();
                let held = rendered(before, id.as_str(), huliho)?;
                let shown = rendered(&snapshot, id.as_str(), huliho)?;
                let changed = beyond(shown, sent, Some(&held));
                answer.updated.insert(id.to_string(), changed);
            }
            Err(refusal) => {
                answer.not_updated.insert(key.clone(), set_error(refusal));
            }
        }
    }
    for (key, result) in destroyed {
        match result {
            Ok(id) => answer.destroyed.push(Value::from(id.as_str())),
            Err(refusal) => {
                answer.not_destroyed.insert(key.clone(), set_error(refusal));
            }
        }
    }
    Ok(answer.object(context.key.as_str(), (before.state, *new_state)))
}

/// The lists of a `/set` answer as they fill (RFC 8620 section 5.3).
#[derive(Default)]
struct Answer {
    created: Map<String, Value>,
    updated: Map<String, Value>,
    destroyed: Vec<Value>,
    not_created: Map<String, Value>,
    not_updated: Map<String, Value>,
    not_destroyed: Map<String, Value>,
}

impl Answer {
    /// The answer, every empty list null.
    fn object(self, account: &str, (old_state, new_state): (u64, u64)) -> Map<String, Value> {
        let destroyed = if self.destroyed.is_empty() {
            Value::Null
        } else {
            Value::Array(self.destroyed)
        };
        let answer = [
            ("accountId", Value::from(account)),
            ("oldState", Value::from(old_state.to_string())),
            ("newState", Value::from(new_state.to_string())),
            ("created", or_null(self.created)),
            ("updated", or_null(self.updated)),
            ("destroyed", destroyed),
            ("notCreated", or_null(self.not_created)),
            ("notUpdated", or_null(self.not_updated)),
            ("notDestroyed", or_null(self.not_destroyed)),
        ];
        answer
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect()
    }
}

/// A mailbox as `Mailbox/get` shows it in the snapshot; one it holds no
/// row for renders as nothing.
fn rendered(
    snapshot: &MailboxSnapshot,
    id: &str,
    huliho: bool,
) -> Result<Map<String, Value>, MethodError> {
    let wanted = properties(None, huliho)?;
    let row = snapshot.rows.iter().find(|row| row.id.as_str() == id);
    let object = row.map(|row| object(row, snapshot, &wanted));
    Ok(object
        .and_then(|object| object.as_object().cloned())
        .unwrap_or_default())
}

/// What a create or an update answers (RFC 8620 section 5.3): every
/// property the server holds otherwise than sent and, among the rest,
/// every one that `before`, the object before the call, held otherwise.
/// A create had no object before and answers every property it did not
/// send; an update answers null when nothing differs. A mailbox the
/// same call made has no object before its update either; `created`
/// shows it in full, so its update compares the sent properties alone.
fn beyond(
    rendered: Map<String, Value>,
    sent: Option<&Map<String, Value>>,
    before: Option<&Map<String, Value>>,
) -> Value {
    let differs = |(name, value): &(String, Value)| {
        if let Some(sent) = sent.and_then(|sent| sent.get(name)) {
            return sent != value;
        }
        before.is_none_or(|before| before.get(name).is_some_and(|held| held != value))
    };
    let kept: Map<String, Value> = rendered.into_iter().filter(differs).collect();
    if kept.is_empty() {
        Value::Null
    } else {
        Value::Object(kept)
    }
}

#[cfg(test)]
mod tests;
