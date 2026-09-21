// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The header sync against the scripted server: batches as states,
//! progress that survives a restart, counts, a renumbered folder and a
//! vanished one.

mod sync_observe;
mod sync_rig;

use huliho_imap_bridge::mailboxes::SyncError;
use huliho_imap_bridge::session::{Session, SessionError};
use huliho_imap_bridge::store::{ChangeKind, ObjectType};
use huliho_imap_bridge::sync::{SYNC_BATCH, Step};
use huliho_imap_bridge::testing::{Behavior, Extension, Folder, Mailboxes, Message};
use serde_json::json;
use sync_observe::{behaving, mail};
use sync_rig::{Rig, inbox};

/// Two full batches and a rest.
const LARGE: u32 = 1100;

/// Two full windows of the UID list and a rest.
const WINDOWED: u32 = 12_000;

#[tokio::test]
async fn every_batch_is_one_state_newest_first_with_progress_in_synced_emails() {
    let rig = Rig::start(inbox(mail(LARGE))).await;
    let (mut session, sync) = rig.open("INBOX").await;
    let mut sync = sync.unwrap();
    assert_eq!(
        sync.batch(&mut session, &rig.cache).await.unwrap(),
        Step::More
    );
    let mailbox = rig.mailbox("INBOX").await;
    assert_eq!(mailbox["syncedEmails"], SYNC_BATCH);
    assert_eq!(mailbox["totalEmails"], LARGE);
    assert_eq!(rig.cache.store.state(&rig.cache.key).unwrap(), 2);
    let logged = rig.changes(ObjectType::Email, 1);
    assert_eq!(logged.len(), SYNC_BATCH);
    assert_eq!(rig.changes(ObjectType::Thread, 1).len(), SYNC_BATCH);
    let folder = rig.folder("INBOX").id.to_string();
    assert_eq!(
        rig.changes(ObjectType::Mailbox, 1),
        [(folder, ChangeKind::Updated)]
    );
    assert_eq!(
        sync.finish(&mut session, &rig.cache).await.unwrap(),
        Step::Done
    );
    assert_eq!(rig.fetched(), ["601:1100", "101:600", "1:100"]);
    assert_eq!(rig.cache.store.state(&rig.cache.key).unwrap(), 4);
    assert_eq!(rig.mailbox("INBOX").await["syncedEmails"], LARGE);
    let (_, again) = rig.open("INBOX").await;
    assert!(again.is_none(), "a folder that is done opens no sync");
}

#[tokio::test]
async fn a_sync_cut_after_one_batch_resumes_below_the_last_uid_it_wrote() {
    let model = behaving(
        inbox(mail(LARGE)),
        Behavior {
            drops_after: Some(1),
            ..Behavior::default()
        },
    );
    let rig = Rig::start(model).await;
    for expected in [SYNC_BATCH, 2 * SYNC_BATCH] {
        let (mut session, sync) = rig.open("INBOX").await;
        let error = sync
            .unwrap()
            .finish(&mut session, &rig.cache)
            .await
            .unwrap_err();
        assert!(
            matches!(error, SyncError::Session(SessionError::Closed)),
            "{error}"
        );
        assert_eq!(rig.mailbox("INBOX").await["syncedEmails"], expected);
    }
    let (mut session, sync) = rig.open("INBOX").await;
    let step = sync.unwrap().finish(&mut session, &rig.cache).await;
    assert_eq!(step.unwrap(), Step::Done);
    assert_eq!(
        rig.fetched(),
        ["601:1100", "101:600", "101:600", "1:100", "1:100"]
    );
    assert_eq!(rig.created(0).len(), usize::try_from(LARGE).unwrap());
}

#[tokio::test]
async fn the_counts_follow_status_until_the_folder_is_done_and_the_memberships_after_rfc8621_2() {
    let messages = vec![
        Message::new(1),
        Message::new(2).flagged(&[]),
        Message::new(3).flagged(&["\\Draft"]),
        Message::new(4).flagged(&["\\Deleted"]),
        Message::new(5).flagged(&["\\Seen", "\\Deleted"]),
    ];
    let rig = Rig::start(inbox(messages)).await;
    let before = rig.mailbox("INBOX").await;
    assert_eq!(before["totalEmails"], 5);
    assert_eq!(before["unreadEmails"], 3);
    assert_eq!(before["syncedEmails"], 0);
    assert_eq!(rig.sync("INBOX").await, (Step::Done, 1));
    let after = rig.mailbox("INBOX").await;
    assert_eq!(after["syncedEmails"], 3, "a deleted message never arrives");
    assert_eq!(after["totalEmails"], 3);
    assert_eq!(after["unreadEmails"], 1, "an unseen draft is not unread");
    assert_eq!(after["totalThreads"], 3);
    assert_eq!(after["unreadThreads"], 1);
}

#[tokio::test]
async fn an_empty_folder_is_done_after_one_write() {
    let rig = Rig::start(inbox(Vec::new())).await;
    assert_eq!(rig.sync("INBOX").await, (Step::Done, 1));
    assert_eq!(rig.fetched(), Vec::<String>::new());
    assert_eq!(rig.cache.store.state(&rig.cache.key).unwrap(), 2);
    assert_eq!(rig.mailbox("INBOX").await["totalEmails"], 0);
}

