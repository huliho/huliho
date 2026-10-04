// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What a server that fails costs a blob: a NO and a renumbered folder
//! leave the session standing, a window the bridge cannot read costs
//! it, a connection lost or an account let go of mid-stream breaks the
//! blob off, a server out of reach opens none and a stream that stops
//! without its closing step is an error.

mod blobs_rig;
mod sync_rig;

use std::sync::Arc;
use std::time::Duration;

use blobs_rig::{
    Blobs, CORPUS, LONG, PLAIN, ROOMY, WIDE, dataset, drained, linked, message, settled, started,
    started_over,
};
use huliho_imap_bridge::blob::{BLOB_BUFFER_WINDOWS, Blob, BlobError, open};
use huliho_imap_bridge::runtime::Link;
use huliho_imap_bridge::testing::{Behavior, Folder, TestConnector};
use sync_rig::{Rig, inbox};
use tokio::sync::mpsc;

/// The UID FETCH commands a connection answers before it closes.
const ANSWERED: usize = 2;

/// A UIDVALIDITY the folder never had.
const RENUMBERED: u32 = 9;

/// An origin octet no window of the tests asks for.
const ELSEWHERE: usize = 7;

/// The UID FETCH of a connection that never gets an answer: its second.
const STALLED: usize = 1;

/// A deadline well under the time the scripted connection gives a read.
const SHORT_DEADLINE: Duration = Duration::from_millis(300);

/// The sign-ins the server saw, one per session opened.
fn logins(rig: &Rig) -> usize {
    rig.fake
        .lines()
        .iter()
        .filter(|line| line.contains(" LOGIN "))
        .count()
}

/// A synced rig over a server that misbehaves as `behavior` says.
async fn started_with(behavior: Behavior) -> Blobs {
    let mut mailboxes = inbox(dataset());
    mailboxes.behavior = behavior;
    started_over(mailboxes).await
}

/// Whether the blob cannot be read right now.
async fn unavailable(blobs: &Blobs, blob_id: &str) -> bool {
    matches!(
        open(&blobs.rig.cache, &blobs.link, blob_id, ROOMY).await,
        Err(BlobError::Unavailable)
    )
}

/// The PDF of the corpus, read whole.
async fn pdf(blobs: &Blobs) -> Vec<u8> {
    let blob_id = format!("{}-2", blobs.ids[&CORPUS]);
    let blob = open(&blobs.rig.cache, &blobs.link, &blob_id, ROOMY)
        .await
        .unwrap();
    drained(blob).await.unwrap()
}

#[tokio::test]
async fn a_no_on_the_messages_own_fetch_and_a_renumbered_folder_keep_the_session() {
    let behavior = Behavior {
        refuses_body_of: Some(PLAIN),
        ..Behavior::default()
    };
    let blobs = started_with(behavior).await;
    // The first blob opens the session of the link.
    assert_eq!(pdf(&blobs).await, b"%PDF-1");
    let before = logins(&blobs.rig);
    assert!(unavailable(&blobs, &blobs.ids[&PLAIN]).await);
    assert_eq!(pdf(&blobs).await, b"%PDF-1");
    // Under another UIDVALIDITY the UID names another message.
    let renumbered = Folder {
        uid_validity: RENUMBERED,
        ..Folder::new("INBOX").with_mail(dataset())
    };
    blobs.rig.fake.script().mailboxes.set(vec![renumbered]);
    assert!(unavailable(&blobs, &blobs.ids[&CORPUS]).await);
    assert_eq!(logins(&blobs.rig), before, "{:?}", blobs.rig.fake.lines());
}

#[tokio::test]
async fn a_window_answered_at_another_offset_costs_the_session() {
    let behavior = Behavior {
        misplaced_origin: Some(ELSEWHERE),
        ..Behavior::default()
    };
    let blobs = started_with(behavior).await;
    let before = logins(&blobs.rig);
    for attempt in 1..=2 {
        assert!(unavailable(&blobs, &blobs.ids[&PLAIN]).await);
        assert_eq!(logins(&blobs.rig), before + attempt);
    }
}

#[tokio::test]
async fn a_server_out_of_reach_opens_no_blob_and_a_connection_lost_mid_stream_breaks_it_off() {
    let mut mailboxes = inbox(vec![message(LONG)]);
    mailboxes.behavior = Behavior {
        drops_after: Some(ANSWERED),
        ..Behavior::default()
    };
    let blobs = started_over(mailboxes).await;
    let long = &blobs.ids[&LONG];
    let blob = open(&blobs.rig.cache, &blobs.link, long, ROOMY)
        .await
        .unwrap();
    assert!(matches!(drained(blob).await, Err(BlobError::Unavailable)));
    let refusing = Blobs {
        link: Arc::new(Link::with_interval(TestConnector::Refusing, Duration::ZERO)),
        ..blobs
    };
    assert!(unavailable(&refusing, &refusing.ids[&LONG]).await);
}

#[tokio::test]
async fn a_window_that_runs_past_the_deadline_breaks_the_blob_off_and_costs_the_session() {
    let mut mailboxes = inbox(vec![message(LONG)]);
    mailboxes.behavior = Behavior {
        stalls_at: Some(STALLED),
        ..Behavior::default()
    };
    let blobs = started_over(mailboxes).await;
    let link = linked(&blobs.rig, SHORT_DEADLINE);
    let long = &blobs.ids[&LONG];
    let blob = open(&blobs.rig.cache, &link, long, ROOMY).await.unwrap();
    assert!(matches!(drained(blob).await, Err(BlobError::Unavailable)));
    // The session that stalled is gone: the next blob signs in again.
    let before = logins(&blobs.rig);
    let again = open(&blobs.rig.cache, &link, long, ROOMY).await;
    assert!(again.is_ok());
    assert_eq!(logins(&blobs.rig), before + 1);
}

#[tokio::test]
async fn an_account_the_runtime_let_go_of_breaks_its_stream_off() {
    let Blobs { rig, link, ids } = started().await;
    let blob = open(&rig.cache, &link, &ids[&WIDE], ROOMY).await.unwrap();
    assert!(settled(&rig, 1 + BLOB_BUFFER_WINDOWS).await);
    drop(link);
    assert!(matches!(drained(blob).await, Err(BlobError::Unavailable)));
}

#[tokio::test]
async fn a_stream_that_stops_without_its_closing_step_is_an_error_and_never_a_whole_blob() {
    let (sender, receiver) = mpsc::channel(2);
    sender.send(Ok(Some(b"whole".to_vec()))).await.unwrap();
    sender.send(Ok(None)).await.unwrap();
    drop(sender);
    let whole = drained(Blob::scripted(receiver)).await;
    assert_eq!(whole.unwrap(), b"whole");
    let (sender, receiver) = mpsc::channel(1);
    sender.send(Ok(Some(b"half".to_vec()))).await.unwrap();
    drop(sender);
    let mut cut = Blob::scripted(receiver);
    assert!(matches!(cut.next().await, Some(Ok(_))));
    assert!(matches!(cut.next().await, Some(Err(BlobError::Task))));
    // An error is the last thing a blob answers.
    assert!(cut.next().await.is_none());
}
