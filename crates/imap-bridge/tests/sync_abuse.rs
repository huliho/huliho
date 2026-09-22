// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The header sync against a server and senders that mean harm: one
//! message must never stall a folder and no answer may grow the cache
//! past its bounds.

mod sync_observe;
mod sync_rig;

use huliho_imap_bridge::mailboxes::SyncError;
use huliho_imap_bridge::session::{
    FetchItems, MAX_NESTING, MAX_RESPONSE_BYTES, MAX_STRUCTURED_BYTES, Session, SessionError,
    UidRange,
};
use huliho_imap_bridge::store::ObjectType;
use huliho_imap_bridge::sync::{FolderSync, Step};
use huliho_imap_bridge::testing::{Behavior, Extension, Folder, Mailboxes, Message};
use sync_observe::{behaving, mail};
use sync_rig::{Rig, inbox};

/// The UID of the message a sender built to hurt.
const HOSTILE: u32 = 17;

/// The header fetch with the structure, as the first sync asks it.
const STRUCTURED: FetchItems = FetchItems {
    structure: true,
    gmail: false,
};

#[tokio::test]
async fn one_message_past_a_bound_of_the_guard_costs_its_attachment_mark_and_nothing_else() {
    let attached = Message::new(HOSTILE).with_attachment();
    for hostile in [
        attached.clone().nested(MAX_NESTING + 8),
        attached.sprawling(MAX_STRUCTURED_BYTES),
    ] {
        let mut messages: Vec<Message> = (1..=40)
            .map(|uid| Message::new(uid).with_attachment())
            .collect();
        messages[16] = hostile;
        let rig = Rig::start(inbox(messages)).await;
        let (step, sessions) = rig.sync("INBOX").await;
        assert_eq!(step, Step::Done);
        // The upper half goes first, so the progress stays one line from the top.
        assert_eq!(
            rig.fetched(),
            [
                "1:40", "21:40", "1:20", "11:20", "16:20", "18:20", "16:17", "17:17", "17:17",
                "16:16", "11:15", "1:10"
            ]
        );
        // Five slices and the lone message fail, each on a session of its own.
        assert_eq!(sessions, 7);
        let mailbox = rig.mailbox("INBOX").await;
        assert_eq!(mailbox["syncedEmails"], 40);
        assert_eq!(mailbox["totalEmails"], 40);
        let emails = rig
            .emails(&rig.created(0), &["subject", "hasAttachment"])
            .await;
        assert_eq!(emails.len(), 40);
        for email in emails {
            let hostile = email["subject"] == format!("Message {HOSTILE}");
            assert_eq!(email["hasAttachment"], !hostile, "{email}");
        }
    }
}

