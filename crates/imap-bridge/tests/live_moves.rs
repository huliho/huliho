// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Moves through `Email/set` against the compose Dovecot: archive, spam,
//! not spam, trash, delete forever in Trash and a move to a folder of
//! the user's with the message marked unread, each seen from a second
//! connection. Then what is read and unread in the inbox through the
//! keyword filters, and a first sync of the folders the moves reached
//! that keeps every email id.

#![cfg(feature = "live-targets")]

mod live_rig;

use std::collections::HashMap;

use huliho_imap_bridge::mailboxes::sync;
use huliho_imap_bridge::runtime::Link;
use huliho_imap_bridge::session::Session;
use huliho_imap_bridge::sync::{Cache, FolderSync, Step};
use huliho_imap_bridge::testing::TestConnector;
use live_rig::{Editor, USER, answer, bridge_session, cache, editor, link, mailbox_list, within};
use serde_json::{Value, json};

/// The header every appended message carries, so a later run finds what
/// an earlier one left.
const MARKER: (&str, &str) = ("X-Huliho-Live", "moves");

/// The folder of the user's a message moves to, created and removed by
/// the test.
const FOLDER: &str = "Huliho moves";

/// The folders the compose Dovecot always holds that the moves reach.
const HELD: [&str; 4] = ["INBOX", "Archive", "Junk", "Trash"];

/// The messages by number and the step that moves each.
const ARCHIVED: u32 = 1;
const JUNKED: u32 = 2;
const TRASHED: u32 = 3;
const FILED: u32 = 4;
const UNREAD: u32 = 5;

/// Enough of the inbox to find the appended messages among what else is
/// there.
const LISTING: usize = 200;

/// The domain of every Message-ID the test writes.
const DOMAIN: &str = "huliho.test";

/// The Message-ID of the message with that number, without its angle
/// brackets as JMAP shows it.
fn message_id(number: u32) -> String {
    format!("moves-{number}@{DOMAIN}")
}

/// The number of an appended message from the email's `messageId`.
fn number_of(email: &Value) -> Option<u32> {
    let id = email["messageId"][0].as_str()?;
    let local = id.strip_suffix(&format!("@{DOMAIN}"))?;
    local.strip_prefix("moves-")?.parse().ok()
}

fn message(number: u32) -> String {
    format!(
        "From: Sanne <{USER}>\r\nTo: {USER}\r\nSubject: Move {number}\r\n{}: {}\r\nMessage-ID: <{}>\r\nMIME-Version: 1.0\r\n\r\nBody of move {number}\r\n",
        MARKER.0,
        MARKER.1,
        message_id(number)
    )
}

/// Expunges what carries the marker from the folders the moves reach and
/// removes the folder an earlier run left, on connections of its own.
async fn sweep() {
    let mut sweeper = editor().await;
    for folder in HELD {
        within(sweeper.select(folder)).await.unwrap();
        let query = format!("HEADER {} {}", MARKER.0, MARKER.1);
        let uids = within(sweeper.uid_search(&query)).await.unwrap();
        if uids.is_empty() {
            continue;
        }
        let set: Vec<String> = uids.iter().map(u32::to_string).collect();
        let set = set.join(",");
        let store = format!("UID STORE {set} +FLAGS.SILENT (\\Deleted)");
        within(sweeper.run_command_and_check_ok(&store))
            .await
            .unwrap();
        within(sweeper.run_command_and_check_ok(&format!("UID EXPUNGE {set}")))
            .await
            .unwrap();
    }
    within(sweeper.logout()).await.unwrap();
    let mut remover = editor().await;
    if within(remover.delete(FOLDER)).await.is_ok() {
        within(remover.logout()).await.unwrap();
    }
}

/// How many messages of the folder answer the search, read by a look
/// that changes nothing there. A message marked `\Deleted` and not
/// expunged still counts.
async fn search(editor: &mut Editor, folder: &str, number: u32, criteria: &str) -> usize {
    within(editor.examine(folder)).await.unwrap();
    let query = format!("{criteria}HEADER Message-ID <{}>", message_id(number));
    within(editor.uid_search(&query)).await.unwrap().len()
}

async fn holds(editor: &mut Editor, folder: &str, number: u32) -> bool {
    search(editor, folder, number, "").await == 1
}

async fn unseen(editor: &mut Editor, folder: &str, number: u32) -> bool {
    search(editor, folder, number, "UNSEEN ").await == 1
}

