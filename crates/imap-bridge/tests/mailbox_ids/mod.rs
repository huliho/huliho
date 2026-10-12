// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The `mailboxIds` of an email by wire name: the id of one mailbox, a
//! patch over several and the mailboxes `Email/get` shows an email in.

use serde_json::{Value, json};

use crate::set_rig::Set;
use crate::sync_rig::ACCOUNT;

/// The id of the mailbox of that wire name.
pub fn mailbox(set: &Set, name: &str) -> String {
    set.rig.folder(name).id.to_string()
}

/// A patch that sets each mailbox of that name to the value.
pub fn patch(set: &Set, entries: &[(&str, Value)]) -> Value {
    let entries = entries
        .iter()
        .map(|(name, value)| (format!("mailboxIds/{}", mailbox(set, name)), value.clone()));
    Value::Object(entries.collect())
}

/// The mailboxes of an email by wire name, as `Email/get` shows them.
pub async fn members(set: &Set, uid: u32) -> Vec<String> {
    let id = &set.ids[&uid];
    let arguments = json!({ "accountId": ACCOUNT, "ids": [id], "properties": ["mailboxIds"] });
    let got = set.rig.call(&json!(["Email/get", arguments, "c1"])).await;
    let folders = set.rig.fake.script().mailboxes.folders();
    let mut names: Vec<String> = folders
        .iter()
        .map(|folder| folder.name.clone())
        .filter(|name| {
            got["list"][0]["mailboxIds"]
                .get(mailbox(set, name))
                .is_some()
        })
        .collect();
    names.sort();
    names
}
