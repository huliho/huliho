// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! `Identity/get`, `Identity/changes` and `Identity/set` (RFC 8621
//! section 6): one identity per account, the account's address with an
//! empty name, which no call changes.

use std::collections::HashSet;

use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::changes::{self, ChangesArguments};
use super::get::{self, GetArguments};
use super::set::or_null;
use super::{Context, MethodError, arguments, object_of};

/// The one state of the identities, since none of them ever moves.
pub(super) const IDENTITY_STATE: &str = "0";

/// The properties of RFC 8621 section 6.
const PROPERTIES: [&str; 8] = [
    "id",
    "name",
    "email",
    "replyTo",
    "bcc",
    "textSignature",
    "htmlSignature",
    "mayDelete",
];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SetArguments {
    account_id: String,
    if_in_state: Option<String>,
    create: Option<Map<String, Value>>,
    update: Option<Map<String, Value>>,
    destroy: Option<Vec<String>>,
}

/// `Identity/get`: the one identity when `ids` is null or names it.
pub(super) fn get(
    context: &Context<'_>,
    raw: &Map<String, Value>,
) -> Result<Map<String, Value>, MethodError> {
    let arguments: GetArguments = arguments(raw)?;
    context.account(&arguments.account_id)?;
    let wanted = get::properties(&PROPERTIES, arguments.properties.as_deref())?;
    let id = context.key.as_str();
    let asked = match arguments.ids.as_deref() {
        None => Vec::new(),
        named => get::ids(named)?,
    };
    let shown = arguments.ids.is_none() || asked.contains(&id);
    let list = if shown {
        vec![object(context, &wanted)]
    } else {
        Vec::new()
    };
    let found: HashSet<&str> = [id].into_iter().filter(|_| shown).collect();
    Ok(get::answer(context, (IDENTITY_STATE, list), &asked, &found))
}

/// `Identity/changes`: nothing changes, so the one state answers an
/// empty change and any other state cannot be followed.
pub(super) fn changes(
    context: &Context<'_>,
    raw: &Map<String, Value>,
) -> Result<Map<String, Value>, MethodError> {
    let arguments: ChangesArguments = arguments(raw)?;
    context.account(&arguments.account_id)?;
    changes::checked(&arguments)?;
    if arguments.since_state != IDENTITY_STATE {
        return Err(MethodError::CannotCalculateChanges);
    }
    let answer = json!({
        "accountId": context.key.as_str(),
        "oldState": IDENTITY_STATE,
        "newState": IDENTITY_STATE,
        "hasMoreChanges": false,
        "created": [],
        "updated": [],
        "destroyed": [],
    });
    Ok(object_of(answer))
}

/// `Identity/set`: every create, update and destroy is `forbidden`, since
/// the identity is the account's own address.
pub(super) fn set(
    context: &Context<'_>,
    raw: &Map<String, Value>,
) -> Result<Map<String, Value>, MethodError> {
    let arguments: SetArguments = arguments(raw)?;
    context.account(&arguments.account_id)?;
    if arguments
        .if_in_state
        .is_some_and(|state| state != IDENTITY_STATE)
    {
        return Err(MethodError::StateMismatch);
    }
    let forbidden = || json!({ "type": "forbidden" });
    let refused = |ids: Vec<String>| or_null(ids.into_iter().map(|id| (id, forbidden())).collect());
    let keys = |objects: Option<Map<String, Value>>| -> Vec<String> {
        objects
            .unwrap_or_default()
            .into_iter()
            .map(|(id, _)| id)
            .collect()
    };
    let answer = json!({
        "accountId": context.key.as_str(),
        "oldState": IDENTITY_STATE,
        "newState": IDENTITY_STATE,
        "created": null,
        "updated": null,
        "destroyed": null,
        "notCreated": refused(keys(arguments.create)),
        "notUpdated": refused(keys(arguments.update)),
        "notDestroyed": refused(arguments.destroy.unwrap_or_default()),
    });
    Ok(object_of(answer))
}

/// The one identity, cut to the wanted properties.
fn object(context: &Context<'_>, wanted: &[&str]) -> Value {
    let pairs = [
        ("id", json!(context.key.as_str())),
        ("name", json!("")),
        ("email", json!(context.address)),
        ("replyTo", Value::Null),
        ("bcc", Value::Null),
        ("textSignature", json!("")),
        ("htmlSignature", json!("")),
        ("mayDelete", json!(false)),
    ];
    pairs
        .into_iter()
        .filter(|(name, _)| wanted.contains(name))
        .map(|(name, value)| (name.to_owned(), value))
        .collect::<Map<_, _>>()
        .into()
}
