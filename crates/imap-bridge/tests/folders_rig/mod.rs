// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What the `Mailbox/set` tests share: a listed rig over a model, the
//! call, the folder commands the server received, the rows by wire name
//! and what the log says about them.

use huliho_imap_bridge::jmap::{CORE_CAPABILITY, HULIHO_CAPABILITY, MAIL_CAPABILITY, handle};
use huliho_imap_bridge::store::{ChangeKind, MailboxRow, ObjectType};
use huliho_imap_bridge::testing::{Folder, Mailboxes, Message};
use serde_json::{Map, Value, json};

use crate::sync_rig::{ACCOUNT, Rig};

/// A Dovecot with a folder `Work` holding `Q3` and a message in `Work`.
pub fn dovecot() -> Mailboxes {
    let mailboxes = Mailboxes::dovecot();
    mailboxes.push(Folder::new("Work").with_mail(vec![Message::new(1)]));
    mailboxes.push(Folder::new("Work/Q3"));
    mailboxes
}

/// The rig over the model, listed once; with `gmail` as a Gmail account.
pub async fn started(mailboxes: Mailboxes, gmail: bool) -> Rig {
    let mut rig = Rig::over(mailboxes).await;
    rig.cache.gmail = gmail;
    rig.pass().await.unwrap();
    rig
}

/// `Mailbox/set` with these arguments beside the account.
pub async fn set(rig: &Rig, mut arguments: Value) -> Value {
    arguments["accountId"] = json!(ACCOUNT);
    rig.call(&json!(["Mailbox/set", arguments, "c1"])).await
}

/// The Response object to these calls under every capability, with
/// `createdIds` where given.
pub async fn request(rig: &Rig, calls: &[Value], created_ids: Option<Value>) -> Value {
    let mut body = json!({
        "using": [CORE_CAPABILITY, MAIL_CAPABILITY, HULIHO_CAPABILITY],
        "methodCalls": calls,
    });
    if let Some(created_ids) = created_ids {
        body["createdIds"] = created_ids;
    }
    let bytes = serde_json::to_vec(&body).unwrap();
    let answer = handle(&rig.cache, &rig.link, &bytes, ACCOUNT)
        .await
        .unwrap();
    serde_json::from_slice(&answer).unwrap()
}

/// Every folder command the server received, without its tag.
pub fn sent(rig: &Rig) -> Vec<String> {
    let verbs = [
        "CREATE ",
        "RENAME ",
        "DELETE ",
        "SUBSCRIBE ",
        "UNSUBSCRIBE ",
        "MYRIGHTS ",
    ];
    rig.fake
        .lines()
        .iter()
        .filter_map(|line| line.split_once(' ')?.1.split_once(' '))
        .map(|(_, command)| command.to_owned())
        .filter(|command| verbs.iter().any(|verb| command.starts_with(verb)))
        .collect()
}

/// Every row by wire name.
pub fn rows(rig: &Rig) -> Vec<MailboxRow> {
    rig.cache
        .store
        .mailbox_snapshot(&rig.cache.key)
        .unwrap()
        .rows
}

/// The id of the row with that wire name.
pub fn id_of(rig: &Rig, imap_name: &str) -> String {
    rig.folder(imap_name).id.to_string()
}

/// The state of the account.
pub fn state(rig: &Rig) -> u64 {
    rig.cache.store.state(&rig.cache.key).unwrap()
}

/// The mailbox changes after a state with their kinds.
pub fn mailboxes_changed(rig: &Rig, since: u64) -> Vec<(String, ChangeKind)> {
    rig.changes(ObjectType::Mailbox, since)
}

/// The `SetError` type an object of that list answered with.
pub fn refused<'a>(answer: &'a Value, list: &str, id: &str) -> &'a str {
    answer[list][id]["type"]
        .as_str()
        .unwrap_or_else(|| panic!("{answer}"))
}

/// One `Mailbox/get` object by id.
pub async fn mailbox(rig: &Rig, id: &str) -> Map<String, Value> {
    let arguments = json!({ "accountId": ACCOUNT, "ids": [id] });
    let got = rig.call(&json!(["Mailbox/get", arguments, "c1"])).await;
    got["list"][0].as_object().cloned().unwrap_or_default()
}
