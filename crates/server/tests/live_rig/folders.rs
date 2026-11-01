// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The folder cycle through `Mailbox/set`, each step seen from the
//! second connection: a folder and one under it created in one call,
//! renamed with both ids kept, hidden and shown, a role moved between
//! mailboxes, the destroy refused while the folder holds a child or mail
//! and then made with its mail. Beside it Dovecot's `Locked`, whose
//! rights the server narrows: every folder command on it and every move
//! out of it refused.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use async_imap::imap_proto::{MailboxDatum, Response, Status};
use huliho_imap_bridge::utf7;
use serde_json::{Map, Value, json};
use tokio::time::sleep;

use super::{Editor, LOOK, Live, MAIL_ADDRESS, Mail, Target, append, editor, listed, within};

/// What every name of the cycle starts with, on the wire as typed.
const PREFIX: &str = "Cyclus ";

/// The names of the cycle, each outside ASCII, so the encoder runs live.
const FOLDER: &str = "Cyclus ü";
const RENAMED: &str = "Cyclus ö";
const CHILD: &str = "Kind é";

/// The hierarchy delimiter every compose target lists.
const DELIMITER: &str = "/";

/// The role the cycle moves: Dovecot's Archive holds it, no mailbox of
/// Cyrus does.
const ROLE: &str = "archive";

/// The folder whose rights Dovecot narrows to `ilrs`, and the message it
/// holds for good, since nothing in it can be expunged.
const LOCKED: &str = "Locked";
const LOCKED_MESSAGE_ID: &str = "<locked@huliho.test>";

/// The message the cycle files under its folder and destroys with it.
const CYCLE_MESSAGE_ID: &str = "<folder-cycle@huliho.test>";

/// The internal date both messages are appended with.
const APPENDED: &str = "\"01-Jan-2026 09:00:00 +0000\"";

/// How long the account may take to list its mailboxes or to sync a
/// message the second connection filed.
const PATIENCE: Duration = Duration::from_secs(60);

const LIST: &str = "LIST \"\" \"*\"";
const LSUB: &str = "LSUB \"\" \"*\"";

/// A path as the wire spells it, each name in modified UTF-7 (RFC 3501
/// section 5.1.3).
fn wire(path: &[&str]) -> String {
    let names: Vec<String> = path.iter().map(|name| utf7::encode(name)).collect();
    names.join(DELIMITER)
}

fn message(message_id: &str, subject: &str) -> String {
    format!(
        "From: {MAIL_ADDRESS}\r\nTo: {MAIL_ADDRESS}\r\nSubject: {subject}\r\n\
         Message-ID: {message_id}\r\nDate: Thu, 01 Jan 2026 09:00:00 +0000\r\n\r\n{subject}\r\n"
    )
}

/// The names a LIST or LSUB of the second connection answers, as the
/// wire spells them.
async fn names(editor: &mut Editor, command: &str) -> Vec<String> {
    let id = within(editor.run_command(command)).await.unwrap();
    let mut names = Vec::new();
    loop {
        let response = within(editor.read_response()).await.unwrap().unwrap();
        match response.parsed() {
            Response::MailboxData(MailboxDatum::List { name, .. }) => names.push(name.to_string()),
            Response::Done { tag, status, .. } if *tag == id => {
                assert_eq!(*status, Status::Ok, "{command}");
                return names;
            }
            _ => {}
        }
    }
}

/// Removes every folder of the cycle an earlier run left, deepest first
/// and only names the server lists: Dovecot's NO for a missing name
/// holds UTF-8, which the parser refuses.
async fn cleared(editor: &mut Editor) {
    let mut left: Vec<String> = names(editor, LIST).await;
    left.retain(|name| name.starts_with(PREFIX));
    left.sort_by_key(|name| std::cmp::Reverse(name.matches(DELIMITER).count()));
    for name in left {
        within(editor.delete(&name)).await.unwrap();
    }
    for name in names(editor, LSUB).await {
        if name.starts_with(PREFIX) {
            within(editor.unsubscribe(&name)).await.unwrap();
        }
    }
}

async fn set(live: &Live, mail: &Mail, mut arguments: Value) -> Value {
    arguments["accountId"] = json!(mail.account);
    live.call(mail, "Mailbox/set", arguments).await
}

/// The mailboxes of these ids as `Mailbox/get` shows them.
async fn got(live: &Live, mail: &Mail, ids: &[&str]) -> Vec<Value> {
    let arguments = json!({ "accountId": mail.account, "ids": ids });
    live.call(mail, "Mailbox/get", arguments).await["list"]
        .as_array()
        .unwrap()
        .clone()
}

