// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The mailbox pass against the scripted server: the LIST form per
//! capability set, every line read, the difference logged.

mod mailboxes_rig;

use std::collections::BTreeSet;
use std::sync::Arc;

use huliho_imap_bridge::mailboxes::SyncError;
use huliho_imap_bridge::session::{
    ImapSession, ListReturn, STEP_TIMEOUT, Session, SessionError, StatusItems, TlsMode,
};
use huliho_imap_bridge::store::{ChangeKind, ObjectType, Store};
use huliho_imap_bridge::testing::imap::{FakeImap, HOST, Script};
use huliho_imap_bridge::testing::{Folder, Mailboxes, PASSWORD, USER};
use mailboxes_rig::{commands, key, pass, script, signed_in};

fn count(kinds: &[ChangeKind], kind: ChangeKind) -> usize {
    kinds.iter().filter(|found| **found == kind).count()
}

#[tokio::test]
async fn a_dovecot_shaped_server_is_listed_in_one_extended_list_with_status_rfc5819() {
    let fake = FakeImap::start(script(Mailboxes::dovecot())).await;
    let store = Arc::new(Store::in_memory().unwrap());
    assert_eq!(pass(&fake, &store).await.unwrap(), 1);
    assert_eq!(
        commands(&fake),
        [
            "CAPABILITY".to_owned(),
            format!("LOGIN \"{USER}\" \"{PASSWORD}\""),
            "CAPABILITY".to_owned(),
            "LIST \"\" \"*\" RETURN (SUBSCRIBED SPECIAL-USE STATUS (MESSAGES UNSEEN UIDNEXT UIDVALIDITY HIGHESTMODSEQ))".to_owned(),
            "LOGOUT".to_owned(),
        ]
    );
    let snapshot = store.mailbox_snapshot(&key()).unwrap();
    let roles: Vec<Option<&str>> = snapshot
        .rows
        .iter()
        .map(|row| row.facts.role.as_deref())
        .collect();
    assert_eq!(
        roles,
        [
            Some("inbox"),
            Some("drafts"),
            Some("sent"),
            Some("archive"),
            Some("junk"),
            Some("trash")
        ]
    );
    let inbox = &snapshot.rows[0].facts;
    assert_eq!(
        (
            inbox.total_emails,
            inbox.unread_emails,
            inbox.uid_next,
            inbox.highest_modseq
        ),
        (17, 3, Some(18), Some(1))
    );
    assert!(
        snapshot
            .rows
            .iter()
            .all(|row| row.facts.subscribed && row.facts.selectable)
    );
}

#[tokio::test]
async fn a_plain_server_gets_list_lsub_and_a_status_per_selectable_mailbox() {
    let mailboxes = Mailboxes::new(
        vec![
            Folder::new("INBOX").with_counts(2, 1),
            Folder::noselect("Mail"),
            Folder {
                subscribed: false,
                ..Folder::new("Mail/Sent Items")
            },
        ],
        BTreeSet::new(),
    );
    let fake = FakeImap::start(script(mailboxes)).await;
    let store = Arc::new(Store::in_memory().unwrap());
    pass(&fake, &store).await.unwrap();
    assert_eq!(
        &commands(&fake)[3..],
        [
            "LIST \"\" \"*\"",
            "STATUS \"INBOX\" (MESSAGES UNSEEN UIDNEXT UIDVALIDITY)",
            "STATUS \"Mail/Sent Items\" (MESSAGES UNSEEN UIDNEXT UIDVALIDITY)",
            "LSUB \"\" \"*\"",
            "LOGOUT",
        ]
    );
    let rows = store.mailbox_snapshot(&key()).unwrap().rows;
    let by_name = |name: &str| rows.iter().find(|row| row.facts.imap_name == name).unwrap();
    let sent = by_name("Mail/Sent Items");
    assert_eq!(sent.facts.role.as_deref(), Some("sent"));
    assert!(!sent.facts.subscribed);
    assert_eq!(sent.parent_id.as_ref(), Some(&by_name("Mail").id));
    let mail = by_name("Mail");
    assert!(!mail.facts.selectable);
    assert_eq!((mail.facts.total_emails, mail.facts.uid_next), (0, None));
    assert_eq!(by_name("INBOX").facts.unread_emails, 1);
}

