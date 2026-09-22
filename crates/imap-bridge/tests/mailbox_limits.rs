// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What the mailbox pass refuses or leaves out: a listing past its
//! limits and a name that cannot go on a command line.

mod mailboxes_rig;

use std::collections::BTreeSet;
use std::sync::Arc;

use huliho_imap_bridge::mailboxes::{SyncError, sync};
use huliho_imap_bridge::session::{Session, SessionError, StatusItems};
use huliho_imap_bridge::store::Store;
use huliho_imap_bridge::testing::imap::FakeImap;
use huliho_imap_bridge::testing::{Extension, Folder, Mailboxes};
use mailboxes_rig::{cache, commands, key, pass, script, signed_in};

/// The mailboxes one account may list.
const MAILBOX_LIMIT: usize = 10_000;

/// More lines than one listing may carry, which is two per mailbox of
/// the limit with some to spare.
const PAST_THE_LINE_LIMIT: usize = 25_000;

/// That many selectable folders, `F0` first.
fn numbered(count: usize) -> Vec<Folder> {
    (0..count)
        .map(|number| Folder::new(&format!("F{number}")))
        .collect()
}

/// After the refusal the stream holds unread lines, so that session is
/// dropped; a listing of exactly the limit fits with its STATUS lines.
#[tokio::test]
async fn a_listing_past_the_mailbox_limit_fails_the_pass_and_writes_nothing() {
    let mailboxes = Mailboxes::new(numbered(MAILBOX_LIMIT + 1), Extension::all());
    let fake = FakeImap::start(script(mailboxes.clone())).await;
    let store = Arc::new(Store::in_memory().unwrap());
    let mut session = signed_in(&fake).await;
    let error = sync(&mut session, &cache(&store)).await.unwrap_err();
    assert!(
        matches!(
            error,
            SyncError::Session(SessionError::Protocol(
                "the listing passes the mailbox limit"
            ))
        ),
        "{error}"
    );
    drop(session);
    let untouched = store.mailbox_snapshot(&key()).unwrap();
    assert_eq!((untouched.state, untouched.rows.len()), (0, 0));
    mailboxes.remove("F0");
    assert_eq!(pass(&fake, &store).await.unwrap(), 1);
    let rows = store.mailbox_snapshot(&key()).unwrap().rows;
    assert_eq!(rows.len(), MAILBOX_LIMIT);
}

/// The refusal LSUB meets over those folders, every one subscribed.
async fn lsub_refusal(folders: Vec<Folder>) -> SessionError {
    let fake = FakeImap::start(script(Mailboxes::new(folders, BTreeSet::new()))).await;
    let mut session = signed_in(&fake).await;
    let Err(error) = session.lsub().await else {
        panic!("an answer of that size is refused");
    };
    error
}

/// A name that holds a line break counts toward no mailbox, so the line
/// limit alone bounds an answer made of such names.
#[tokio::test]
async fn an_answer_past_the_line_limit_is_refused() {
    let unsendable = (0..PAST_THE_LINE_LIMIT)
        .map(|number| Folder::new(&format!("F{number}\n")))
        .collect();
    let error = lsub_refusal(unsendable).await;
    assert!(
        matches!(
            error,
            SessionError::Protocol("the answer passes the line limit")
        ),
        "{error}"
    );
}

#[tokio::test]
async fn subscribed_names_past_the_mailbox_limit_are_refused() {
    let error = lsub_refusal(numbered(MAILBOX_LIMIT + 1)).await;
    assert!(
        matches!(
            error,
            SessionError::Protocol("the listing passes the mailbox limit")
        ),
        "{error}"
    );
}

#[tokio::test]
async fn status_refuses_a_name_holding_a_line_break_before_any_command_is_sent() {
    let fake = FakeImap::start(script(Mailboxes::default())).await;
    let mut session = signed_in(&fake).await;
    let error = session
        .status("a\r\nA9 NOOP", StatusItems { modseq: false })
        .await
        .unwrap_err();
    assert!(
        matches!(
            error,
            SessionError::Protocol("the mailbox name cannot be sent")
        ),
        "{error}"
    );
    session.logout().await.unwrap();
    assert_eq!(commands(&fake)[2..], ["LOGOUT"]);
}

#[tokio::test]
async fn a_listed_name_holding_a_line_break_is_left_out_and_never_sent_back() {
    let folders = vec![
        Folder::new("INBOX"),
        Folder::new("x\r\nA9 DELETE INBOX"),
        Folder::new("Work"),
    ];
    let fake = FakeImap::start(script(Mailboxes::new(folders, BTreeSet::new()))).await;
    let store = Arc::new(Store::in_memory().unwrap());
    assert_eq!(pass(&fake, &store).await.unwrap(), 1);
    assert_eq!(
        &commands(&fake)[3..],
        [
            "LIST \"\" \"*\"",
            "STATUS \"INBOX\" (MESSAGES UNSEEN UIDNEXT UIDVALIDITY)",
            "STATUS \"Work\" (MESSAGES UNSEEN UIDNEXT UIDVALIDITY)",
            "LSUB \"\" \"*\"",
            "LOGOUT",
        ]
    );
    let rows = store.mailbox_snapshot(&key()).unwrap().rows;
    let names: BTreeSet<&str> = rows
        .iter()
        .map(|row| row.facts.imap_name.as_str())
        .collect();
    assert_eq!(names, ["INBOX", "Work"].into_iter().collect());
}