#[tokio::test]
async fn a_message_the_server_cannot_describe_at_all_is_left_out_and_the_folder_finishes() {
    let no_such_day = Message {
        internal_date: "31-Feb-2026 00:00:00 +0000".to_owned(),
        ..Message::new(5)
    };
    let header_past_the_byte_bound = Message {
        header: format!("Subject: {}\r\n\r\n", "x".repeat(MAX_RESPONSE_BYTES)),
        ..Message::new(5)
    };
    for hostile in [no_such_day, header_past_the_byte_bound] {
        let mut messages = mail(9);
        messages[4] = hostile;
        let rig = Rig::start(inbox(messages)).await;
        let (step, _) = rig.sync("INBOX").await;
        assert_eq!(step, Step::Done);
        let mailbox = rig.mailbox("INBOX").await;
        assert_eq!(mailbox["syncedEmails"], 8);
        assert_eq!(mailbox["totalEmails"], 8);
        let subjects: Vec<String> = rig
            .emails(&rig.created(0), &["subject"])
            .await
            .iter()
            .map(|email| email["subject"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(subjects.len(), 8);
        assert!(!subjects.contains(&"Message 5".to_owned()), "{subjects:?}");
    }
}

#[tokio::test]
async fn lines_nobody_asked_for_change_nothing_and_too_many_fail_the_answer_rfc3501_7() {
    let noisy = behaving(
        inbox(mail(3)),
        Behavior {
            volunteered: 1000,
            ..Behavior::default()
        },
    );
    let rig = Rig::start(noisy).await;
    assert_eq!(rig.sync("INBOX").await, (Step::Done, 1));
    let emails = rig.emails(&rig.created(0), &["subject", "keywords"]).await;
    assert_eq!(emails.len(), 3, "a message outside the range stays out");
    for email in emails {
        assert_eq!(email["keywords"], serde_json::json!({ "$seen": true }));
    }
    let flood = behaving(
        inbox(mail(3)),
        Behavior {
            volunteered: 2000,
            ..Behavior::default()
        },
    );
    let rig = Rig::start(flood).await;
    let mut session = rig.session().await;
    session.examine("INBOX").await.unwrap();
    let error = session
        .uid_fetch(UidRange { low: 1, high: 3 }, STRUCTURED)
        .await
        .unwrap_err();
    assert!(
        matches!(
            error,
            SessionError::Protocol("the answer passes the line limit")
        ),
        "{error}"
    );
}

#[tokio::test]
async fn a_mod_sequence_past_63_bits_is_a_protocol_failure_on_every_path_rfc7162_3_1() {
    let fetch = behaving(
        inbox(mail(2)),
        Behavior {
            fetch_modseq: Some(u64::MAX),
            ..Behavior::default()
        },
    );
    let rig = Rig::start(fetch).await;
    let mut session = rig.session().await;
    session.examine("INBOX").await.unwrap();
    let error = session
        .uid_fetch(UidRange { low: 1, high: 2 }, STRUCTURED)
        .await
        .unwrap_err();
    assert!(
        matches!(
            error,
            SessionError::Protocol("a mod-sequence passes 63 bits")
        ),
        "{error}"
    );
    let status = Mailboxes::new(
        vec![Folder {
            highest_modseq: u64::MAX,
            ..Folder::new("INBOX")
        }],
        Extension::all(),
    );
    let rig = Rig::over(status).await;
    let error = rig.pass().await.unwrap_err();
    assert!(
        matches!(
            error,
            SyncError::Session(SessionError::Protocol("a mod-sequence passes 63 bits"))
        ),
        "{error}"
    );
    assert_eq!(rig.cache.store.state(&rig.cache.key).unwrap(), 0);
    let error = rig.session().await.examine("INBOX").await.unwrap_err();
    assert!(
        matches!(
            error,
            SessionError::Protocol("a mod-sequence passes 63 bits")
        ),
        "{error}"
    );
}

#[tokio::test]
async fn a_folder_that_vanished_under_the_sync_is_skipped_or_fails_in_fixed_words() {
    let model = Mailboxes::new(
        vec![
            Folder::new("INBOX").with_mail(mail(2)),
            Folder::new("Work").with_mail(mail(2)),
        ],
        Extension::all(),
    );
    let rig = Rig::start(model.clone()).await;
    let work = rig.folder("Work");
    model.remove("Work");
    let mut session = rig.session().await;
    let refused = FolderSync::open(&mut session, &rig.cache, &work).await;
    assert!(refused.unwrap().is_none(), "a plain NO skips the folder");
    // Dovecot's NO names the mailbox in raw UTF-8, which the parser refuses.
    let raw = behaving(
        Mailboxes::new(
            vec![Folder::new("INBOX"), Folder::new("Work")],
            Extension::all(),
        ),
        Behavior {
            utf8_no: true,
            ..Behavior::default()
        },
    );
    let rig = Rig::start(raw.clone()).await;
    let work = rig.folder("Work");
    raw.remove("Work");
    let mut session = rig.session().await;
    let Err(error) = FolderSync::open(&mut session, &rig.cache, &work).await else {
        panic!("the parse failure must surface");
    };
    assert!(
        matches!(
            error,
            SyncError::Session(SessionError::Protocol("the answer could not be parsed"))
        ),
        "{error}"
    );
    assert_eq!(rig.changes(ObjectType::Email, 0), []);
}

#[tokio::test]
async fn an_examine_that_fails_or_counts_nothing_leaves_no_mailbox_to_list_rfc3501_6_3_1() {
    let rig = Rig::start(inbox(mail(3))).await;
    let mut session = rig.session().await;
    session.examine("INBOX").await.unwrap();
    let refused = session.examine("Gone").await;
    assert!(matches!(refused, Err(SessionError::Refused)), "{refused:?}");
    let error = session.uid_list().await.unwrap_err();
    assert!(
        matches!(error, SessionError::Protocol("no mailbox is selected")),
        "{error}"
    );
    let silent = behaving(
        inbox(mail(3)),
        Behavior {
            no_exists: true,
            ..Behavior::default()
        },
    );
    let rig = Rig::start(silent).await;
    let error = rig.session().await.examine("INBOX").await.unwrap_err();
    assert!(
        matches!(
            error,
            SessionError::Protocol("EXAMINE answered without EXISTS")
        ),
        "{error}"
    );
}

#[tokio::test]
async fn a_range_that_runs_backward_is_refused_before_anything_is_sent() {
    let rig = Rig::start(inbox(mail(3))).await;
    let mut session = rig.session().await;
    session.examine("INBOX").await.unwrap();
    let error = session
        .uid_fetch(UidRange { low: 3, high: 1 }, STRUCTURED)
        .await
        .unwrap_err();
    assert!(
        matches!(error, SessionError::Protocol("the UID range runs backward")),
        "{error}"
    );
    assert_eq!(rig.fetched(), Vec::<String>::new());
}

#[tokio::test]
async fn a_batch_whose_account_is_gone_writes_nothing() {
    let rig = Rig::start(inbox(mail(3))).await;
    let (mut session, sync) = rig.open("INBOX").await;
    let mut sync = sync.unwrap();
    let before = rig.cache.store.state(&rig.cache.key).unwrap();
    rig.cache
        .store
        .apply_mailboxes(&rig.cache.key, &[])
        .unwrap();
    let after_removal = rig.cache.store.state(&rig.cache.key).unwrap();
    assert_eq!(after_removal, before + 1);
    assert_eq!(
        sync.batch(&mut session, &rig.cache).await.unwrap(),
        Step::Stale
    );
    assert_eq!(
        rig.cache.store.state(&rig.cache.key).unwrap(),
        after_removal
    );
    assert_eq!(rig.changes(ObjectType::Email, 0), []);
}