/// The id of the listed object that answers to the test.
fn id_where(objects: &[Value], test: impl Fn(&Value) -> bool) -> Option<String> {
    let found = objects.iter().find(|object| test(object))?;
    Some(found["id"].as_str().unwrap().to_owned())
}

/// A `/changes` call, after which a bridge account refreshes once its
/// interval has passed, as a client's poll has it.
async fn poll(live: &Live, mail: &Mail) {
    let state = live.state(mail).await;
    let arguments = json!({ "accountId": mail.account, "sinceState": state });
    live.call(mail, "Email/changes", arguments).await;
}

/// The ids of the mailbox's emails once the account shows `count` of
/// them, synced where the bridge serves it.
async fn holding(live: &Live, mail: &Mail, mailbox: &str, count: u64) -> Vec<String> {
    let until = Instant::now() + PATIENCE;
    loop {
        poll(live, mail).await;
        let shown = got(live, mail, &[mailbox]).await.remove(0);
        let total = shown["totalEmails"].as_u64();
        let synced = !mail.vendor || shown["syncedEmails"].as_u64() == total;
        if total >= Some(count) && synced {
            let filter = json!({ "inMailbox": mailbox });
            let arguments = json!({ "accountId": mail.account, "filter": filter });
            return listed(&live.call(mail, "Email/query", arguments).await, "ids");
        }
        assert!(Instant::now() < until, "{shown}");
        sleep(LOOK).await;
    }
}

/// One run of the cycle: the account, the second connection and the
/// folder's names before and after its rename. Both names carry the
/// run's stamp: Cyrus keeps a deleted mailbox under its name and the
/// second it went, so a second delete of one name in that second fails.
struct Cycle<'a> {
    live: &'a Live,
    mail: &'a Mail,
    editor: &'a mut Editor,
    folder: String,
    renamed: String,
}

/// The folder cycle on the target, leaving it as it was found.
pub async fn cycle(target: Target) {
    let live = Live::start().await;
    let mut editor = editor(target).await;
    cleared(&mut editor).await;
    let mail = live.add(&target.body(), target.is_bridge()).await;
    let mailboxes = live.wait_listed(&mail, PATIENCE).await;
    let holder = id_where(&mailboxes, |mailbox| mailbox["role"] == ROLE);
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let stamp = stamp.as_nanos();
    let mut run = Cycle {
        live: &live,
        mail: &mail,
        editor: &mut editor,
        folder: format!("{FOLDER} {stamp}"),
        renamed: format!("{RENAMED} {stamp}"),
    };
    let (folder, child) = run.created().await;
    run.moved_along(&folder, &child).await;
    run.hidden_and_shown(&folder).await;
    let holder = holder.as_deref();
    moved(&live, &mail, holder, Some(&folder)).await;
    moved(&live, &mail, Some(&folder), Some(&child)).await;
    moved(&live, &mail, Some(&child), holder).await;
    run.destroyed(&folder, &child).await;
    within(editor.logout()).await.unwrap();
    live.remove(&mail).await;
}

