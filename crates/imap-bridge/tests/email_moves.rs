// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Moves and destroys through `Email/set` on a folder account: a move is
//! `UID MOVE`, else `UID COPY` and an expunge of what it copied, with
//! the row rewritten to its new folder and UID in one state; a destroy
//! is `\Deleted` and `UID EXPUNGE` of those messages alone. A server
//! with neither MOVE nor UIDPLUS grants no move and gets none asked; an
//! email in two folders or in none is refused before anything is sent.

mod set_rig;
mod sync_rig;

use std::collections::BTreeSet;

use huliho_imap_bridge::store::{ChangeKind, ObjectType};
use huliho_imap_bridge::testing::{Extension, Folder, Mailboxes, Message};
use serde_json::{Value, json};
use set_rig::{Set, refused, started};
use sync_rig::{ACCOUNT, inbox};

const INBOX: &str = "INBOX";
const ARCHIVE: &str = "Archive";
const TRASH: &str = "Trash";

/// An INBOX of three messages beside an empty Archive and Trash, behind
/// the extensions given.
fn account(extensions: BTreeSet<Extension>) -> Mailboxes {
    let mut mailboxes = inbox((1..=3).map(Message::new).collect());
    mailboxes.push(Folder::special(ARCHIVE, "\\Archive"));
    mailboxes.push(Folder::special(TRASH, "\\Trash"));
    mailboxes.extensions = extensions;
    mailboxes
}

async fn synced(extensions: BTreeSet<Extension>) -> Set {
    started(account(extensions), INBOX, false).await
}

/// The extensions of Dovecot without the one named.
fn without(extension: Extension) -> BTreeSet<Extension> {
    let mut extensions = Extension::all();
    extensions.remove(&extension);
    extensions
}

/// The id of the folder of that wire name.
fn mailbox(set: &Set, name: &str) -> String {
    set.rig.folder(name).id.to_string()
}

/// A patch that takes an email from one folder to another.
fn move_patch(set: &Set, from: &str, to: &str) -> Value {
    let (from, to) = (mailbox(set, from), mailbox(set, to));
    json!({ format!("mailboxIds/{from}"): null, format!("mailboxIds/{to}"): true })
}

/// The UIDs the folder of that name holds on the server.
fn held(set: &Set, name: &str) -> Vec<u32> {
    let folders = set.rig.fake.script().mailboxes.folders();
    let folder = folders.iter().find(|folder| folder.name == name).unwrap();
    folder.mail.iter().map(|message| message.uid).collect()
}

/// The mailbox ids an email is a member of, as `Email/get` shows them.
async fn mailbox_ids(set: &Set, uid: u32) -> Value {
    let arguments = json!({
        "accountId": ACCOUNT,
        "ids": [set.ids[&uid]],
        "properties": ["mailboxIds"],
    });
    let got = set.rig.call(&json!(["Email/get", arguments, "c1"])).await;
    got["list"][0]["mailboxIds"].clone()
}

#[tokio::test]
async fn archive_is_one_uid_move_and_the_row_follows_under_its_new_uid_rfc6851_3_2() {
    let set = synced(Extension::all()).await;
    let before = set.state();
    let patch = move_patch(&set, INBOX, ARCHIVE);
    let answered = set.update(&[(1, patch.clone()), (2, patch)]).await;
    let both = json!({ &set.ids[&1]: null, &set.ids[&2]: null });
    assert_eq!(answered["updated"], both, "{answered}");
    assert_eq!(answered["newState"], (before + 1).to_string());
    assert_eq!(
        set.written(),
        ["SELECT \"INBOX\"", "UID MOVE 1:2 \"Archive\""]
    );
    assert_eq!(
        (held(&set, INBOX), held(&set, ARCHIVE)),
        (vec![3], vec![1, 2])
    );
    let archive = mailbox(&set, ARCHIVE);
    assert_eq!(mailbox_ids(&set, 1).await, json!({ &archive: true }));
    let mut moved = vec![set.ids[&1].clone(), set.ids[&2].clone()];
    moved.sort();
    assert_eq!(set.updated(ObjectType::Email, before), moved);
    let mut folders = vec![mailbox(&set, INBOX), archive];
    folders.sort();
    assert_eq!(set.updated(ObjectType::Mailbox, before), folders);
    assert!(set.rig.changes(ObjectType::Thread, before).is_empty());
    // The refresh finds each message where the move put it.
    let arguments = json!({ "accountId": ACCOUNT, "sinceState": answered["newState"] });
    let changed = set
        .rig
        .call(&json!(["Email/changes", arguments, "c1"]))
        .await;
    for list in ["created", "updated", "destroyed"] {
        assert_eq!(changed[list], json!([]), "{changed}");
    }
}