/// The JMAP side: the cache, the link that writes through Dovecot and
/// the ids it answers.
struct Bridge {
    cache: Cache,
    link: Link<TestConnector>,
    roles: HashMap<String, String>,
    ids: HashMap<u32, String>,
}

impl Bridge {
    async fn call(&self, method: &str, mut arguments: Value) -> Value {
        arguments["accountId"] = json!(self.cache.key.as_str());
        answer(&self.cache, &self.link, &json!([method, arguments, "c1"])).await
    }

    /// The numbers of the appended messages among these ids.
    async fn numbers(&self, ids: &Value) -> Vec<u32> {
        let found = self
            .call(
                "Email/get",
                json!({ "ids": ids, "properties": ["messageId"] }),
            )
            .await;
        let mut numbers: Vec<u32> = found["list"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(number_of)
            .collect();
        numbers.sort_unstable();
        numbers
    }

    /// The appended messages the inbox lists, by number, matching the
    /// filter's keyword test when one is named.
    async fn in_inbox(&self, keyword: Option<(&str, &str)>) -> Vec<u32> {
        let mut filter = json!({ "inMailbox": self.roles["inbox"] });
        if let Some((test, keyword)) = keyword {
            filter[test] = json!(keyword);
        }
        let query = json!({ "filter": filter, "limit": LISTING });
        let listed = self.call("Email/query", query).await;
        self.numbers(&listed["ids"]).await
    }

    /// One `Email/set` update for the message with that number; it
    /// stands and the state moves.
    async fn update(&self, number: u32, patch: Value) {
        let id = &self.ids[&number];
        let set = self
            .call("Email/set", json!({ "update": { id: patch } }))
            .await;
        assert!(set["updated"].get(id).is_some(), "{set}");
        assert_ne!(set["oldState"], set["newState"], "{set}");
    }

    /// A patch that takes the email out of the first mailbox and into
    /// the second.
    fn moving(&self, from: &str, to: &str) -> Value {
        let (from, to) = (&self.roles[from], &self.roles[to]);
        json!({ format!("mailboxIds/{from}"): null, format!("mailboxIds/{to}"): true })
    }

    /// The mailboxes of the message with that number, by role or name.
    async fn mailboxes_of(&self, number: u32) -> Vec<String> {
        let ids = json!([self.ids[&number]]);
        let got = self
            .call(
                "Email/get",
                json!({ "ids": ids, "properties": ["mailboxIds"] }),
            )
            .await;
        let held = got["list"][0]["mailboxIds"].as_object().unwrap();
        let mut named: Vec<String> = self
            .roles
            .iter()
            .filter(|(_, id)| held.contains_key(id.as_str()))
            .map(|(name, _)| name.clone())
            .collect();
        named.sort_unstable();
        named
    }
}

/// The ids of every mailbox by role, the folder of the user's by its
/// name; each one lets mail in and out, since Dovecot moves.
async fn roles(cache: &Cache, link: &Link<TestConnector>) -> HashMap<String, String> {
    let mut roles = HashMap::new();
    for mailbox in mailbox_list(cache, link).await {
        assert_eq!(mailbox["myRights"]["mayAddItems"], true, "{mailbox}");
        assert_eq!(mailbox["myRights"]["mayRemoveItems"], true, "{mailbox}");
        let name = match mailbox["role"].as_str() {
            Some(role) => role.to_owned(),
            None => mailbox["name"].as_str().unwrap().to_owned(),
        };
        roles.insert(name, mailbox["id"].as_str().unwrap().to_owned());
    }
    roles
}

/// The first sync of the folders with these names, on one session.
async fn first_sync(cache: &Cache, names: &[&str]) {
    let mut session = bridge_session().await;
    sync(&mut session, cache).await.unwrap();
    let rows = cache.store.mailbox_snapshot(&cache.key).unwrap().rows;
    for name in names {
        let row = rows
            .iter()
            .find(|row| row.facts.imap_name == *name)
            .unwrap();
        let mut folder = FolderSync::open(&mut session, cache, row)
            .await
            .unwrap()
            .unwrap_or_else(|| panic!("{name} waits for its first sync"));
        assert_eq!(
            folder.finish(&mut session, cache).await.unwrap(),
            Step::Done
        );
    }
    session.logout().await.unwrap();
}

/// The account with the five messages in the inbox, all read but one,
/// the inbox synced and every message's id known.
async fn start(editor: &mut Editor) -> Bridge {
    within(editor.create(FOLDER)).await.unwrap();
    for number in ARCHIVED..=UNREAD {
        let flags = (number != UNREAD).then_some("(\\Seen)");
        within(editor.append("INBOX", flags, None, message(number)))
            .await
            .unwrap();
    }
    let cache = cache("live-moves");
    first_sync(&cache, &["INBOX"]).await;
    let link = link();
    let roles = roles(&cache, &link).await;
    let mut bridge = Bridge {
        cache,
        link,
        roles,
        ids: HashMap::new(),
    };
    let query = json!({ "filter": { "inMailbox": bridge.roles["inbox"] }, "limit": LISTING });
    let listed = bridge.call("Email/query", query).await;
    let got = bridge
        .call(
            "Email/get",
            json!({ "ids": listed["ids"], "properties": ["id", "messageId"] }),
        )
        .await;
    for email in got["list"].as_array().unwrap() {
        if let Some(number) = number_of(email) {
            let id = email["id"].as_str().unwrap().to_owned();
            bridge.ids.insert(number, id);
        }
    }
    let appended = usize::try_from(UNREAD).unwrap();
    assert_eq!(bridge.ids.len(), appended, "{:?}", bridge.ids);
    bridge
}

#[tokio::test]
async fn dovecot_moves_through_email_set_and_a_second_connection_sees_each() {
    sweep().await;
    let mut editor = editor().await;
    let bridge = start(&mut editor).await;

    bridge
        .update(ARCHIVED, bridge.moving("inbox", "archive"))
        .await;
    assert_eq!(bridge.mailboxes_of(ARCHIVED).await, ["archive"]);
    assert!(!holds(&mut editor, "INBOX", ARCHIVED).await);
    assert!(holds(&mut editor, "Archive", ARCHIVED).await);

    bridge.update(JUNKED, bridge.moving("inbox", "junk")).await;
    assert_eq!(bridge.mailboxes_of(JUNKED).await, ["junk"]);
    assert!(holds(&mut editor, "Junk", JUNKED).await);
    bridge.update(JUNKED, bridge.moving("junk", "inbox")).await;
    assert_eq!(bridge.mailboxes_of(JUNKED).await, ["inbox"]);
    assert!(!holds(&mut editor, "Junk", JUNKED).await);
    assert!(holds(&mut editor, "INBOX", JUNKED).await);

    bridge
        .update(TRASHED, bridge.moving("inbox", "trash"))
        .await;
    assert_eq!(bridge.mailboxes_of(TRASHED).await, ["trash"]);
    assert!(holds(&mut editor, "Trash", TRASHED).await);
    let trashed = &bridge.ids[&TRASHED];
    let destroyed = bridge
        .call("Email/set", json!({ "destroy": [trashed] }))
        .await;
    assert_eq!(destroyed["destroyed"], json!([trashed]), "{destroyed}");
    let gone = bridge.call("Email/get", json!({ "ids": [trashed] })).await;
    assert_eq!(gone["notFound"], json!([trashed]), "{gone}");
    assert!(!holds(&mut editor, "Trash", TRASHED).await);

    let mut filed = bridge.moving("inbox", FOLDER);
    filed["keywords/$seen"] = Value::Null;
    bridge.update(FILED, filed).await;
    assert_eq!(bridge.mailboxes_of(FILED).await, [FOLDER]);
    assert!(holds(&mut editor, FOLDER, FILED).await);
    assert!(unseen(&mut editor, FOLDER, FILED).await);

    assert_eq!(bridge.in_inbox(None).await, [JUNKED, UNREAD]);
    let unread = bridge.in_inbox(Some(("notKeyword", "$seen"))).await;
    assert_eq!(unread, [UNREAD]);
    let read = bridge.in_inbox(Some(("hasKeyword", "$seen"))).await;
    assert_eq!(read, [JUNKED]);

    let before = json!(
        bridge
            .cache
            .store
            .state(&bridge.cache.key)
            .unwrap()
            .to_string()
    );
    first_sync(&bridge.cache, &["Archive", FOLDER]).await;
    let changes = bridge
        .call("Email/changes", json!({ "sinceState": before }))
        .await;
    assert!(
        bridge.numbers(&changes["created"]).await.is_empty(),
        "{changes}"
    );
    assert_eq!(changes["destroyed"], json!([]), "{changes}");
    assert_eq!(bridge.mailboxes_of(ARCHIVED).await, ["archive"]);
    assert_eq!(bridge.mailboxes_of(FILED).await, [FOLDER]);

    within(editor.logout()).await.unwrap();
    sweep().await;
}
