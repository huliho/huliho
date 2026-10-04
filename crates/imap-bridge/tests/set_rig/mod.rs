// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What the `Email/set` tests share: a synced rig with its emails by
//! UID, the call, the commands of the write path the server received
//! and what the store and the server hold afterwards.

use std::collections::HashMap;

use huliho_imap_bridge::store::{ChangeKind, ObjectType};
use huliho_imap_bridge::testing::messages::SIZE_ABOVE_UID;
use huliho_imap_bridge::testing::{Mailboxes, Message};
use serde_json::{Value, json};

use crate::sync_rig::{ACCOUNT, Rig};

/// A rig over the model with the folder synced and the email ids by
/// UID.
pub async fn started(mailboxes: Mailboxes, folder: &str, gmail: bool) -> Set {
    let rig = if gmail {
        let mut rig = Rig::over(mailboxes).await;
        rig.cache.gmail = true;
        rig.pass().await.unwrap();
        rig
    } else {
        Rig::start(mailboxes).await
    };
    rig.sync(folder).await;
    let created = rig.created(0);
    let arguments = json!({ "accountId": ACCOUNT, "ids": created, "properties": ["size"] });
    let listed = rig.call(&json!(["Email/get", arguments, "c1"])).await;
    let ids = listed["list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|email| {
            let uid = email["size"].as_u64().unwrap() - u64::from(SIZE_ABOVE_UID);
            let id = email["id"].as_str().unwrap().to_owned();
            (u32::try_from(uid).unwrap(), id)
        })
        .collect();
    Set { rig, ids }
}

/// The rig with the email ids by UID.
pub struct Set {
    pub rig: Rig,
    pub ids: HashMap<u32, String>,
}

impl Set {
    /// `Email/set` with these arguments beside the account.
    pub async fn set(&self, mut arguments: Value) -> Value {
        arguments["accountId"] = json!(ACCOUNT);
        self.rig.call(&json!(["Email/set", arguments, "c1"])).await
    }

    /// `Email/set` with one patch per UID.
    pub async fn update(&self, patches: &[(u32, Value)]) -> Value {
        let update: serde_json::Map<String, Value> = patches
            .iter()
            .map(|(uid, patch)| (self.ids[uid].clone(), patch.clone()))
            .collect();
        self.set(json!({ "update": update })).await
    }

    /// Every SELECT and UID STORE the server received, without its tag.
    pub fn written(&self) -> Vec<String> {
        self.rig
            .fake
            .lines()
            .iter()
            .filter_map(|line| line.split_once(' ')?.1.split_once(' '))
            .map(|(_, command)| command.to_owned())
            .filter(|command| command.starts_with("SELECT ") || command.starts_with("UID STORE "))
            .collect()
    }

    /// The keywords the row of that UID holds, as `Email/get` shows them.
    pub async fn keywords(&self, uid: u32) -> Value {
        let arguments = json!({
            "accountId": ACCOUNT,
            "ids": [self.ids[&uid]],
            "properties": ["keywords"],
        });
        let got = self.rig.call(&json!(["Email/get", arguments, "c1"])).await;
        got["list"][0]["keywords"].clone()
    }

    /// The flags the server holds for that UID in the folder.
    pub fn flags(&self, folder: &str, uid: u32) -> Vec<String> {
        let folders = self.rig.fake.script().mailboxes.folders();
        let folder = folders.iter().find(|found| found.name == folder).unwrap();
        let message: &Message = folder.mail.iter().find(|found| found.uid == uid).unwrap();
        message.flags.clone()
    }

    /// The ids of a type the log names as updated after a state.
    pub fn updated(&self, object: ObjectType, since: u64) -> Vec<String> {
        let mut ids: Vec<String> = self
            .rig
            .changes(object, since)
            .into_iter()
            .filter(|(_, kind)| *kind == ChangeKind::Updated)
            .map(|(id, _)| id)
            .collect();
        ids.sort();
        ids
    }

    /// The state of the account.
    pub fn state(&self) -> u64 {
        self.rig.cache.store.state(&self.rig.cache.key).unwrap()
    }
}

/// The type of the error an update of that UID ended with.
pub fn refused<'a>(set: &Set, answer: &'a Value, uid: u32) -> &'a str {
    answer["notUpdated"][&set.ids[&uid]]["type"]
        .as_str()
        .unwrap_or_else(|| panic!("{answer}"))
}