#[tokio::test]
async fn a_mailbox_that_refuses_status_keeps_a_row_without_counts_and_the_pass_goes_on() {
    let mailboxes = Mailboxes::new(
        vec![
            Folder::new("INBOX").with_counts(2, 1),
            Folder {
                refuses_status: true,
                ..Folder::new("Shared").with_counts(5, 2)
            },
            Folder::new("Work").with_counts(4, 0),
        ],
        BTreeSet::new(),
    );
    let fake = FakeImap::start(script(mailboxes)).await;
    let store = Arc::new(Store::in_memory().unwrap());
    assert_eq!(pass(&fake, &store).await.unwrap(), 1);
    assert_eq!(
        &commands(&fake)[3..],
        [
            "LIST \"\" \"*\"",
            "STATUS \"INBOX\" (MESSAGES UNSEEN UIDNEXT UIDVALIDITY)",
            "STATUS \"Shared\" (MESSAGES UNSEEN UIDNEXT UIDVALIDITY)",
            "STATUS \"Work\" (MESSAGES UNSEEN UIDNEXT UIDVALIDITY)",
            "LSUB \"\" \"*\"",
            "LOGOUT",
        ]
    );
    let rows = store.mailbox_snapshot(&key()).unwrap().rows;
    let facts = |name: &str| {
        let row = rows.iter().find(|row| row.facts.imap_name == name);
        &row.unwrap().facts
    };
    let shared = facts("Shared");
    assert!(shared.selectable);
    assert_eq!((shared.total_emails, shared.unread_emails), (0, 0));
    assert_eq!(
        (shared.uid_validity, shared.uid_next, shared.highest_modseq),
        (None, None, None)
    );
    let inbox = facts("INBOX");
    assert_eq!((inbox.total_emails, inbox.unread_emails), (2, 1));
    let work = facts("Work");
    assert_eq!((work.total_emails, work.uid_next), (4, Some(5)));
}

#[tokio::test]
async fn a_status_line_after_list_and_an_alert_in_between_are_read_never_dropped() {
    let mut mailboxes = Mailboxes::dovecot();
    mailboxes.chatter = true;
    mailboxes.push(Folder::noselect("Lists"));
    mailboxes.push(Folder {
        refuses_status: true,
        ..Folder::new("Shared").with_counts(5, 2)
    });
    let fake = FakeImap::start(script(mailboxes)).await;
    let store = Arc::new(Store::in_memory().unwrap());
    pass(&fake, &store).await.unwrap();
    let rows = store.mailbox_snapshot(&key()).unwrap().rows;
    assert_eq!(rows.len(), 8);
    assert_eq!(rows[0].facts.total_emails, 17);
    for name in ["Lists", "Shared"] {
        let row = rows.iter().find(|row| row.facts.imap_name == name);
        let facts = &row.unwrap().facts;
        assert_eq!(
            (facts.total_emails, facts.unread_emails, facts.uid_validity),
            (0, 0, None),
            "{name}"
        );
    }
    assert!(
        commands(&fake)
            .iter()
            .all(|command| !command.starts_with("STATUS"))
    );
}

/// The capability list is read before the sign-in, so an answer without
/// `IMAP4rev1` ends the connection there.
#[tokio::test]
async fn a_server_without_imap4rev1_is_refused_at_connect_before_any_list() {
    let fake = FakeImap::start(Script {
        capabilities: "IMAP4rev2 AUTH=PLAIN AUTH=XOAUTH2",
        ..script(Mailboxes::dovecot())
    })
    .await;
    let target = fake.target(HOST, TlsMode::Implicit);
    let Err(error) = ImapSession::connect(fake.trusting(), &target, STEP_TIMEOUT).await else {
        panic!("a capability list without IMAP4rev1 is refused");
    };
    assert!(
        matches!(
            error,
            SessionError::Protocol("the answer could not be parsed")
        ),
        "{error}"
    );
    assert_eq!(commands(&fake), ["CAPABILITY"]);
}

