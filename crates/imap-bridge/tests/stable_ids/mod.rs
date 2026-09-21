// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Random ids swapped for names that hold across runs, so a snapshot
//! and an assertion can name an email by its UID.

use std::collections::HashMap;

use serde_json::Value;

use crate::sync_rig::Rig;

/// The scripted size of a message is this much above its UID.
const SIZE_ABOVE_UID: u64 = 1000;

/// A name per id from Email objects that carry `id`, `size` and
/// `threadId`: the email `e<uid>`, its thread `t<uid>` after the lowest
/// UID it holds and the INBOX by its name.
pub fn names(rig: &Rig, emails: &[Value]) -> HashMap<String, String> {
    let mut emails: Vec<&Value> = emails.iter().collect();
    emails.sort_by_key(|email| email["size"].as_u64());
    let mut names = HashMap::from([(rig.folder("INBOX").id.to_string(), "INBOX".to_owned())]);
    for email in emails {
        let uid = email["size"].as_u64().unwrap() - SIZE_ABOVE_UID;
        names.insert(email["id"].as_str().unwrap().to_owned(), format!("e{uid}"));
        let thread = email["threadId"].as_str().unwrap().to_owned();
        names.entry(thread).or_insert(format!("t{uid}"));
    }
    names
}

/// Every string in `value` that names an id, as a value or as a key,
/// becomes the name.
pub fn rename(value: &mut Value, names: &HashMap<String, String>) {
    match value {
        Value::String(text) => {
            if let Some(name) = names.get(text.as_str()) {
                text.clone_from(name);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|item| rename(item, names)),
        Value::Object(fields) => {
            let renamed = std::mem::take(fields)
                .into_iter()
                .map(|(key, mut field)| {
                    rename(&mut field, names);
                    (names.get(&key).cloned().unwrap_or(key), field)
                })
                .collect();
            *fields = renamed;
        }
        _ => {}
    }
}
