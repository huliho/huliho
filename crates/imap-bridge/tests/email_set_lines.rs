// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What an `Email/set` puts on a command line: flags or labels past one
//! line go out in STOREs that each fit, a label as long as a name may be
//! is never sent and All Mail opened `[READ-ONLY]` or under another
//! UIDVALIDITY for the labels of a move fails the update with the row
//! moved and the session kept.

mod mailbox_ids;
mod set_rig;
mod sync_rig;

use huliho_imap_bridge::session::{MAX_COMMAND_BYTES, MAX_WIRE_NAME_BYTES};
use huliho_imap_bridge::store::ObjectType;
use huliho_imap_bridge::testing::mailboxes::{ALL_MAIL, SPAM};
use huliho_imap_bridge::testing::{Behavior, Mailboxes, Message};
use mailbox_ids::{mailbox, members, patch};
use serde_json::{Value, json};
use set_rig::{Set, refused, started};
use sync_rig::inbox;

const INBOX: &str = "INBOX";

/// The longest flag the sync keeps as a keyword.
const LONGEST_KEYWORD: usize = 255;

/// Keywords enough that clearing them passes one command line.
const MANY_KEYWORDS: usize = 20;

/// Half the longest name a server may list: two such labels pass what
/// one STORE carries.
const LONG: usize = MAX_WIRE_NAME_BYTES / 2;

/// The user label of the account.
const WORK: &str = "Work";

/// The message that waits in Spam, under the user label.
const JUNKED: u32 = 2;

/// On the write's connection the SELECT of All Mail follows the one of
/// Spam.
const SECOND_SELECT: usize = 1;

/// One message in the inbox beside these user labels, synced in All
/// Mail.
async fn labeled(labels: &[String]) -> Set {
    let mail = vec![Message::new(1).labeled(&["\\Inbox"])];
    let mut mailboxes = Mailboxes::gmail(mail);
    for label in labels {
        mailboxes = mailboxes.with_label(label);
    }
    started(mailboxes, ALL_MAIL, true).await
}

/// Two messages in the inbox with the second moved to Spam by another
/// client before the sync, synced in All Mail and in Spam, on a server
/// that behaves this way.
async fn with_spam(behavior: Behavior) -> Set {
    let mail = vec![
        Message::new(1).labeled(&["\\Inbox"]),
        Message::new(JUNKED).labeled(&["\\Inbox", WORK]),
    ];
    let mut mailboxes = Mailboxes::gmail(mail).with_label(WORK);
    mailboxes.behavior = behavior;
    mailboxes.move_to((ALL_MAIL, JUNKED), SPAM);
    let mut set = Box::pin(started(mailboxes, ALL_MAIL, true)).await;
    let before = set.state();
    Box::pin(set.rig.sync(SPAM)).await;
    let created = set.rig.created(before);
    assert_eq!(created.len(), 1, "{created:?}");
    set.ids.insert(JUNKED, created[0].clone());
    set
}

/// The labels the server holds for the message of that UID in All Mail.
fn labels(set: &Set, uid: u32) -> Vec<String> {
    let folders = set.rig.fake.script().mailboxes.folders();
    let folder = folders.iter().find(|found| found.name == ALL_MAIL).unwrap();
    let message = folder.mail.iter().find(|found| found.uid == uid).unwrap();
    message.labels.clone()
}

/// The sign-ins the server saw.
fn logins(set: &Set) -> usize {
    let lines = set.rig.fake.lines();
    lines.iter().filter(|line| line.contains(" LOGIN ")).count()
}

/// The STOREs of that item and sign the server received.
fn stores(set: &Set, item: &str) -> Vec<String> {
    set.written()
        .into_iter()
        .filter(|line| line.contains(item))
        .collect()
}

/// Every command fits one line and every name is on exactly one of them.
fn each_fits_and_carries_once(commands: &[String], names: &[String]) {
    for command in commands {
        assert!(command.len() <= MAX_COMMAND_BYTES, "{}", command.len());
    }
    for name in names {
        let carrying = commands
            .iter()
            .filter(|command| command.contains(name))
            .count();
        assert_eq!(carrying, 1, "{name:.8}");
    }
}

#[tokio::test]
async fn keywords_past_one_command_line_go_out_in_stores_that_each_fit_rfc7162_4() {
    let held: Vec<String> = (0..MANY_KEYWORDS)
        .map(|n| format!("{n:0LONGEST_KEYWORD$}"))
        .collect();
    let flags: Vec<&str> = held.iter().map(String::as_str).collect();
    let set = started(inbox(vec![Message::new(1).flagged(&flags)]), INBOX, false).await;
    let kept = set.keywords(1).await;
    assert_eq!(kept.as_object().unwrap().len(), MANY_KEYWORDS, "{kept}");
    let answered = set.update(&[(1, json!({ "keywords": {} }))]).await;
    assert_eq!(
        answered["updated"],
        json!({ &set.ids[&1]: null }),
        "{answered}"
    );
    let cleared = stores(&set, "-FLAGS.SILENT");
    assert_eq!(cleared.len(), 2, "{cleared:?}");
    each_fits_and_carries_once(&cleared, &held);
    let flags = set.flags(INBOX, 1);
    assert!(flags.is_empty(), "{flags:?}");
    assert_eq!(set.keywords(1).await, json!({}));
}

