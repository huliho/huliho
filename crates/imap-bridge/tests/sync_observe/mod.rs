// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What the sync tests read back from the rig and the models they
//! build.

use huliho_imap_bridge::testing::{Behavior, Mailboxes, Message};
use serde_json::{Value, json};

use crate::sync_rig::{ACCOUNT, Rig};

/// The same model behind a server that misbehaves this way.
pub fn behaving(mut mailboxes: Mailboxes, behavior: Behavior) -> Mailboxes {
    mailboxes.behavior = behavior;
    mailboxes
}

/// The messages `1..=count`.
pub fn mail(count: u32) -> Vec<Message> {
    (1..=count).map(Message::new).collect()
}

impl Rig {
    /// The Mailbox object of the folder with that wire name.
    pub fn mailbox(&self, imap_name: &str) -> Value {
        let id = self.folder(imap_name).id;
        let call = json!(["Mailbox/get", { "accountId": ACCOUNT, "ids": [id] }, "c1"]);
        self.call(&call)["list"][0].take()
    }

    /// `Email/get` for the ids with the named properties.
    pub fn emails(&self, ids: &[String], properties: &[&str]) -> Vec<Value> {
        let call = json!([
            "Email/get",
            { "accountId": ACCOUNT, "ids": ids, "properties": properties },
            "c1"
        ]);
        self.call(&call)["list"].as_array().unwrap().clone()
    }

    /// The UID FETCH ranges the server received, in order.
    pub fn fetched(&self) -> Vec<String> {
        self.fake
            .lines()
            .iter()
            .filter_map(|line| line.split_once("UID FETCH ").map(|(_, rest)| rest))
            .filter_map(|rest| rest.split_once(' ').map(|(range, _)| range.to_owned()))
            .collect()
    }
}
