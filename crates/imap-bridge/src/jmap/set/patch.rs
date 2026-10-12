// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What a patch of `Email/set` reads as (RFC 8620 section 5.3): the
//! keywords it sets and clears and the mailboxes it adds and removes,
//! each as a whole object or one path at a time. Any other path or
//! value is named back.

use serde_json::Value;

use super::super::references::unescaped;
use super::SetError;
use crate::sync::mapping::canonical_keyword;

/// The two properties an update may patch.
const KEYWORDS: &str = "keywords";
const MAILBOX_IDS: &str = "mailboxIds";

/// The bytes of keywords one update may name, each with its separator,
/// and the bytes of flags or labels one STORE carries: with a full set
/// of UIDs the command then stays on one line.
pub(in crate::jmap) const MAX_UPDATE_KEYWORD_BYTES: usize = 4096;

/// What one update does to the keywords of its email, every keyword in
/// lower case.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::jmap) struct Edit {
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
    pub(in crate::jmap) fn path(&self, keyword: &str) -> String {
        path_of(&self.named, keyword, KEYWORDS)
    }
}

/// What one update does to the mailboxes of its email, each by id as
/// the patch wrote it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::jmap) struct MailboxEdit {
    /// The mailboxes the email enters.
    pub add: Vec<String>,
    /// The mailboxes it leaves.
    pub remove: Vec<String>,
    /// Whether `add` is every mailbox the email is left in.
    pub whole: bool,
    /// Each mailbox a path names, with that path as the patch wrote it.
    pub named: Vec<(String, String)>,
}

impl MailboxEdit {
    /// The path the patch named the mailbox by.
    pub(in crate::jmap) fn path(&self, mailbox: &str) -> String {
        path_of(&self.named, mailbox, MAILBOX_IDS)
    }
}

/// What one update asks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::jmap) struct Patch {
    pub keywords: Edit,
    /// `None` when the patch names no mailbox.
    pub mailboxes: Option<MailboxEdit>,
}

fn path_of(named: &[(String, String)], name: &str, whole: &str) -> String {
    let found = named.iter().find(|(found, _)| found == name);
    found.map_or_else(|| whole.to_owned(), |(_, path)| path.clone())
}

/// The keyword a path token names; `None` for a token that holds a
/// level below it, escapes badly or is no keyword.
fn keyword_of(token: &str) -> Option<String> {
    if token.contains('/') {
        return None;
    }
    canonical_keyword(&unescaped(token)?)
}

/// The mailbox id a path token names; whether it exists is the
/// account's to say.
fn mailbox_of(token: &str) -> Option<String> {
    if token.contains('/') {
        return None;
    }
    unescaped(token).filter(|id| !id.is_empty())
}

/// Adds a name once.
fn keep(names: &mut Vec<String>, name: String) {
    if !names.contains(&name) {
        names.push(name);
    }
}

/// A whole object: every key read by `read`, every value true.
fn whole(value: &Value, read: fn(&str) -> Option<String>) -> Option<Vec<String>> {
    let mut names = Vec::new();
    for (name, value) in value.as_object()? {
        if *value != Value::Bool(true) {
            return None;
        }
        keep(&mut names, read(name)?);
    }
    Some(names)
}

/// One path of a patch read into what it asks; `false` when the path or
/// its value is none the update may name.
fn read(into: &mut Patch, path: &str, value: &Value) -> bool {
    let inside = |property: &str| {
        path.strip_prefix(property)
            .and_then(|rest| rest.strip_prefix('/'))
    };
    let set = matches!(value, Value::Bool(true));
    let pathed = set || value.is_null();
    if path == KEYWORDS {
        let Some(keywords) = whole(value, canonical_keyword) else {
            return false;
        };
        into.keywords.whole = true;
        into.keywords.set = keywords;
        return true;
    }
    if path == MAILBOX_IDS {
        let edit = into.mailboxes.get_or_insert_with(MailboxEdit::default);
        let Some(mailboxes) = whole(value, mailbox_of) else {
            return false;
        };
        edit.whole = true;
        edit.add = mailboxes;
        return true;
    }
    if let Some(keyword) = inside(KEYWORDS).filter(|_| pathed).and_then(keyword_of) {
        let edit = &mut into.keywords;
        edit.named.push((keyword.clone(), path.to_owned()));
        keep(if set { &mut edit.set } else { &mut edit.clear }, keyword);
        return true;
    }
    if let Some(mailbox) = inside(MAILBOX_IDS).filter(|_| pathed).and_then(mailbox_of) {
        let edit = into.mailboxes.get_or_insert_with(MailboxEdit::default);
        edit.named.push((mailbox.clone(), path.to_owned()));
        keep(if set { &mut edit.add } else { &mut edit.remove }, mailbox);
        return true;
    }
    false
}

/// Reads one patch: `keywords` and `mailboxIds` as a whole or one
/// entry at a time with true to set it and null to take it off (RFC 8620
/// section 5.3). Any other path or value is named back; a patch that
/// names a whole object and an entry inside it is no patch, and neither
/// is one that sets and clears one keyword in two spellings. An email
/// left in no mailbox is invalid (RFC 8621 section 4.1.1).
pub(in crate::jmap) fn patch(value: &Value) -> Result<Patch, SetError> {
    let Value::Object(entries) = value else {
        return Err(SetError::InvalidPatch);
    };
    let mut patch = Patch::default();
    let invalid: Vec<String> = entries
        .iter()
        .filter(|(path, value)| !read(&mut patch, path, value))
        .map(|(path, _)| path.clone())
        .collect();
    if !invalid.is_empty() {
        return Err(SetError::InvalidProperties(invalid));
    }
    let keywords = &patch.keywords;
    let keyword_paths = entries
        .keys()
        .filter(|path| path.starts_with(KEYWORDS))
        .count();
    if keywords.whole && keyword_paths > 1 {
        return Err(SetError::InvalidPatch);
    }
    if keywords
        .set
        .iter()
        .any(|keyword| keywords.clear.contains(keyword))
    {
        return Err(SetError::InvalidPatch);
    }
    if let Some(mailboxes) = &patch.mailboxes {
        let mailbox_paths = entries
            .keys()
            .filter(|path| path.starts_with(MAILBOX_IDS))
            .count();
        if mailboxes.whole && mailbox_paths > 1 {
            return Err(SetError::InvalidPatch);
        }
        if mailboxes.whole && mailboxes.add.is_empty() {
            return Err(SetError::InvalidProperties(vec![MAILBOX_IDS.to_owned()]));
        }
    }
    let bytes: usize = keywords
        .set
        .iter()
        .chain(&keywords.clear)
        .map(|k| k.len() + 1)
        .sum();
    if bytes > MAX_UPDATE_KEYWORD_BYTES {
        return Err(SetError::TooManyKeywords);
    }
    Ok(patch)
}