impl Cycle<'_> {
    /// A folder and one under it in one call, the child naming its parent
    /// by creation id (RFC 8620 section 5.3); the second connection lists
    /// both.
    async fn created(&mut self) -> (String, String) {
        let create = json!({
            "f": { "name": self.folder, "parentId": null },
            "c": { "name": CHILD, "parentId": "#f" },
        });
        let answer = set(self.live, self.mail, json!({ "create": create })).await;
        let id = |key: &str| {
            let id = answer["created"][key]["id"].as_str();
            id.unwrap_or_else(|| panic!("{answer}")).to_owned()
        };
        let (folder, child) = (id("f"), id("c"));
        let shown = got(self.live, self.mail, &[&child]).await;
        assert_eq!(shown[0]["parentId"], folder.as_str(), "{shown:?}");
        let now = names(self.editor, LIST).await;
        assert!(now.contains(&wire(&[&self.folder])), "{now:?}");
        assert!(now.contains(&wire(&[&self.folder, CHILD])), "{now:?}");
        (folder, child)
    }

    /// The folder renamed with the one under it: both keep their ids (RFC
    /// 8621 section 2.5) and the second connection lists the new path
    /// alone.
    async fn moved_along(&mut self, folder: &str, child: &str) {
        let update = json!({ (folder): { "name": self.renamed } });
        let answer = set(self.live, self.mail, json!({ "update": update })).await;
        assert!(answer["updated"].get(folder).is_some(), "{answer}");
        for shown in got(self.live, self.mail, &[folder, child]).await {
            if shown["id"] == folder {
                assert_eq!(shown["name"], self.renamed.as_str(), "{shown}");
            } else {
                assert_eq!(shown["name"], CHILD, "{shown}");
                assert_eq!(shown["parentId"], folder, "{shown}");
            }
        }
        let now = names(self.editor, LIST).await;
        assert!(now.contains(&wire(&[&self.renamed, CHILD])), "{now:?}");
        assert!(!now.contains(&wire(&[&self.folder])), "{now:?}");
    }

    /// Hidden, then shown again (RFC 8621 section 2: `isSubscribed`), as
    /// the second connection's LSUB reads it.
    async fn hidden_and_shown(&mut self, folder: &str) {
        for subscribed in [false, true] {
            let update = json!({ (folder): { "isSubscribed": subscribed } });
            let answer = set(self.live, self.mail, json!({ "update": update })).await;
            assert!(answer["notUpdated"].is_null(), "{answer}");
            let shown = got(self.live, self.mail, &[folder]).await.remove(0);
            assert_eq!(shown["isSubscribed"], subscribed, "{shown}");
            let now = names(self.editor, LSUB).await;
            let listed = now.contains(&wire(&[&self.renamed]));
            assert_eq!(listed, subscribed, "{now:?}");
        }
    }

    /// Refused while the folder holds a child (RFC 8621 section 2.5) and
    /// while the child holds mail without `onDestroyRemoveEmails`. Then
    /// the child goes with its mail and the log names it, on the bridge
    /// the email too; the folder goes after it. Cyrus answers an
    /// `Email/changes` from before the destroy with
    /// `cannotCalculateChanges`, so the email is looked up there instead.
    async fn destroyed(&mut self, folder: &str, child: &str) {
        let (live, mail) = (self.live, self.mail);
        let filed = message(CYCLE_MESSAGE_ID, "Folder cycle");
        append(
            self.editor,
            &wire(&[&self.renamed, CHILD]),
            APPENDED,
            &filed,
        )
        .await;
        let email = holding(live, mail, child, 1).await.remove(0);
        let answer = set(live, mail, json!({ "destroy": [folder] })).await;
        assert_eq!(
            answer["notDestroyed"][folder]["type"], "mailboxHasChild",
            "{answer}"
        );
        let answer = set(live, mail, json!({ "destroy": [child] })).await;
        assert_eq!(
            answer["notDestroyed"][child]["type"], "mailboxHasEmail",
            "{answer}"
        );
        let before = live.state(mail).await;
        let arguments = json!({ "destroy": [child], "onDestroyRemoveEmails": true });
        let answer = set(live, mail, arguments).await;
        assert_eq!(answer["destroyed"], json!([child]), "{answer}");
        let since = json!({ "accountId": mail.account, "sinceState": answer["oldState"] });
        let changed = live.call(mail, "Mailbox/changes", since).await;
        assert!(
            listed(&changed, "destroyed").iter().any(|id| id == child),
            "{changed}"
        );
        let arguments = json!({ "accountId": mail.account, "ids": [&email], "properties": ["id"] });
        let looked = live.call(mail, "Email/get", arguments).await;
        assert_eq!(looked["notFound"], json!([&email]), "{looked}");
        if mail.vendor {
            let gone = |answer: &Value| listed(answer, "destroyed").contains(&email);
            let changed = live.changes_showing(mail, &before, gone).await;
            assert!(gone(&changed), "{changed}");
        }
        let answer = set(live, mail, json!({ "destroy": [folder] })).await;
        assert_eq!(answer["destroyed"], json!([folder]), "{answer}");
        let now = names(self.editor, LIST).await;
        let renamed = wire(&[&self.renamed]);
        assert!(
            !now.iter().any(|name| name.starts_with(&renamed)),
            "{now:?}"
        );
    }
}

/// The role moves in one call from one mailbox to another, where either
/// end may be none; `Mailbox/get` reads it back (RFC 8621 section 2: one
/// mailbox per role).
async fn moved(live: &Live, mail: &Mail, from: Option<&str>, to: Option<&str>) {
    let mut update = Map::new();
    if let Some(from) = from {
        update.insert(from.to_owned(), json!({ "role": null }));
    }
    if let Some(to) = to {
        update.insert(to.to_owned(), json!({ "role": ROLE }));
    }
    let answer = set(live, mail, json!({ "update": update })).await;
    assert!(answer["notUpdated"].is_null(), "{answer}");
    let ids: Vec<&str> = from.into_iter().chain(to).collect();
    for shown in got(live, mail, &ids).await {
        let takes = to.is_some_and(|to| shown["id"] == to);
        let role = if takes { json!(ROLE) } else { Value::Null };
        assert_eq!(shown["role"], role, "{shown}");
    }
}

