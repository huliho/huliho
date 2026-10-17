// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Moves and destroys through `Email/set` on a Gmail account: archive and
//! every label are X-GM-LABELS stores on All Mail with no `\Deleted` and
//! no expunge; Starred and `$flagged` move together; entering Spam or
//! Trash is a move between the stores and leaving either a move into All
//! Mail with the labels stored and read back there; a destroy stays in
//! Spam and Trash. What the server changed beyond a patch is in the
//! answer (RFC 8620 section 5.3).

mod mailbox_ids;
mod set_rig;
mod sync_rig;

use huliho_imap_bridge::store::ObjectType;
use huliho_imap_bridge::testing::mailboxes::{ALL_MAIL, SPAM, TRASH};
use huliho_imap_bridge::testing::messages::SIZE_ABOVE_UID;
use huliho_imap_bridge::testing::{Extension, Folder, Mailboxes, Message};
use mailbox_ids::{mailbox, members, patch};
use serde_json::{Value, json};
use set_rig::{Set, refused, started};
use sync_rig::{ACCOUNT, inbox};

/// The user label of the account.
const WORK: &str = "Work";

/// The message that waits in Spam, under the user label.
const JUNKED: u32 = 4;

/// Three messages in the inbox with the second under the user label too;
/// one more under that label, which a second client moves to Spam.
fn mail() -> Mailboxes {
    let mail = vec![
        Message::new(1),
        Message::new(2).labeled(&["\\Inbox", WORK]),
        Message::new(3),
        Message::new(JUNKED).labeled(&["\\Inbox", WORK]),
    ];
    Mailboxes::gmail(mail).with_label(WORK)
}

/// The account over that mail.
async fn account() -> Set {
    synced(mail()).await
}

/// The same account on a Gmail server that does not offer MOVE.
async fn without_move() -> Set {
    let mut mailboxes = mail();
    mailboxes.extensions.remove(&Extension::Move);
    synced(mailboxes).await
}

/// The model with its last message moved to Spam, synced in All Mail and
/// in Spam, that message's id among the others.
async fn synced(mailboxes: Mailboxes) -> Set {
    mailboxes.move_to((ALL_MAIL, JUNKED), SPAM);
    let mut set = Box::pin(started(mailboxes, ALL_MAIL, true)).await;
    let before = set.state();
    Box::pin(set.rig.sync(SPAM)).await;
    let created = set.rig.created(before);
    let arguments = json!({ "accountId": ACCOUNT, "ids": created, "properties": ["size"] });
    let listed = set.rig.call(&json!(["Email/get", arguments, "c1"])).await;
    let size = listed["list"][0]["size"].as_u64().unwrap();
    assert_eq!(size, u64::from(SIZE_ABOVE_UID + JUNKED));
    set.ids.insert(JUNKED, created[0].clone());
    set
}

/// The labels the server holds for a message of a folder.
fn labels(set: &Set, folder: &str, uid: u32) -> Vec<String> {
    let folders = set.rig.fake.script().mailboxes.folders();
    let folder = folders.iter().find(|found| found.name == folder).unwrap();
    let message = folder.mail.iter().find(|found| found.uid == uid).unwrap();
    message.labels.clone()
}

/// Whether a command stored `\Deleted` or expunged.
fn expunges(written: &[String]) -> bool {
    written
        .iter()
        .any(|line| line.contains("\\Deleted") || line.contains("EXPUNGE"))
}

/// Whether emails may be added to and removed from the mailbox of that
/// wire name, as `Mailbox/get` shows it.
async fn moving(set: &Set, name: &str) -> (bool, bool) {
    let arguments = json!({
        "accountId": ACCOUNT,
        "ids": [mailbox(set, name)],
        "properties": ["myRights"],
    });
    let got = set.rig.call(&json!(["Mailbox/get", arguments, "c1"])).await;
    let rights = &got["list"][0]["myRights"];
    let right = |name: &str| rights[name].as_bool().unwrap_or_else(|| panic!("{got}"));
    (right("mayAddItems"), right("mayRemoveItems"))
}

#[tokio::test]
async fn archive_on_gmail_removes_the_inbox_label_and_nothing_else() {
    let set = account().await;
    let before = set.state();
    let archive = patch(&set, &[("INBOX", Value::Null), (ALL_MAIL, json!(true))]);
    let answered = set.update(&[(1, archive)]).await;
    assert_eq!(
        answered["updated"],
        json!({ &set.ids[&1]: null }),
        "{answered}"
    );
    assert_eq!(
        set.written(),
        [
            "SELECT \"[Gmail]/All Mail\"",
            "UID STORE 1 -X-GM-LABELS.SILENT (\\Inbox)",
        ]
    );
    let held = labels(&set, ALL_MAIL, 1);
    assert!(held.is_empty(), "{held:?}");
    assert_eq!(members(&set, 1).await, [ALL_MAIL]);
    assert_eq!(
        set.updated(ObjectType::Mailbox, before),
        [mailbox(&set, "INBOX")]
    );
}

