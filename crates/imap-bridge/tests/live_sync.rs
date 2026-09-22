// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The header sync against the compose Dovecot: a folder half done
//! shows in `syncedEmails`, a subject arrives decoded and a message
//! nested past the guard's bound does not stall the folder. Then the
//! refresh: what a second connection delivers, flags and expunges in a
//! folder of its own reaches the cache through `/changes`.

#![cfg(feature = "live-targets")]

mod live_rig;

use std::fmt::Write as _;

use huliho_imap_bridge::mailboxes::{SyncError, sync};
use huliho_imap_bridge::runtime::Link;
use huliho_imap_bridge::session::{MAX_NESTING, Session};
use huliho_imap_bridge::store::{ChangeKind, ObjectType};
use huliho_imap_bridge::sync::{Cache, FolderSync, SYNC_BATCH, Step};
use huliho_imap_bridge::testing::TestConnector;
use live_rig::{Editor, USER, answer, bridge_session, cache, editor, link, mailbox_list, within};
use serde_json::{Value, json};

/// The header every appended message carries, so a later run finds what
/// an earlier one left.
const MARKER: (&str, &str) = ("X-Huliho-Live", "header-sync");

/// A little over one batch, so the sync shows a folder half done.
const APPENDED: usize = SYNC_BATCH + 10;

/// The folder the refresh test works in, created and removed by the
/// test, so the sync test's INBOX and this one never meet.
const REFRESH_FOLDER: &str = "Huliho refresh";

/// The messages the refresh test starts with.
const REFRESH_MESSAGES: usize = 5;

