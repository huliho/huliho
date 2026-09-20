// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The header sync against the compose Dovecot: a folder half done
//! shows in `syncedEmails`, a subject arrives decoded and a message
//! nested past the guard's bound does not stall the folder.

#![cfg(feature = "live-targets")]

mod live_rig;

use std::fmt::Write as _;
use std::sync::Arc;

use huliho_imap_bridge::mailboxes::{SyncError, sync};
use huliho_imap_bridge::session::{MAX_NESTING, Session};
use huliho_imap_bridge::store::{AccountKey, ChangeKind, ObjectType, Store};
use huliho_imap_bridge::sync::{Cache, FolderSync, SYNC_BATCH, Step};
use huliho_imap_bridge::testing::seal::TestSealer;
use live_rig::{Editor, USER, answer, bridge_session, editor, mailbox_list, within};
use serde_json::json;

/// The header every appended message carries, so a later run finds what
/// an earlier one left.
const MARKER: (&str, &str) = ("X-Huliho-Live", "header-sync");

/// A little over one batch, so the sync shows a folder half done.
const APPENDED: usize = SYNC_BATCH + 10;

/// The subject of the newest appended message as RFC 2047 sends it.
const ENCODED_SUBJECT: &str = "=?UTF-8?Q?Caf=C3=A9_om_drie_uur?=";
const DECODED_SUBJECT: &str = "Café om drie uur";

/// The subject of the oldest appended message, whose body nests past
/// the guard's bound.
const NESTED_SUBJECT: &str = "Live nested";

/// The sessions the nested message may cost: nine halvings of a full
/// batch, the lone fetch and the one that goes through.
const MAX_SESSIONS: usize = 12;

/// Expunges what carries the marker; the INBOX is left as it was found.
async fn clear_appended(editor: &mut Editor) {
    within(editor.select("INBOX")).await.unwrap();
    let query = format!("HEADER {} {}", MARKER.0, MARKER.1);
    let uids = within(editor.uid_search(&query)).await.unwrap();
    if uids.is_empty() {
        return;
    }
    let set: Vec<String> = uids.iter().map(u32::to_string).collect();
    let store = format!("UID STORE {} +FLAGS.SILENT (\\Deleted)", set.join(","));
    within(editor.run_command_and_check_ok(&store))
        .await
        .unwrap();
    within(editor.run_command_and_check_ok("EXPUNGE"))
        .await
        .unwrap();
}

/// A multipart body `depth` levels deep around one attached PDF.
fn nested_body(depth: usize) -> String {
    let mut body = String::new();
    for level in 0..depth {
        let _ = write!(
            body,
            "Content-Type: multipart/mixed; boundary=\"b{level}\"\r\n\r\n--b{level}\r\n"
        );
    }
    body.push_str(
        "Content-Type: application/pdf\r\nContent-Disposition: attachment\r\n\r\n%PDF\r\n",
    );
    for level in (0..depth).rev() {
        let _ = write!(body, "--b{level}--\r\n");
    }
    body
}

/// The ids of the emails created after a state.
fn created_since(cache: &Cache, state: u64) -> Vec<String> {
    cache
        .store
        .changes_since(&cache.key, ObjectType::Email, state)
        .unwrap()
        .changes
        .unwrap()
        .into_iter()
        .filter(|change| change.kind == ChangeKind::Created)
        .map(|change| change.id)
        .collect()
}

/// `syncedEmails` and `totalEmails` of the inbox.
fn inbox_counts(cache: &Cache) -> (u64, u64) {
    let inbox = mailbox_list(&cache.store, &cache.key)
        .into_iter()
        .find(|mailbox| mailbox["role"] == "inbox")
        .unwrap();
    (
        inbox["syncedEmails"].as_u64().unwrap(),
        inbox["totalEmails"].as_u64().unwrap(),
    )
}

#[tokio::test]
async fn dovecot_syncs_its_inbox_in_batches_with_progress_in_synced_emails() {
    let mut editor = editor().await;
    clear_appended(&mut editor).await;
    for number in 1..=APPENDED {
        let (subject, body) = match number {
            1 => (NESTED_SUBJECT.to_owned(), nested_body(MAX_NESTING + 8)),
            APPENDED => (ENCODED_SUBJECT.to_owned(), "\r\nBody\r\n".to_owned()),
            _ => (format!("Live message {number}"), "\r\nBody\r\n".to_owned()),
        };
        let message = format!(
            "From: Sanne <{USER}>\r\nTo: {USER}\r\nSubject: {subject}\r\n{}: {}\r\nMessage-ID: <live-{number}@huliho.test>\r\nMIME-Version: 1.0\r\n{body}",
            MARKER.0, MARKER.1
        );
        within(editor.append("INBOX", None, None, message))
            .await
            .unwrap();
    }
    let cache = Cache {
        store: Arc::new(Store::in_memory().unwrap()),
        sealer: Arc::new(TestSealer::default()),
        key: AccountKey::new("live-sync"),
    };
    let mut session = bridge_session().await;
    sync(&mut session, Arc::clone(&cache.store), cache.key.clone())
        .await
        .unwrap();
    let rows = cache.store.mailbox_snapshot(&cache.key).unwrap().rows;
    let inbox = rows
        .iter()
        .find(|row| row.facts.role.as_deref() == Some("inbox"))
        .unwrap();
    let mut folder = FolderSync::open(&mut session, &cache, inbox)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        folder.batch(&mut session, &cache).await.unwrap(),
        Step::More
    );
    let (synced, total) = inbox_counts(&cache);
    assert_eq!(synced, u64::try_from(SYNC_BATCH).unwrap());
    assert!(synced < total, "{synced} of {total}");
    let newest = created_since(&cache, 1);
    let call = json!([
        "Email/get",
        { "accountId": cache.key.as_str(), "ids": newest, "properties": ["subject", "from"] },
        "c1"
    ]);
    let emails = answer(&cache.store, &cache.key, &call);
    let decoded = emails["list"]
        .as_array()
        .unwrap()
        .iter()
        .find(|email| email["subject"] == DECODED_SUBJECT)
        .expect("the newest message is in the first batch");
    assert_eq!(decoded["from"][0]["name"], "Sanne");
    // The nested message ends every session that fetches its structure.
    let mut sessions = 1;
    loop {
        match folder.finish(&mut session, &cache).await {
            Ok(step) => break assert_eq!(step, Step::Done),
            Err(SyncError::Session(_)) if sessions < MAX_SESSIONS => {
                session = bridge_session().await;
                sessions += 1;
                assert!(folder.resume(&mut session).await.unwrap());
            }
            Err(other) => panic!("{other}"),
        }
    }
    assert!(sessions > 1, "Dovecot describes the nested body in full");
    let (synced, total) = inbox_counts(&cache);
    assert_eq!(synced, total);
    assert!(total >= u64::try_from(APPENDED).unwrap());
    let call = json!([
        "Email/get",
        {
            "accountId": cache.key.as_str(),
            "ids": created_since(&cache, 2),
            "properties": ["subject", "hasAttachment"]
        },
        "c1"
    ]);
    let emails = answer(&cache.store, &cache.key, &call);
    let nested = emails["list"]
        .as_array()
        .unwrap()
        .iter()
        .find(|email| email["subject"] == NESTED_SUBJECT)
        .expect("the nested message arrives");
    assert_eq!(nested["hasAttachment"], false);
    session.logout().await.unwrap();
    clear_appended(&mut editor).await;
    within(editor.logout()).await.unwrap();
}