#[tokio::test]
async fn without_move_a_copy_and_an_expunge_of_what_it_copied_move_the_message_rfc4315_3() {
    let set = synced(without(Extension::Move)).await;
    let answered = set.update(&[(3, move_patch(&set, INBOX, TRASH))]).await;
    assert_eq!(
        answered["updated"],
        json!({ &set.ids[&3]: null }),
        "{answered}"
    );
    assert_eq!(
        set.written(),
        [
            "SELECT \"INBOX\"",
            "UID COPY 3 \"Trash\"",
            "UID STORE 3 +FLAGS.SILENT (\\Deleted)",
            "UID EXPUNGE 3",
        ]
    );
    assert_eq!(
        (held(&set, INBOX), held(&set, TRASH)),
        (vec![1, 2], vec![1])
    );
    assert_eq!(
        mailbox_ids(&set, 3).await,
        json!({ mailbox(&set, TRASH): true })
    );
}

#[tokio::test]
async fn a_server_with_neither_move_nor_uidplus_grants_no_move_and_gets_none_asked_rfc8621_2() {
    let mut extensions = without(Extension::Move);
    extensions.remove(&Extension::Uidplus);
    let set = synced(extensions).await;
    let arguments = json!({ "accountId": ACCOUNT, "ids": [mailbox(&set, INBOX)] });
    let got = set.rig.call(&json!(["Mailbox/get", arguments, "c1"])).await;
    let rights = &got["list"][0]["myRights"];
    assert_eq!(
        (&rights["mayAddItems"], &rights["mayRemoveItems"]),
        (&json!(false), &json!(false))
    );
    let answered = set
        .set(json!({
            "update": { &set.ids[&1]: move_patch(&set, INBOX, ARCHIVE) },
            "destroy": [set.ids[&2]],
        }))
        .await;
    assert_eq!(refused(&set, &answered, 1), "forbidden", "{answered}");
    assert_eq!(answered["notDestroyed"][&set.ids[&2]]["type"], "forbidden");
    assert!(set.written().is_empty(), "{:?}", set.written());
}

#[tokio::test]
async fn a_folder_account_refuses_an_email_in_two_folders_or_in_none_rfc8621_4_6() {
    let set = synced(Extension::all()).await;
    let (inbox, archive) = (mailbox(&set, INBOX), mailbox(&set, ARCHIVE));
    let answered = set
        .update(&[
            (1, json!({ format!("mailboxIds/{archive}"): true })),
            (2, json!({ format!("mailboxIds/{inbox}"): null })),
            (3, json!({ "mailboxIds/m-nowhere": true })),
        ])
        .await;
    let too_many = &answered["notUpdated"][&set.ids[&1]];
    assert_eq!(
        *too_many,
        json!({ "type": "tooManyMailboxes" }),
        "{answered}"
    );
    assert_eq!(refused(&set, &answered, 2), "invalidProperties");
    let named = &answered["notUpdated"][&set.ids[&2]]["properties"];
    assert_eq!(*named, json!([format!("mailboxIds/{inbox}")]));
    let unknown = &answered["notUpdated"][&set.ids[&3]]["properties"];
    assert_eq!(*unknown, json!(["mailboxIds/m-nowhere"]));
    assert!(set.written().is_empty());
}