#[tokio::test]
async fn a_second_pass_logs_the_difference_and_an_equal_one_leaves_the_state() {
    let mailboxes = Mailboxes::dovecot();
    let fake = FakeImap::start(script(mailboxes.clone())).await;
    let store = Arc::new(Store::in_memory().unwrap());
    assert_eq!(pass(&fake, &store).await.unwrap(), 1);
    assert_eq!(pass(&fake, &store).await.unwrap(), 1);
    mailboxes.push(Folder::new("Work"));
    mailboxes.remove("Archive");
    let mut folders = mailboxes.folders();
    folders[0] = Folder::new("INBOX").with_counts(18, 4);
    mailboxes.set(folders);
    assert_eq!(pass(&fake, &store).await.unwrap(), 2);
    let since = store.changes_since(&key(), ObjectType::Mailbox, 1).unwrap();
    let kinds: Vec<ChangeKind> = since
        .changes
        .unwrap()
        .iter()
        .map(|change| change.kind)
        .collect();
    assert_eq!(count(&kinds, ChangeKind::Created), 1);
    assert_eq!(count(&kinds, ChangeKind::Updated), 1);
    assert_eq!(count(&kinds, ChangeKind::Destroyed), 1);
    assert_eq!(store.mailbox_snapshot(&key()).unwrap().rows.len(), 6);
}

#[tokio::test]
async fn the_scripted_server_refuses_a_form_beyond_its_capabilities() {
    let fake = FakeImap::start(script(Mailboxes::default())).await;
    let mut session = signed_in(&fake).await;
    let error = session
        .list(ListReturn {
            subscribed: true,
            ..ListReturn::default()
        })
        .await
        .unwrap_err();
    assert!(
        matches!(error, SessionError::Protocol("the server answered BAD")),
        "{error}"
    );
    let missing = session
        .status("Nowhere", StatusItems { modseq: false })
        .await
        .unwrap_err();
    assert!(matches!(missing, SessionError::Refused), "{missing}");
    session.logout().await.unwrap();
}

#[tokio::test]
async fn a_mailbox_listed_twice_is_one_row_under_one_status() {
    let twice = vec![Folder::new("INBOX"), Folder::new("INBOX")];
    let fake = FakeImap::start(script(Mailboxes::new(twice, BTreeSet::new()))).await;
    let store = Arc::new(Store::in_memory().unwrap());
    assert_eq!(pass(&fake, &store).await.unwrap(), 1);
    assert_eq!(store.mailbox_snapshot(&key()).unwrap().rows.len(), 1);
    let statuses = commands(&fake)
        .iter()
        .filter(|command| command.starts_with("STATUS"))
        .count();
    assert_eq!(statuses, 1);
}

#[tokio::test]
async fn a_listing_without_a_mailbox_fails_the_pass_and_keeps_every_row_rfc3501_5_1() {
    let mailboxes = Mailboxes::dovecot();
    let fake = FakeImap::start(script(mailboxes.clone())).await;
    let store = Arc::new(Store::in_memory().unwrap());
    assert_eq!(pass(&fake, &store).await.unwrap(), 1);
    mailboxes.set(Vec::new());
    let error = pass(&fake, &store).await.unwrap_err();
    assert!(
        matches!(
            error,
            SyncError::Session(SessionError::Protocol("LIST answered without a mailbox"))
        ),
        "{error}"
    );
    let snapshot = store.mailbox_snapshot(&key()).unwrap();
    assert_eq!((snapshot.state, snapshot.rows.len()), (1, 6));
}

#[tokio::test]
async fn a_dot_delimited_tree_maps_the_role_by_the_leaf_under_inbox() {
    let folders = vec![Folder::new("INBOX"), Folder::new("INBOX.Sent")];
    let mut mailboxes = Mailboxes::new(folders, BTreeSet::new());
    mailboxes.delimiter = '.';
    let fake = FakeImap::start(script(mailboxes)).await;
    let store = Arc::new(Store::in_memory().unwrap());
    pass(&fake, &store).await.unwrap();
    let rows = store.mailbox_snapshot(&key()).unwrap().rows;
    let by_name = |name: &str| rows.iter().find(|row| row.facts.imap_name == name).unwrap();
    let sent = by_name("INBOX.Sent");
    assert_eq!(
        (sent.facts.name.as_str(), sent.facts.role.as_deref()),
        ("Sent", Some("sent"))
    );
    assert_eq!(sent.parent_id.as_ref(), Some(&by_name("INBOX").id));
}