#[tokio::test]
async fn a_label_is_stored_on_all_mail_as_gmail_spells_it() {
    let set = account().await;
    let moved = patch(&set, &[("INBOX", Value::Null), (WORK, json!(true))]);
    let answered = set.update(&[(3, moved)]).await;
    assert_eq!(
        answered["updated"],
        json!({ &set.ids[&3]: null }),
        "{answered}"
    );
    assert_eq!(
        set.written()[1..],
        [
            "UID STORE 3 +X-GM-LABELS.SILENT (\"Work\")",
            "UID STORE 3 -X-GM-LABELS.SILENT (\\Inbox)",
        ]
    );
    assert_eq!(labels(&set, ALL_MAIL, 3), [WORK]);
    assert_eq!(members(&set, 3).await, [WORK, ALL_MAIL]);
    assert!(!expunges(&set.written()));
}

#[tokio::test]
async fn a_second_mailbox_is_a_label_on_gmail_and_one_too_many_on_a_folder_account() {
    let set = account().await;
    let labeled = set
        .update(&[(1, patch(&set, &[(WORK, json!(true))]))])
        .await;
    assert_eq!(
        labeled["updated"],
        json!({ &set.ids[&1]: null }),
        "{labeled}"
    );
    let folders = inbox(vec![Message::new(1)]);
    folders.push(Folder::special("Archive", "\\Archive"));
    let folders = started(folders, "INBOX", false).await;
    let archive = mailbox(&folders, "Archive");
    let added = json!({ format!("mailboxIds/{archive}"): true });
    let answered = folders.update(&[(1, added)]).await;
    assert_eq!(
        refused(&folders, &answered, 1),
        "tooManyMailboxes",
        "{answered}"
    );
}

#[tokio::test]
async fn starred_and_flagged_move_together_and_the_answer_names_the_other_one() {
    let set = account().await;
    let starred = patch(&set, &[("[Gmail]/Starred", json!(true))]);
    let answered = set
        .update(&[(1, starred), (3, json!({ "keywords/$flagged": true }))])
        .await;
    let both = json!({ "$flagged": true, "$seen": true });
    assert_eq!(
        answered["updated"][&set.ids[&1]],
        json!({ "keywords": both }),
        "{answered}"
    );
    let flagged = &answered["updated"][&set.ids[&3]]["mailboxIds"];
    assert_eq!(
        flagged.get(mailbox(&set, "[Gmail]/Starred")),
        Some(&json!(true))
    );
    let lines = set.written();
    assert!(lines.contains(&"UID STORE 1 +X-GM-LABELS.SILENT (\\Starred)".to_owned()));
    assert!(lines.contains(&"UID STORE 3 +FLAGS.SILENT (\\Flagged)".to_owned()));
    assert_eq!(set.flags(ALL_MAIL, 1), ["\\Seen", "\\Flagged"]);
    assert_eq!(set.keywords(1).await, both);
    assert!(
        members(&set, 3)
            .await
            .contains(&"[Gmail]/Starred".to_owned())
    );
}

#[tokio::test]
async fn trash_on_gmail_is_a_move_between_the_stores_and_a_destroy_stays_there() {
    let set = account().await;
    let trash = patch(&set, &[("INBOX", Value::Null), (TRASH, json!(true))]);
    let answered = set.update(&[(2, trash)]).await;
    let alone = json!({ "mailboxIds": { mailbox(&set, TRASH): true } });
    assert_eq!(answered["updated"][&set.ids[&2]], alone, "{answered}");
    assert_eq!(set.written()[1..], ["UID MOVE 2 \"[Gmail]/Trash\""]);
    assert_eq!(members(&set, 2).await, [TRASH]);
    let destroyed = set.set(json!({ "destroy": [set.ids[&2]] })).await;
    assert_eq!(destroyed["destroyed"], json!([set.ids[&2]]), "{destroyed}");
    assert_eq!(
        set.written()[2..],
        [
            "SELECT \"[Gmail]/Trash\"",
            "UID STORE 1 +FLAGS.SILENT (\\Deleted)",
            "UID EXPUNGE 1",
        ]
    );
    let folders = set.rig.fake.script().mailboxes.folders();
    let trash = folders.iter().find(|folder| folder.name == TRASH).unwrap();
    assert!(trash.mail.is_empty(), "{:?}", trash.mail);
}

#[tokio::test]
async fn a_destroy_outside_spam_and_trash_is_forbidden_on_gmail() {
    let set = account().await;
    let answered = set.set(json!({ "destroy": [set.ids[&1]] })).await;
    let refusal = &answered["notDestroyed"][&set.ids[&1]]["type"];
    assert_eq!(refusal, "forbidden", "{answered}");
    let commands = set.written();
    assert!(commands.is_empty(), "{commands:?}");
}