/// The subject of each email with its id.
async fn subject_of(rig: &Rig, ids: &[String]) -> Vec<(String, String)> {
    rig.emails(ids, &["subject"])
        .await
        .iter()
        .map(|email| {
            (
                email["subject"].as_str().unwrap().to_owned(),
                email["id"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

/// The INBOX after the server renumbered it: message 2 gone, message 6
/// new, the rest under UIDs a hundred higher.
fn renumbered() -> Folder {
    let mut messages: Vec<Message> = [1, 3, 4, 5]
        .into_iter()
        .map(|uid| Message {
            uid: uid + 100,
            ..Message::new(uid)
        })
        .collect();
    messages.push(Message::new(106));
    Folder {
        uid_validity: 2,
        ..Folder::new("INBOX").with_mail(messages)
    }
}

#[tokio::test]
async fn a_renumbered_folder_keeps_the_ids_of_the_messages_it_still_holds_rfc3501_2_3_1_1() {
    let model = inbox(mail(5));
    let rig = Rig::start(model.clone()).await;
    rig.sync("INBOX").await;
    let before = subject_of(&rig, &rig.created(0)).await;
    model.set(vec![renumbered()]);
    let passed = rig.pass().await.unwrap();
    assert_eq!(
        rig.mailbox("INBOX").await["syncedEmails"],
        5,
        "the rows wait"
    );
    assert_eq!(rig.sync("INBOX").await, (Step::Done, 1));
    let changes = rig.changes(ObjectType::Email, passed);
    let kinds = |kind| changes.iter().filter(|(_, found)| *found == kind).count();
    assert_eq!(kinds(ChangeKind::Updated), 4);
    assert_eq!(kinds(ChangeKind::Created), 1);
    assert_eq!(kinds(ChangeKind::Destroyed), 1);
    let kept: Vec<String> = changes
        .iter()
        .filter(|(_, kind)| *kind == ChangeKind::Updated)
        .map(|(id, _)| id.clone())
        .collect();
    for pair in subject_of(&rig, &kept).await {
        assert!(before.contains(&pair), "{pair:?}");
    }
    let gone = &before
        .iter()
        .find(|(subject, _)| subject == "Message 2")
        .unwrap()
        .1;
    assert!(changes.contains(&(gone.clone(), ChangeKind::Destroyed)));
    assert_eq!(rig.mailbox("INBOX").await["totalEmails"], 5);
}

#[tokio::test]
async fn a_vanished_mailbox_takes_its_emails_and_threads_along_in_the_state_of_the_pass() {
    let model = Mailboxes::new(
        vec![
            Folder::new("INBOX").with_mail(mail(2)),
            // Ids of their own: a copy of an INBOX message would share
            // its thread, which then outlives the folder.
            Folder::new("Work").with_mail((11..=13).map(Message::new).collect()),
        ],
        Extension::all(),
    );
    let rig = Rig::start(model.clone()).await;
    rig.sync("INBOX").await;
    rig.sync("Work").await;
    let work = rig.folder("Work").id.to_string();
    let synced = rig.cache.store.state(&rig.cache.key).unwrap();
    model.remove("Work");
    assert_eq!(rig.pass().await.unwrap(), synced + 1);
    assert_eq!(
        rig.changes(ObjectType::Mailbox, synced),
        [(work, ChangeKind::Destroyed)]
    );
    for object in [ObjectType::Email, ObjectType::Thread] {
        let changes = rig.changes(object, synced);
        assert_eq!(changes.len(), 3);
        assert!(
            changes
                .iter()
                .all(|(_, kind)| *kind == ChangeKind::Destroyed)
        );
    }
    let left = rig
        .call(&json!([
            "Email/get",
            { "accountId": sync_rig::ACCOUNT, "ids": rig.created(0), "properties": ["id"] },
            "c1"
        ]))
        .await;
    assert_eq!(left["list"].as_array().unwrap().len(), 2);
    assert_eq!(left["notFound"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn the_uid_list_comes_in_windows_and_an_expunge_in_between_hides_no_message_rfc3501_7_4_1() {
    let leaving = behaving(
        inbox(mail(WINDOWED)),
        Behavior {
            expunges: 3,
            ..Behavior::default()
        },
    );
    let rig = Rig::start(leaving).await;
    let mut session = rig.session().await;
    session.examine("INBOX").await.unwrap();
    let uids = session.uid_list().await.unwrap();
    assert_eq!(windows(&rig), ["7001:12000", "2001:7000", "1:2000"]);
    // The three lowest left behind the first answer; what moved down by
    // three shows up in two windows and is kept once.
    let expected: Vec<u32> = (4..=WINDOWED).rev().collect();
    assert_eq!(uids, expected);
}

#[tokio::test]
async fn a_second_list_names_no_number_above_the_count_the_expunges_left_rfc3501_9() {
    let leaving = behaving(
        inbox(mail(10)),
        Behavior {
            expunges: 3,
            ..Behavior::default()
        },
    );
    let rig = Rig::start(leaving).await;
    let mut session = rig.session().await;
    session.examine("INBOX").await.unwrap();
    assert_eq!(session.uid_list().await.unwrap().len(), 10);
    let second = session.uid_list().await.unwrap();
    // The server answers BAD to a number above its count.
    assert_eq!(windows(&rig), ["1:10", "1:7"]);
    assert_eq!(second, (4..=10).rev().collect::<Vec<u32>>());
}

/// The windows of UID SEARCH the server received, in order.
fn windows(rig: &Rig) -> Vec<String> {
    rig.fake
        .lines()
        .iter()
        .filter_map(|line| {
            line.split_once("UID SEARCH ")
                .map(|(_, window)| window.to_owned())
        })
        .collect()
}
