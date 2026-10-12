// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The session's moves, copies and expunges against the scripted
//! server in its folder mode and its Gmail mode: the new UIDs from
//! `COPYUID`, the count that follows the expunges, a message another
//! client marked left alone, a read-only folder that takes no write and
//! a `COPYUID` that does not pair with the ask.

mod session_rig;

use huliho_imap_bridge::session::{
    ImapSession, MoveAsk, Moved, Session, SessionError, StoreAsk, StoreItem,
};
use huliho_imap_bridge::testing::imap::FakeImap;
use huliho_imap_bridge::testing::mailboxes::{ALL_MAIL, TRASH};
use huliho_imap_bridge::testing::{Behavior, Extension, Folder, Mailboxes, Message, Storing};
use session_rig::{session, start};

const INBOX: &str = "INBOX";
const ARCHIVE: &str = "Archive";

/// The UIDVALIDITY of the archive, apart from the inbox's.
const ARCHIVE_VALIDITY: u32 = 42;

/// The words of a write on a mailbox no SELECT opened for writing.
const NOT_WRITABLE: &str = "no mailbox is open for writing";

/// A Dovecot-shaped server: an inbox of `messages` messages and an empty
/// archive, MOVE and UIDPLUS among the extensions.
fn dovecot(messages: u32) -> Mailboxes {
    let mailboxes = Mailboxes::dovecot();
    mailboxes.set(vec![
        Folder::new(INBOX).with_mail((1..=messages).map(Message::new).collect()),
        Folder {
            uid_validity: ARCHIVE_VALIDITY,
            ..Folder::special(ARCHIVE, "\\Archive")
        },
    ]);
    mailboxes
}

/// A server over the model and a session with the folder selected for
/// writing.
async fn selected(mailboxes: Mailboxes, folder: &str) -> (FakeImap, ImapSession) {
    let fake = start(mailboxes).await;
    let mut session = session(&fake).await;
    let writable = session.select(folder).await.unwrap();
    assert!(!writable.read_only);
    (fake, session)
}

fn to<'a>(uids: &'a [u32], target: &'a str) -> MoveAsk<'a> {
    MoveAsk { uids, target }
}

/// The UIDs a folder of the server holds, in order.
fn uids_in(fake: &FakeImap, folder: &str) -> Vec<u32> {
    let folders = fake.script().mailboxes.folders();
    let found = folders.iter().find(|found| found.name == folder).unwrap();
    found.mail.iter().map(|message| message.uid).collect()
}

/// Every write command the server received, without its tag.
fn written(fake: &FakeImap) -> Vec<String> {
    let verbs = ["SELECT", "UID MOVE", "UID COPY", "UID STORE", "UID EXPUNGE"];
    fake.lines()
        .iter()
        .filter_map(|line| line.split_once(' ')?.1.split_once(' '))
        .map(|(_, command)| command.to_owned())
        .filter(|command| verbs.iter().any(|verb| command.starts_with(verb)))
        .collect()
}

#[tokio::test]
async fn a_move_names_every_new_uid_and_the_count_follows_its_expunges_rfc6851_3_2() {
    let (fake, mut session) = selected(dovecot(5), INBOX).await;
    let moved = session.uid_move(&to(&[4, 2], ARCHIVE)).await.unwrap();
    let expected = Moved {
        uid_validity: ARCHIVE_VALIDITY,
        uids: vec![(2, 1), (4, 2)],
    };
    assert_eq!(moved, Some(expected));
    assert_eq!(uids_in(&fake, INBOX), [1, 3, 5]);
    assert_eq!(uids_in(&fake, ARCHIVE), [1, 2]);
    // A count left at five would ask for sequence numbers past the
    // server's count, which it answers with BAD.
    assert_eq!(session.uid_list().await.unwrap(), [5, 3, 1]);
    assert_eq!(
        written(&fake),
        ["SELECT \"INBOX\"", "UID MOVE 2,4 \"Archive\""]
    );
}

#[tokio::test]
async fn a_uid_that_names_no_message_has_no_pair_rfc3501_6_4_8() {
    let (_fake, mut session) = selected(dovecot(3), INBOX).await;
    let moved = session.uid_move(&to(&[2, 99], ARCHIVE)).await.unwrap();
    assert_eq!(moved.unwrap().uids, [(2, 1)]);
}

#[tokio::test]
async fn a_copy_then_an_expunge_leaves_a_message_another_client_marked_rfc4315_2_1() {
    let mut mailboxes = dovecot(5);
    mailboxes.extensions.remove(&Extension::Move);
    mailboxes.store_flags(INBOX, 3, &["\\Seen", "\\Deleted"]);
    let (fake, mut session) = selected(mailboxes, INBOX).await;
    let moved = session.uid_copy(&to(&[2], ARCHIVE)).await.unwrap();
    assert_eq!(moved.unwrap().uids, [(2, 1)]);
    session.uid_expunge(&[2]).await.unwrap();
    assert_eq!(uids_in(&fake, INBOX), [1, 3, 4, 5]);
    assert_eq!(uids_in(&fake, ARCHIVE), [1]);
    assert_eq!(session.uid_list().await.unwrap(), [5, 4, 3, 1]);
    // MOVE is not advertised there, and the server says so.
    let refused = session.uid_move(&to(&[1], ARCHIVE)).await;
    assert!(matches!(refused, Err(SessionError::Protocol(_))));
    assert_eq!(
        written(&fake),
        [
            "SELECT \"INBOX\"",
            "UID COPY 2 \"Archive\"",
            "UID STORE 2 +FLAGS.SILENT (\\Deleted)",
            "UID EXPUNGE 2",
            "UID MOVE 1 \"Archive\"",
        ]
    );
}

