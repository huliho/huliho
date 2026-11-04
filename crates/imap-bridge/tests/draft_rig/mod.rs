// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What the draft tests share: an account with a Drafts folder synced, the
//! create object of the client, the answers and what the server holds.

use std::collections::BTreeSet;

use huliho_imap_bridge::testing::{Extension, Folder, Mailboxes, Message};
use serde_json::{Value, json};

use crate::set_rig::{Set, started};
use crate::sync_rig::{ACCOUNT, inbox};

pub const INBOX: &str = "INBOX";
pub const DRAFTS: &str = "Drafts";

/// An INBOX of three messages beside an empty Drafts and Trash, behind
/// the extensions given.
pub fn account(extensions: BTreeSet<Extension>) -> Mailboxes {
    let mut mailboxes = inbox((1..=3).map(Message::new).collect());
    mailboxes.push(Folder::special(DRAFTS, "\\Drafts"));
    mailboxes.push(Folder::special("Trash", "\\Trash"));
    mailboxes.extensions = extensions;
    mailboxes
}

/// The extensions of Dovecot without those named.
pub fn without(names: &[Extension]) -> BTreeSet<Extension> {
    let mut extensions = Extension::all();
    for name in names {
        extensions.remove(name);
    }
    extensions
}

/// The account over the model, with both of its folders synced.
pub async fn synced(mailboxes: Mailboxes) -> Set {
    let set = Box::pin(started(mailboxes, INBOX, false)).await;
    Box::pin(set.rig.sync(DRAFTS)).await;
    set
}

/// The id of the Drafts folder.
pub fn drafts(set: &Set) -> String {
    set.rig.folder(DRAFTS).id.to_string()
}

/// The object the client sends, text and HTML into that mailbox.
pub fn object(mailbox: &str) -> Value {
    json!({
        "mailboxIds": { (mailbox): true },
        "keywords": { "$draft": true, "$seen": true },
        "from": [{ "name": "Sanne", "email": "sanne@example.test" }],
        "to": [
            { "name": "Mo, the first", "email": "mo@example.test" },
            { "email": "li@example.test" },
        ],
        "bcc": [{ "email": "hidden@example.test" }],
        "subject": "Caf\u{e9} om drie",
        "bodyStructure": {
            "type": "multipart/alternative",
            "subParts": [
                { "partId": "t", "header:Content-Type": " text/plain; format=flowed; delsp=yes" },
                { "partId": "h", "type": "text/html" },
            ],
        },
        "bodyValues": {
            "t": { "value": "Hallo \nwereld\n" },
            "h": { "value": "<p>Hallo wereld</p>" },
        },
    })
}

/// The object with a text of that many bytes.
pub fn sized(mailbox: &str, bytes: usize) -> Value {
    let mut object = object(mailbox);
    object["bodyStructure"] = json!({ "partId": "t", "type": "text/plain" });
    object["bodyValues"] = json!({ "t": { "value": "x".repeat(bytes) } });
    object
}

/// `Email/set` with one create under the creation id `d`.
pub async fn create(set: &Set, object: Value) -> Value {
    set.set(json!({ "create": { "d": object } })).await
}

/// `Email/set` with one create and one destroy.
pub async fn replace(set: &Set, object: Value, old: &str) -> Value {
    set.set(json!({ "create": { "d": object }, "destroy": [old] }))
        .await
}

/// The id a create answered.
pub fn created(answer: &Value) -> String {
    let id = answer["created"]["d"]["id"].as_str();
    id.unwrap_or_else(|| panic!("{answer}")).to_owned()
}

/// The type of the error a create ended with.
pub fn refused(answer: &Value) -> &str {
    let kind = answer["notCreated"]["d"]["type"].as_str();
    kind.unwrap_or_else(|| panic!("{answer}"))
}

/// The messages a folder holds on the server.
pub fn held(set: &Set, name: &str) -> Vec<Message> {
    let folders = set.rig.fake.script().mailboxes.folders();
    let folder = folders.iter().find(|folder| folder.name == name).unwrap();
    folder.mail.clone()
}

/// The command lines the server received that put a message in a mailbox,
/// without their tag and their literal.
pub fn appends(set: &Set) -> Vec<String> {
    let commands = set.rig.fake.lines();
    let lines = commands.iter().filter_map(|line| {
        let (_, tagged) = line.split_once(' ')?;
        let (_, command) = tagged.split_once(' ')?;
        let command = command.split("\r\n").next()?;
        (command.starts_with("APPEND ") || command.starts_with("UID REPLACE "))
            .then(|| command.to_owned())
    });
    lines.collect()
}

/// `Email/get` of one email with the named properties; null where the
/// account lacks it.
pub async fn email(set: &Set, id: &str, properties: &[&str]) -> Value {
    let arguments = json!({ "accountId": ACCOUNT, "ids": [id], "properties": properties });
    let mut got = set.rig.call(&json!(["Email/get", arguments, "c1"])).await;
    got["list"].get_mut(0).map_or(Value::Null, Value::take)
}

/// The next refresh finds the rows as the call wrote them.
pub async fn refreshed_equal(set: &Set, state: &Value) {
    let arguments = json!({ "accountId": ACCOUNT, "sinceState": state });
    let changed = set
        .rig
        .call(&json!(["Email/changes", arguments, "c1"]))
        .await;
    for list in ["created", "updated", "destroyed"] {
        assert_eq!(changed[list], json!([]), "{changed}");
    }
}