/// The window the refresh test asks for.
const WINDOW: usize = 2;

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
async fn inbox_counts(cache: &Cache, link: &Link<TestConnector>) -> (u64, u64) {
    let inbox = mailbox_list(cache, link)
        .await
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
    let cache = cache("live-sync");
    let link = link();
    let mut session = bridge_session().await;
    sync(&mut session, &cache).await.unwrap();
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
    let (synced, total) = inbox_counts(&cache, &link).await;
    assert_eq!(synced, u64::try_from(SYNC_BATCH).unwrap());
    assert!(synced < total, "{synced} of {total}");
    let newest = created_since(&cache, 1);
    let call = json!([
        "Email/get",
        { "accountId": cache.key.as_str(), "ids": newest, "properties": ["subject", "from"] },
        "c1"
    ]);
    let emails = answer(&cache, &link, &call).await;
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
    let (synced, total) = inbox_counts(&cache, &link).await;
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
    let emails = answer(&cache, &link, &call).await;
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

/// One message of the refresh folder, its number in its Message-ID.
fn refresh_message(number: usize) -> String {
    format!(
        "From: Sanne <{USER}>\r\nTo: {USER}\r\nSubject: Refresh message {number}\r\nMessage-ID: <refresh-{number}@huliho.test>\r\nMIME-Version: 1.0\r\n\r\nBody of refresh message {number}\r\n"
    )
}

/// Appends a refresh message with an INTERNALDATE a second past the one
/// before it, since Dovecot would give messages appended within one
/// second the same date and the window would then sort them by id. The
/// client library sends the date as written, so the quotes RFC 3501
/// section 6.3.11 asks for go here.
async fn deliver(editor: &mut Editor, number: usize) {
    let received = format!("\"20-Sep-2026 12:00:{number:02} +0000\"");
    let appended = editor.append(
        REFRESH_FOLDER,
        None,
        Some(&received),
        refresh_message(number),
    );
    within(appended).await.unwrap();
}

/// Removes the refresh folder an earlier run left, on a connection of
/// its own.
async fn clear_refresh_folder() {
    let mut sweeper = editor().await;
    if within(sweeper.delete(REFRESH_FOLDER)).await.is_ok() {
        within(sweeper.logout()).await.unwrap();
    }
}

/// The UID Dovecot gave the refresh message with this number.
async fn uid_of(editor: &mut Editor, number: usize) -> u32 {
    let query = format!("HEADER Message-ID <refresh-{number}@huliho.test>");
    let uids = within(editor.uid_search(&query)).await.unwrap();
    assert_eq!(uids.len(), 1, "{query}");
    uids.into_iter().next().unwrap()
}

/// Sets one flag on the refresh message with this number, on the second
/// connection.
async fn flag(editor: &mut Editor, number: usize, flag: &str) {
    let uid = uid_of(editor, number).await;
    let store = format!("UID STORE {uid} +FLAGS.SILENT ({flag})");
    within(editor.run_command_and_check_ok(&store))
        .await
        .unwrap();
}

/// `Email/changes` since a state.
async fn email_changes(cache: &Cache, link: &Link<TestConnector>, since: &Value) -> Value {
    let call = json!([
        "Email/changes",
        { "accountId": cache.key.as_str(), "sinceState": since },
        "c1"
    ]);
    answer(cache, link, &call).await
}

/// The newest `WINDOW` emails of a folder with the total.
async fn window_of(cache: &Cache, link: &Link<TestConnector>, folder: &Value) -> Value {
    let query = json!([
        "Email/query",
        {
            "accountId": cache.key.as_str(),
            "filter": { "inMailbox": folder },
            "calculateTotal": true,
            "limit": WINDOW
        },
        "c1"
    ]);
    answer(cache, link, &query).await
}

/// The named properties of these emails.
async fn properties(
    cache: &Cache,
    link: &Link<TestConnector>,
    ids: &Value,
    names: &[&str],
) -> Value {
    let call = json!([
        "Email/get",
        { "accountId": cache.key.as_str(), "ids": ids, "properties": names },
        "c1"
    ]);
    answer(cache, link, &call).await["list"].take()
}

#[tokio::test]
async fn dovecot_changes_from_a_second_connection_reach_the_cache_through_a_refresh() {
    clear_refresh_folder().await;
    let mut editor = editor().await;
    within(editor.create(REFRESH_FOLDER)).await.unwrap();
    for number in 1..=REFRESH_MESSAGES {
        deliver(&mut editor, number).await;
    }
    let cache = cache("live-refresh");
    let link = link();
    let mut session = bridge_session().await;
    sync(&mut session, &cache).await.unwrap();
    let rows = cache.store.mailbox_snapshot(&cache.key).unwrap().rows;
    let folder = rows
        .iter()
        .find(|row| row.facts.imap_name == REFRESH_FOLDER)
        .expect("the pass lists the created folder");
    let mut first = FolderSync::open(&mut session, &cache, folder)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        first.finish(&mut session, &cache).await.unwrap(),
        Step::Done
    );
    session.logout().await.unwrap();
    let folder = json!(folder.id);
    let window = window_of(&cache, &link, &folder).await;
    assert_eq!(window["total"], REFRESH_MESSAGES);
    assert_eq!(window["ids"].as_array().unwrap().len(), WINDOW);
    let newest = json!([window["ids"][0]]);
    let newest = properties(&cache, &link, &newest, &["subject", "preview"]).await;
    assert_eq!(
        newest[0]["subject"],
        format!("Refresh message {REFRESH_MESSAGES}")
    );
    assert_eq!(
        newest[0]["preview"],
        format!("Body of refresh message {REFRESH_MESSAGES}")
    );
    // The fetched preview is one state of its own; the changes start
    // after it.
    let synced = json!(cache.store.state(&cache.key).unwrap().to_string());
    // Another client delivers one message and flags another.
    within(editor.select(REFRESH_FOLDER)).await.unwrap();
    deliver(&mut editor, REFRESH_MESSAGES + 1).await;
    flag(&mut editor, 1, "\\Flagged").await;
    let changed = email_changes(&cache, &link, &synced).await;
    assert_eq!(changed["created"].as_array().unwrap().len(), 1, "{changed}");
    assert_eq!(changed["updated"].as_array().unwrap().len(), 1, "{changed}");
    assert_eq!(changed["destroyed"], json!([]));
    let updated = properties(&cache, &link, &changed["updated"], &["keywords"]).await;
    assert_eq!(updated[0]["keywords"]["$flagged"], true);
    // The same client expunges one.
    flag(&mut editor, 2, "\\Deleted").await;
    within(editor.run_command_and_check_ok("EXPUNGE"))
        .await
        .unwrap();
    let gone = email_changes(&cache, &link, &changed["newState"]).await;
    assert_eq!(gone["destroyed"].as_array().unwrap().len(), 1, "{gone}");
    assert_eq!(gone["created"], json!([]));
    let recounted = window_of(&cache, &link, &folder).await;
    assert_eq!(recounted["total"], REFRESH_MESSAGES);
    within(editor.close()).await.unwrap();
    within(editor.delete(REFRESH_FOLDER)).await.unwrap();
    within(editor.logout()).await.unwrap();
}
