// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A Dovecot-shaped account with mail about a plan in four folders and a
//! Gmail account over the same mail, both synced; the searches a test
//! sends, each answer read back as UIDs, which the size of a scripted
//! message names; every command the server received.

use std::time::Duration;

use huliho_imap_bridge::runtime::{Link, Timing};
use huliho_imap_bridge::session::TlsMode;
use huliho_imap_bridge::testing::imap::HOST;
use huliho_imap_bridge::testing::mailboxes::{SPAM, TRASH};
use huliho_imap_bridge::testing::messages::SIZE_ABOVE_UID;
use huliho_imap_bridge::testing::{Behavior, Mailboxes, Message, TestConnector};
use serde_json::{Map, Value, json};

use crate::sync_rig::{ACCOUNT, Rig};

/// Room for a loopback exchange.
const STEP: Duration = Duration::from_secs(2);

/// A message from `sender` with this subject and body.
pub fn message(uid: u32, sender: &str, subject: &str, body: &str) -> Message {
    Message {
        header: format!(
            "From: {sender}\r\nTo: mo@example.test\r\nSubject: {subject}\r\nMessage-ID: <m{uid}@example.test>\r\n\r\n"
        ),
        body: body.to_owned(),
        ..Message::new(uid)
    }
}

/// The same message as a reply in the thread of message 1.
fn in_plan_thread(message: Message) -> Message {
    let header = message.header.replace(
        "\r\n\r\n",
        "\r\nIn-Reply-To: <m1@example.test>\r\nReferences: <m1@example.test>\r\n\r\n",
    );
    Message { header, ..message }.in_thread(Message::new(1).thrid)
}

/// The inbox: the plan from Anna; a lunch note from Bob two days on,
/// written in a zone behind UTC; an unseen reply with an attachment.
pub fn inbox_mail() -> Vec<Message> {
    vec![
        message(
            1,
            "Anna Jansen <anna@example.test>",
            "Quarterly plan",
            "Zoë sends the plan.",
        ),
        Message {
            internal_date: "03-Jan-2026 23:30:00 -0500".to_owned(),
            ..message(2, "Bob <bob@example.test>", "Lunch", "Tomorrow?")
        },
        in_plan_thread(message(
            3,
            "anna@example.test",
            "Re: Quarterly plan",
            "Thanks, Zoë!",
        ))
        .with_attachment()
        .flagged(&[]),
    ]
}

/// What the other folders hold: an older answer to the plan in Archive,
/// written in a zone ahead of UTC; an offer in Junk; a plan in Trash.
fn archive_mail() -> Vec<Message> {
    vec![Message {
        internal_date: "05-Jan-2026 00:30:00 +0200".to_owned(),
        ..in_plan_thread(message(
            11,
            "anna@example.test",
            "Plan archive",
            "the old plan",
        ))
    }]
}

fn junk_mail() -> Message {
    message(21, "deals@example.test", "Plan offer", "a cheap plan")
}

fn trash_mail() -> Message {
    message(31, "anna@example.test", "Old plan", "the plan, deleted")
}

/// A Dovecot with the plan's mail in INBOX, Archive, Junk and Trash.
pub fn dovecot() -> Mailboxes {
    let mailboxes = Mailboxes::dovecot();
    let mut folders = mailboxes.folders();
    for folder in &mut folders {
        let mail = match folder.name.as_str() {
            "INBOX" => inbox_mail(),
            "Archive" => archive_mail(),
            "Junk" => vec![junk_mail()],
            "Trash" => vec![trash_mail()],
            _ => continue,
        };
        *folder = folder.clone().with_mail(mail);
    }
    mailboxes.set(folders);
    mailboxes
}

/// A Gmail account over the same mail: the inbox and the archive in All
/// Mail with the first message starred; the offer in Spam; the plan in
/// Trash.
pub fn gmail() -> Mailboxes {
    let mut mail = inbox_mail();
    mail[0] = mail[0].clone().labeled(&["\\Inbox", "\\Starred"]);
    mail.extend(
        archive_mail()
            .into_iter()
            .map(|message| message.labeled(&[])),
    );
    let mailboxes = Mailboxes::gmail(mail);
    let mut folders = mailboxes.folders();
    for folder in &mut folders {
        let held = match folder.name.as_str() {
            SPAM => junk_mail(),
            TRASH => trash_mail(),
            _ => continue,
        };
        *folder = folder.clone().with_mail(vec![held.labeled(&[])]);
    }
    mailboxes.set(folders);
    mailboxes
}