#[tokio::test]
async fn an_expunge_whose_store_fails_sends_no_expunge() {
    let mut mailboxes = dovecot(3);
    mailboxes.behavior = Behavior {
        storing: Storing::Refuses,
        ..Behavior::default()
    };
    let (fake, mut session) = selected(mailboxes, INBOX).await;
    let refused = session.uid_expunge(&[2]).await;
    assert!(matches!(refused, Err(SessionError::Refused)));
    let commands = written(&fake);
    assert_eq!(
        commands.last().unwrap(),
        "UID STORE 2 +FLAGS.SILENT (\\Deleted)"
    );
    assert_eq!(uids_in(&fake, INBOX), [1, 2, 3]);
}

#[tokio::test]
async fn a_server_without_uidplus_moves_and_names_no_pairs() {
    let mut mailboxes = dovecot(2);
    mailboxes.extensions.remove(&Extension::Uidplus);
    let (fake, mut session) = selected(mailboxes, INBOX).await;
    assert_eq!(session.uid_move(&to(&[1], ARCHIVE)).await.unwrap(), None);
    assert_eq!(uids_in(&fake, ARCHIVE), [1]);
}

#[tokio::test]
async fn a_gmail_message_moves_between_stores_with_its_ids_and_without_the_inbox_label() {
    let mail = vec![
        Message::new(1).labeled(&["\\Inbox", "Work"]),
        Message::new(2),
    ];
    let (fake, mut session) = selected(Mailboxes::gmail(mail), ALL_MAIL).await;
    let trashed = session.uid_move(&to(&[1], TRASH)).await.unwrap();
    assert_eq!(trashed.unwrap().uids, [(1, 1)]);
    let folders = fake.script().mailboxes.folders();
    let trash = folders.iter().find(|folder| folder.name == TRASH).unwrap();
    assert_eq!(trash.mail[0].labels, ["Work"]);
    assert_eq!(trash.mail[0].msgid, Message::new(1).msgid);
    assert_eq!(uids_in(&fake, ALL_MAIL), [2]);
    session.select(TRASH).await.unwrap();
    let back = session.uid_move(&to(&[1], ALL_MAIL)).await.unwrap();
    assert_eq!(back.unwrap().uids, [(1, 3)]);
    assert_eq!(uids_in(&fake, ALL_MAIL), [2, 3]);
}

#[tokio::test]
async fn a_read_only_folder_takes_no_write_and_nothing_is_sent_rfc3501_6_3_1() {
    let mailboxes = dovecot(3);
    let mut folders = mailboxes.folders();
    folders[0].read_only = true;
    mailboxes.set(folders);
    let fake = start(mailboxes).await;
    let mut session = session(&fake).await;
    assert!(session.select(INBOX).await.unwrap().read_only);
    let flags = ["\\Seen".to_owned()];
    let store = StoreAsk {
        uids: &[1],
        flags: &flags,
        add: true,
        item: StoreItem::Flags,
    };
    let refusals = [
        session.uid_store(&store).await.err(),
        session.uid_move(&to(&[1], ARCHIVE)).await.err(),
        session.uid_copy(&to(&[1], ARCHIVE)).await.err(),
        session.uid_expunge(&[1]).await.err(),
    ];
    for refusal in refusals {
        assert!(matches!(
            refusal,
            Some(SessionError::Protocol(NOT_WRITABLE))
        ));
    }
    session.examine(ARCHIVE).await.unwrap();
    assert!(session.uid_expunge(&[1]).await.is_err());
    assert_eq!(written(&fake), ["SELECT \"INBOX\""]);
    // A SELECT the server grants read-write opens the way again.
    session.select(ARCHIVE).await.unwrap();
    session.uid_store(&store).await.unwrap();
}

#[tokio::test]
async fn a_copyuid_that_does_not_pair_with_the_ask_is_a_protocol_failure_rfc4315_3() {
    for code in [
        // Fewer UIDs in the target than in the source, then more.
        "COPYUID 9 2:3 5",
        "COPYUID 9 2 5:6",
        // A source UID the ask never named.
        "COPYUID 9 2:3,8 5:7",
        // A UID of zero.
        "COPYUID 9 2:3 0:1",
        // A range that would expand past any ask.
        "COPYUID 9 1:4294967295 1:4294967295",
    ] {
        for copy in [false, true] {
            let mut mailboxes = dovecot(3);
            mailboxes.behavior = Behavior {
                copyuid: Some(code),
                ..Behavior::default()
            };
            // The session is dropped after the failure, as a caller does.
            let (_fake, mut session) = selected(mailboxes, INBOX).await;
            let ask = to(&[2, 3], ARCHIVE);
            let answered = if copy {
                session.uid_copy(&ask).await
            } else {
                session.uid_move(&ask).await
            };
            assert!(
                matches!(answered, Err(SessionError::Protocol(_))),
                "{code} {copy}"
            );
        }
    }
}