#[tokio::test]
async fn labels_past_one_command_line_go_out_in_stores_that_each_fit_rfc7162_4() {
    let long: Vec<String> = ["a", "b"]
        .iter()
        .map(|letter| letter.repeat(LONG))
        .collect();
    let set = labeled(&long).await;
    let before = set.state();
    let both = patch(&set, &[(&long[0], json!(true)), (&long[1], json!(true))]);
    let answered = set.update(&[(1, both)]).await;
    assert_eq!(
        answered["updated"],
        json!({ &set.ids[&1]: null }),
        "{answered}"
    );
    let added = stores(&set, "+X-GM-LABELS.SILENT");
    assert_eq!(added.len(), 2, "{added:?}");
    let quoted: Vec<String> = long.iter().map(|label| format!("\"{label}\"")).collect();
    each_fits_and_carries_once(&added, &quoted);
    let held = labels(&set, 1);
    assert!(
        long.iter().all(|label| held.contains(label)),
        "{}",
        held.len()
    );
    let held = members(&set, 1).await;
    assert!(
        long.iter().all(|label| held.contains(label)),
        "{}",
        held.len()
    );
    let mut gained = [mailbox(&set, &long[0]), mailbox(&set, &long[1])];
    gained.sort();
    assert_eq!(set.updated(ObjectType::Mailbox, before), gained);
    let signed = logins(&set);
    let archive = patch(&set, &[(INBOX, Value::Null)]);
    let answered = set.update(&[(1, archive)]).await;
    assert_eq!(
        answered["updated"],
        json!({ &set.ids[&1]: null }),
        "{answered}"
    );
    assert_eq!(logins(&set), signed, "both STOREs went on one session");
}

#[tokio::test]
async fn a_label_as_long_as_a_name_may_be_is_never_sent_and_its_update_fails() {
    let longest = "l".repeat(MAX_WIRE_NAME_BYTES);
    let set = labeled(&[longest.clone(), WORK.to_owned()]).await;
    let signed = logins(&set);
    let widest = patch(&set, &[(&longest, json!(true))]);
    let answered = set.update(&[(1, widest)]).await;
    assert_eq!(refused(&set, &answered, 1), "serverFail", "{answered}");
    let commands = set.written();
    assert!(commands.is_empty(), "{commands:?}");
    assert_eq!(logins(&set), signed, "nothing was asked of the server");
    assert_eq!(labels(&set, 1), ["\\Inbox"]);
    let fitting = patch(&set, &[(WORK, json!(true))]);
    let answered = set.update(&[(1, fitting)]).await;
    assert_eq!(
        answered["updated"],
        json!({ &set.ids[&1]: null }),
        "{answered}"
    );
    assert_eq!(
        stores(&set, "X-GM-LABELS.SILENT"),
        ["UID STORE 1 +X-GM-LABELS.SILENT (\"Work\")"]
    );
}

/// Not spam on a server that behaves this way on the SELECT of All
/// Mail: the update fails with `serverFail`, nothing goes to All Mail
/// past that SELECT, the row lies in All Mail without its labels and a
/// second write runs on the same session.
async fn restore_fails_and_the_session_stands(behavior: Behavior) {
    let set = with_spam(behavior).await;
    let not_spam = patch(&set, &[(SPAM, Value::Null), (INBOX, json!(true))]);
    let answered = set.update(&[(JUNKED, not_spam)]).await;
    assert_eq!(refused(&set, &answered, JUNKED), "serverFail", "{answered}");
    assert_eq!(
        set.written(),
        [
            "SELECT \"[Gmail]/Spam\"",
            "UID MOVE 1 \"[Gmail]/All Mail\"",
            "SELECT \"[Gmail]/All Mail\"",
        ]
    );
    assert_eq!(members(&set, JUNKED).await, [ALL_MAIL]);
    let signed = logins(&set);
    let archive = patch(&set, &[(INBOX, Value::Null)]);
    let answered = set.update(&[(1, archive)]).await;
    assert_eq!(
        answered["updated"],
        json!({ &set.ids[&1]: null }),
        "{answered}"
    );
    assert_eq!(logins(&set), signed, "the target keeps the session");
}

#[tokio::test]
async fn all_mail_read_only_after_a_move_fails_the_restore_and_keeps_the_session_rfc3501_6_3_1() {
    let closing = Behavior {
        read_only_at: Some(SECOND_SELECT),
        ..Behavior::default()
    };
    restore_fails_and_the_session_stands(closing).await;
}

#[tokio::test]
async fn all_mail_renumbered_after_a_move_fails_the_restore_and_keeps_the_session_rfc3501_2_3_1_1()
{
    let renumbering = Behavior {
        renumbered_at: Some(SECOND_SELECT),
        ..Behavior::default()
    };
    restore_fails_and_the_session_stands(renumbering).await;
}