#[tokio::test]
async fn not_spam_moves_into_all_mail_stores_the_inbox_and_reads_the_labels_back() {
    let set = account().await;
    let not_spam = patch(&set, &[(SPAM, Value::Null), ("INBOX", json!(true))]);
    let answered = set.update(&[(JUNKED, not_spam)]).await;
    let regained = &answered["updated"][&set.ids[&JUNKED]]["mailboxIds"];
    assert_eq!(
        regained.get(mailbox(&set, WORK)),
        Some(&json!(true)),
        "{answered}"
    );
    let lines = set.written();
    assert_eq!(
        lines[..4],
        [
            "SELECT \"[Gmail]/Spam\"",
            "UID MOVE 1 \"[Gmail]/All Mail\"",
            "SELECT \"[Gmail]/All Mail\"",
            "UID STORE 5 +X-GM-LABELS.SILENT (\\Inbox)",
        ]
    );
    let fetched = set.rig.fake.lines();
    let read_back = "UID FETCH 5:5 (UID FLAGS X-GM-LABELS)";
    assert!(
        fetched.iter().any(|line| line.ends_with(read_back)),
        "{fetched:?}"
    );
    assert_eq!(members(&set, JUNKED).await, ["INBOX", WORK, ALL_MAIL]);
    assert_eq!(labels(&set, ALL_MAIL, 5), [WORK, "\\Inbox"]);
    assert!(!expunges(&lines));
}

#[tokio::test]
async fn a_patch_gmail_cannot_hold_is_named_back_and_nothing_is_sent() {
    let set = account().await;
    let leaves = patch(&set, &[(ALL_MAIL, Value::Null)]);
    let labeled_trash = patch(&set, &[(TRASH, json!(true)), (WORK, json!(true))]);
    let answered = set.update(&[(1, leaves), (3, labeled_trash)]).await;
    assert_eq!(
        refused(&set, &answered, 1),
        "invalidProperties",
        "{answered}"
    );
    let named = &answered["notUpdated"][&set.ids[&3]]["properties"];
    assert_eq!(
        *named,
        json!([format!("mailboxIds/{}", mailbox(&set, WORK))])
    );
    let commands = set.written();
    assert!(commands.is_empty(), "{commands:?}");
}

#[tokio::test]
async fn without_move_gmail_refuses_a_store_move_and_a_label_still_lands_rfc6851_3_2() {
    let set = without_move().await;
    let trash = patch(&set, &[("INBOX", Value::Null), (TRASH, json!(true))]);
    let not_spam = patch(&set, &[(SPAM, Value::Null), ("INBOX", json!(true))]);
    let label = patch(&set, &[(WORK, json!(true))]);
    let answered = set
        .update(&[(2, trash), (JUNKED, not_spam), (1, label)])
        .await;
    assert_eq!(refused(&set, &answered, 2), "forbidden", "{answered}");
    assert_eq!(refused(&set, &answered, JUNKED), "forbidden");
    assert_eq!(answered["updated"], json!({ &set.ids[&1]: null }));
    assert_eq!(
        set.written(),
        [
            "SELECT \"[Gmail]/All Mail\"",
            "UID STORE 1 +X-GM-LABELS.SILENT (\"Work\")",
        ]
    );
    assert_eq!(members(&set, 2).await, ["INBOX", WORK, ALL_MAIL]);
    assert_eq!(members(&set, JUNKED).await, [SPAM]);
}

#[tokio::test]
async fn without_move_the_three_stores_show_no_move_right_and_a_label_mailbox_keeps_it() {
    let set = without_move().await;
    for store in [ALL_MAIL, SPAM, TRASH] {
        assert_eq!(moving(&set, store).await, (false, false), "{store}");
    }
    for label in ["INBOX", WORK] {
        assert_eq!(moving(&set, label).await, (true, true), "{label}");
    }
    let with_move = account().await;
    for store in [ALL_MAIL, SPAM, TRASH] {
        assert_eq!(moving(&with_move, store).await, (true, true), "{store}");
    }
}

#[tokio::test]
async fn a_message_another_client_moved_first_keeps_its_id_until_the_pass_finds_it() {
    let set = account().await;
    set.rig.fake.script().mailboxes.move_to((ALL_MAIL, 2), SPAM);
    let before = set.state();
    let trash = patch(&set, &[("INBOX", Value::Null), (TRASH, json!(true))]);
    let answered = set.update(&[(2, trash)]).await;
    assert_eq!(refused(&set, &answered, 2), "notFound", "{answered}");
    assert_eq!(set.written()[1..], ["UID MOVE 2 \"[Gmail]/Trash\""]);
    assert_eq!(set.state(), before, "a parked row is no state");
    let arguments = json!({ "accountId": ACCOUNT, "sinceState": before.to_string() });
    let changed = set
        .rig
        .call(&json!(["Email/changes", arguments, "c1"]))
        .await;
    assert_eq!(changed["updated"], json!([set.ids[&2]]), "{changed}");
    assert_eq!(changed["destroyed"], json!([]));
    assert_eq!(members(&set, 2).await, [SPAM]);
}