#[tokio::test]
async fn destroy_stores_deleted_and_expunges_those_messages_alone_rfc4315_2_1() {
    let set = synced(Extension::all()).await;
    let before = set.state();
    let answered = set
        .set(json!({ "destroy": [set.ids[&2], set.ids[&2]] }))
        .await;
    assert_eq!(answered["destroyed"], json!([set.ids[&2]]), "{answered}");
    assert_eq!(answered["notDestroyed"], Value::Null);
    assert_eq!(
        set.written(),
        [
            "SELECT \"INBOX\"",
            "UID STORE 2 +FLAGS.SILENT (\\Deleted)",
            "UID EXPUNGE 2"
        ]
    );
    assert_eq!(held(&set, INBOX), [1, 3]);
    let destroyed = set.rig.changes(ObjectType::Email, before);
    assert_eq!(destroyed, [(set.ids[&2].clone(), ChangeKind::Destroyed)]);
    let threads = set.rig.changes(ObjectType::Thread, before);
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0].1, ChangeKind::Destroyed);
}

#[tokio::test]
async fn an_update_of_an_email_the_call_destroys_is_skipped_rfc8620_5_3() {
    let set = synced(Extension::all()).await;
    let answered = set
        .set(json!({
            "update": { &set.ids[&1]: { "keywords/$flagged": true } },
            "destroy": [set.ids[&1]],
        }))
        .await;
    assert_eq!(refused(&set, &answered, 1), "willDestroy", "{answered}");
    assert_eq!(answered["destroyed"], json!([set.ids[&1]]));
    assert!(!set.written().iter().any(|line| line.contains("\\Flagged")));
}

#[tokio::test]
async fn a_message_another_client_expunged_first_is_not_found_and_its_row_leaves() {
    let set = synced(Extension::all()).await;
    set.rig.fake.script().mailboxes.expunge_uid(INBOX, 1);
    let before = set.state();
    let answered = set.update(&[(1, move_patch(&set, INBOX, ARCHIVE))]).await;
    assert_eq!(refused(&set, &answered, 1), "notFound", "{answered}");
    let destroyed = set.rig.changes(ObjectType::Email, before);
    assert_eq!(destroyed, [(set.ids[&1].clone(), ChangeKind::Destroyed)]);
    let gone = set.set(json!({ "destroy": [set.ids[&1]] })).await;
    assert_eq!(gone["notDestroyed"][&set.ids[&1]]["type"], "notFound");
}

#[tokio::test]
async fn a_read_only_folder_takes_no_move_and_no_destroy_rfc3501_6_3_1() {
    let mailboxes = account(Extension::all());
    let mut folders = mailboxes.folders();
    folders[0].read_only = true;
    mailboxes.set(folders);
    let set = started(mailboxes, INBOX, false).await;
    let answered = set
        .set(json!({
            "update": { &set.ids[&1]: move_patch(&set, INBOX, ARCHIVE) },
            "destroy": [set.ids[&2]],
        }))
        .await;
    assert_eq!(refused(&set, &answered, 1), "forbidden", "{answered}");
    assert_eq!(answered["notDestroyed"][&set.ids[&2]]["type"], "forbidden");
    assert_eq!(set.written(), ["SELECT \"INBOX\""]);
    assert_eq!(held(&set, INBOX), [1, 2, 3]);
}

#[tokio::test]
async fn a_move_with_a_keyword_stores_the_keyword_first_and_the_move_carries_it() {
    let set = synced(Extension::all()).await;
    let mut patch = move_patch(&set, INBOX, ARCHIVE);
    patch["keywords/$junk"] = json!(true);
    let answered = set.update(&[(1, patch)]).await;
    assert_eq!(
        answered["updated"],
        json!({ &set.ids[&1]: null }),
        "{answered}"
    );
    assert_eq!(
        set.written(),
        [
            "SELECT \"INBOX\"",
            "UID STORE 1 +FLAGS.SILENT ($junk)",
            "UID MOVE 1 \"Archive\"",
        ]
    );
    assert_eq!(
        set.keywords(1).await,
        json!({ "$junk": true, "$seen": true })
    );
    assert_eq!(set.flags(ARCHIVE, 1), ["\\Seen", "$junk"]);
}