/// The account synced, its searches on these clocks.
pub async fn synced(mailboxes: Mailboxes, gmail: bool, timing: Timing) -> Rig {
    let mut rig = if gmail {
        let mut rig = Rig::over(mailboxes).await;
        rig.cache.gmail = true;
        rig.pass().await.unwrap();
        rig
    } else {
        Rig::start(mailboxes).await
    };
    let stores: Vec<String> = rig
        .cache
        .store
        .mailbox_snapshot(&rig.cache.key)
        .unwrap()
        .rows
        .into_iter()
        .filter(|row| row.facts.store && row.facts.selectable)
        .map(|row| row.facts.imap_name)
        .collect();
    for store in stores {
        rig.sync(&store).await;
    }
    let fake = &rig.fake;
    let connector =
        TestConnector::scripted(fake.trusting(), fake.target(HOST, TlsMode::Implicit), STEP);
    rig.link = Link::with_timing(connector, timing);
    rig
}

/// The clocks a test runs on: no wait between refreshes.
pub fn quick() -> Timing {
    Timing {
        interval: Duration::ZERO,
        ..Timing::default()
    }
}

/// The same mailboxes whose server misbehaves so.
pub fn behaving(mut mailboxes: Mailboxes, behavior: Behavior) -> Mailboxes {
    mailboxes.behavior = behavior;
    mailboxes
}

/// The id of the mailbox with that wire name.
pub fn id_of(rig: &Rig, imap_name: &str) -> String {
    rig.folder(imap_name).id.to_string()
}

/// The ids of the mailboxes a default scope leaves out: Junk and Trash,
/// by role.
pub fn bins(rig: &Rig) -> Vec<String> {
    rig.cache
        .store
        .mailbox_snapshot(&rig.cache.key)
        .unwrap()
        .rows
        .into_iter()
        .filter(|row| matches!(row.facts.role.as_deref(), Some("junk" | "trash")))
        .map(|row| row.id.to_string())
        .collect()
}

/// `Email/query` with this filter and these arguments on top, the total
/// asked.
pub async fn query(rig: &Rig, filter: Value, extra: Value) -> Value {
    let mut arguments = json!({ "accountId": ACCOUNT, "filter": filter, "calculateTotal": true });
    let extra: Map<String, Value> = serde_json::from_value(extra).unwrap();
    arguments.as_object_mut().unwrap().extend(extra);
    rig.call(&json!(["Email/query", arguments, "q"])).await
}

/// The UIDs the ids of an answer name, in its order.
pub async fn uids(rig: &Rig, answer: &Value) -> Vec<u64> {
    let got = json!({ "accountId": ACCOUNT, "ids": answer["ids"], "properties": ["size"] });
    let listed = rig.call(&json!(["Email/get", got, "g"])).await;
    let emails = listed["list"]
        .as_array()
        .unwrap_or_else(|| panic!("{answer}"));
    emails
        .iter()
        .map(|email| email["size"].as_u64().unwrap() - u64::from(SIZE_ABOVE_UID))
        .collect()
}

/// The UIDs a search with this filter answers, newest first.
pub async fn found(rig: &Rig, filter: Value) -> Vec<u64> {
    let answer = query(rig, filter, json!({})).await;
    uids(rig, &answer).await
}

/// Where the record of received lines stands, for the commands after.
pub fn mark(rig: &Rig) -> usize {
    rig.fake.lines().len()
}

/// Every command the server received after the mark that opens with
/// `verb`, without its tag.
pub fn commands(rig: &Rig, from: usize, verb: &str) -> Vec<String> {
    rig.fake
        .lines()
        .iter()
        .skip(from)
        .filter_map(|line| line.split_once(' ')?.1.split_once(' '))
        .map(|(_, command)| command.to_owned())
        .filter(|command| command.starts_with(verb))
        .collect()
}

/// The folders the server was asked to EXAMINE after the mark, in order.
pub fn examined(rig: &Rig, from: usize) -> Vec<String> {
    commands(rig, from, "EXAMINE ")
        .into_iter()
        .map(|command| command["EXAMINE ".len()..].trim_matches('"').to_owned())
        .collect()
}