/// Whether the second connection finds the message `Locked` keeps.
async fn holds_locked(editor: &mut Editor) -> bool {
    within(editor.examine(LOCKED)).await.unwrap();
    let query = format!("HEADER Message-ID \"{LOCKED_MESSAGE_ID}\"");
    !within(editor.uid_search(&query)).await.unwrap().is_empty()
}

/// Dovecot's `Locked`: the rights `ilrs` as `Mailbox/get` reads them. A
/// folder created under it, its rename, its destroy and a move or a
/// destroy of its message all answer `forbidden` (RFC 4314 section 4);
/// the second connection still finds the message there.
pub async fn locked() {
    let live = Live::start().await;
    let mut editor = editor(Target::Dovecot).await;
    if !holds_locked(&mut editor).await {
        let kept = message(LOCKED_MESSAGE_ID, "Locked");
        append(&mut editor, LOCKED, APPENDED, &kept).await;
    }
    let mail = live.add(&Target::Dovecot.body(), true).await;
    let mailboxes = live.wait_listed(&mail, PATIENCE).await;
    let locked = id_where(&mailboxes, |mailbox| mailbox["name"] == LOCKED).unwrap();
    let inbox = id_where(&mailboxes, |mailbox| mailbox["role"] == "inbox").unwrap();
    let shown = got(&live, &mail, &[&locked]).await.remove(0);
    let narrowed = json!({
        "mayReadItems": true, "mayAddItems": true, "mayRemoveItems": false,
        "maySetSeen": true, "maySetKeywords": false, "mayCreateChild": false,
        "mayRename": false, "mayDelete": false, "maySubmit": false,
    });
    assert_eq!(shown["myRights"], narrowed, "{shown}");
    refused_commands(&live, &mail, &locked).await;
    refused_moves(&live, &mail, &locked, &inbox).await;
    assert!(holds_locked(&mut editor).await);
    let now = names(&mut editor, LIST).await;
    assert!(now.iter().any(|name| name == LOCKED), "{now:?}");
    within(editor.logout()).await.unwrap();
    live.remove(&mail).await;
}

/// A folder created under the narrowed one, its rename and its destroy.
async fn refused_commands(live: &Live, mail: &Mail, locked: &str) {
    let create = json!({ "c": { "name": CHILD, "parentId": locked } });
    let update = json!({ (locked): { "name": RENAMED } });
    let answer = set(live, mail, json!({ "create": create, "update": update })).await;
    assert_eq!(answer["notCreated"]["c"]["type"], "forbidden", "{answer}");
    assert_eq!(
        answer["notUpdated"][locked]["type"], "forbidden",
        "{answer}"
    );
    let answer = set(live, mail, json!({ "destroy": [locked] })).await;
    assert_eq!(
        answer["notDestroyed"][locked]["type"], "forbidden",
        "{answer}"
    );
}

/// The message the narrowed folder keeps, moved to the inbox and
/// destroyed.
async fn refused_moves(live: &Live, mail: &Mail, locked: &str, inbox: &str) {
    let held = holding(live, mail, locked, 1).await;
    let emails = live.emails(mail, &held, &["messageId"]).await;
    let bare = LOCKED_MESSAGE_ID.trim_matches(['<', '>']);
    let kept = id_where(&emails, |email| email["messageId"][0] == bare);
    let email = kept.unwrap_or_else(|| panic!("{emails:?}"));
    let patch =
        json!({ format!("mailboxIds/{locked}"): null, format!("mailboxIds/{inbox}"): true });
    let arguments = json!({ "accountId": mail.account, "update": { (&email): patch } });
    let answer = live.call(mail, "Email/set", arguments).await;
    assert_eq!(
        answer["notUpdated"][&email]["type"], "forbidden",
        "{answer}"
    );
    let arguments = json!({ "accountId": mail.account, "destroy": [&email] });
    let answer = live.call(mail, "Email/set", arguments).await;
    assert_eq!(
        answer["notDestroyed"][&email]["type"], "forbidden",
        "{answer}"
    );
}
