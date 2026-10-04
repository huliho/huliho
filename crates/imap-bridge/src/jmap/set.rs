// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! `Email/set` (RFC 8620 section 5.3, RFC 8621 section 4.6): updates of
//! `keywords` alone. A call is read on the first pass over its request,
//! its flags are stored on the server between two passes and the answer
//! is rendered from what came of that on the next. Nothing else can be
//! set: a create and a destroy are forbidden per object.

use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::references::unescaped;
use super::{Context, MAX_OBJECTS_IN_SET, MethodError, arguments};
use crate::sync::mapping::canonical_keyword;

/// The property an update may patch.
const KEYWORDS: &str = "keywords";

/// The bytes of keywords one update may name, each with its separator,
/// and the bytes of flags one STORE carries: with a full set of UIDs
/// the command then stays on one line.
pub(super) const MAX_UPDATE_KEYWORD_BYTES: usize = 4096;

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

/// What one update does to the keywords of its email, every keyword in
/// lower case.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Edit {
    /// The keywords the email gains.
    pub set: Vec<String>,
    /// The keywords it loses.
    pub clear: Vec<String>,
    /// Whether `set` is every keyword the email is left with.
    pub whole: bool,
    /// Each keyword a path of the patch names, with that path as the
    /// patch wrote it.
    pub named: Vec<(String, String)>,
}

impl Edit {
    /// The path the patch named the keyword by: its own path, or the
    /// whole `keywords` object.
    pub(super) fn path(&self, keyword: &str) -> String {
        let named = self.named.iter().find(|(found, _)| found == keyword);
        named.map_or_else(|| KEYWORDS.to_owned(), |(_, path)| path.clone())
    }
}

/// What one call wants stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SetAsk {
    pub if_in_state: Option<String>,
    /// The updates whose patch reads, by email id as written.
    pub updates: Vec<(String, Edit)>,
}

/// What came of one ask.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Outcome {
    /// `ifInState` named another state than the account stood at.
    StateMismatch,
    Stored {
        old_state: u64,
        new_state: u64,
        /// Every update of the ask in its order.
        results: Vec<(String, Result<(), SetError>)>,
    },
}

/// The keyword a path token names; `None` for a token that holds a
/// level below it, escapes badly or is no keyword.
fn keyword_of(token: &str) -> Option<String> {
    if token.contains('/') {
        return None;
    }
    canonical_keyword(&unescaped(token)?)
}

/// Adds a keyword once.
fn keep(keywords: &mut Vec<String>, keyword: String) {
    if !keywords.contains(&keyword) {
        keywords.push(keyword);
    }
}

/// A whole `keywords` object: every key a keyword, every value true.
fn whole(value: &Value) -> Option<Vec<String>> {
    let mut keywords = Vec::new();
    for (name, value) in value.as_object()? {
        if *value != Value::Bool(true) {
            return None;
        }
        keep(&mut keywords, canonical_keyword(name)?);
    }
    Some(keywords)
}

/// Reads one patch: `keywords` as a whole or `keywords/<keyword>` with
/// true to set it and null to take it off (RFC 8620 section 5.3). Any
/// other path or value is named back; a patch that names the whole and
/// a keyword inside it is no patch.
fn edit(patch: &Value) -> Result<Edit, SetError> {
    let Value::Object(patch) = patch else {
        return Err(SetError::InvalidPatch);
    };
    let mut edit = Edit::default();
    let mut invalid = Vec::new();
    for (path, value) in patch {
        let inside = path
            .strip_prefix(KEYWORDS)
            .and_then(|rest| rest.strip_prefix('/'));
        let read = match (inside, value) {
            (None, value) if path == KEYWORDS => whole(value).map(|keywords| {
                edit.whole = true;
                edit.set = keywords;
            }),
            (Some(token), Value::Bool(true) | Value::Null) => keyword_of(token).map(|keyword| {
                edit.named.push((keyword.clone(), path.clone()));
                let list = if value.is_null() {
                    &mut edit.clear
                } else {
                    &mut edit.set
                };
                keep(list, keyword);
            }),
            _ => None,
        };
        if read.is_none() {
            invalid.push(path.clone());
        }
    }
    if !invalid.is_empty() {
        return Err(SetError::InvalidProperties(invalid));
    }
    if edit.whole && patch.len() > 1 {
        return Err(SetError::InvalidPatch);
    }
    if edit.set.iter().any(|keyword| edit.clear.contains(keyword)) {
        return Err(SetError::InvalidPatch);
    }
    let bytes: usize = edit
        .set
        .iter()
        .chain(&edit.clear)
        .map(|k| k.len() + 1)
        .sum();
    if bytes > MAX_UPDATE_KEYWORD_BYTES {
        return Err(SetError::TooManyKeywords);
    }
    Ok(edit)
}

/// A map of objects by id, null when it holds none (RFC 8620 section 5.3).
fn or_null(objects: Map<String, Value>) -> Value {
    if objects.is_empty() {
        Value::Null
    } else {
        Value::Object(objects)
    }
}

/// `Email/set`: on the first pass the updates that read are left for
/// the request to store and the call answers `serverUnavailable`, which
/// stands when nothing could be stored at all; with the outcome at hand
/// the call answers it.
pub(super) fn email(
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
    let forbidden = SetError::Forbidden.object();
    let not_created = create.keys().map(|id| (id.clone(), forbidden.clone()));
    let not_destroyed = destroy.iter().map(|id| (id.clone(), forbidden.clone()));
    let mut not_updated = Map::new();
    let mut edits = Vec::new();
    for (id, patch) in &update {
        match edit(patch) {
            Ok(edit) => edits.push((id.clone(), edit)),
            Err(error) => {
                not_updated.insert(id.clone(), error.object());
            }
        }
    }
    let written = context.written.borrow();
    let Some(outcome) = *written else {
        *context.set_ask.borrow_mut() = Some(SetAsk {
            if_in_state: arguments.if_in_state,
            updates: edits,
        });
        return Err(MethodError::ServerUnavailable);
    };
    let Outcome::Stored {
        old_state,
        new_state,
        results,
    } = outcome
    else {
        return Err(MethodError::StateMismatch);
    };
    let mut updated = Map::new();
    for (id, result) in results {
        match result {
            Ok(()) => updated.insert(id.clone(), Value::Null),
            Err(error) => not_updated.insert(id.clone(), error.object()),
        };
    }
    let answer = [
        ("accountId", Value::from(context.key.as_str())),
        ("oldState", Value::from(old_state.to_string())),
        ("newState", Value::from(new_state.to_string())),
        ("created", Value::Null),
        ("updated", or_null(updated)),
        ("destroyed", Value::Null),
        ("notCreated", or_null(not_created.collect())),
        ("notUpdated", or_null(not_updated)),
        ("notDestroyed", or_null(not_destroyed.collect())),
    ];
    Ok(answer
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect())
}

#[cfg(test)]
mod tests;
