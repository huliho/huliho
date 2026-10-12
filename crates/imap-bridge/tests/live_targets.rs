// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The checks against the compose Dovecot: the credentials, the mailbox
//! pass, the JMAP methods reading what it wrote and the session's moves.

#![cfg(feature = "live-targets")]

mod live_rig;

use huliho_imap_bridge::mailboxes::sync;
use huliho_imap_bridge::runtime::Link;
use huliho_imap_bridge::session::{
    MoveAsk, STEP_TIMEOUT, Session, SessionError, StatusItems, TlsMode,
};
use huliho_imap_bridge::smtp;
use huliho_imap_bridge::sync::Cache;
use huliho_imap_bridge::testing::TestConnector;
use huliho_imap_bridge::utf7;
use huliho_imap_bridge::verify::{Credential, VerifyError, verify};
use live_rig::{
    Editor, IMAPS_PORT, PASSWORD, USER, answer, bridge_session, cache, dovecot, editor, link,
    mailbox_list, tls, within,
};
use serde_json::{Value, json};

const IMAP_PORT: u16 = 31143;
const SUBMISSION_PORT: u16 = 31587;

/// A mailbox with a character outside ASCII, so the UTF-7 path runs
/// live.
const CREATED_NAME: &str = "Bijlagen é";

/// The folders the move check and the copy check work in, each pair its
/// own. The compose Dovecot always holds them, so no check adds a
/// mailbox the listing check would count.
const MOVE_FOLDERS: (&str, &str) = ("Drafts", "Archive");
const COPY_FOLDERS: (&str, &str) = ("Sent", "Trash");

/// The checks that change the account take turns: a count the move
/// checks change mid-pass is an update the listing check would read.
static ACCOUNT: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The status items the checks read a folder's UIDVALIDITY by.
const VALIDITY: StatusItems = StatusItems { modseq: false };

fn password(password: &str) -> Credential {
    Credential::Password {
        username: USER.to_owned(),
        password: password.to_owned(),
    }
}

/// Removes what an earlier run left, on a connection of its own: the
/// parser refuses Dovecot's NO for a missing name, which holds UTF-8.
async fn clear(wire: &str) {
    let mut editor = editor().await;
    if within(editor.delete(wire)).await.is_ok() {
        within(editor.logout()).await.unwrap();
    }
}

async fn changes_since(cache: &Cache, link: &Link<TestConnector>, since: &str) -> Value {
    let call = json!([
        "Mailbox/changes",
        { "accountId": cache.key.as_str(), "sinceState": since },
        "c1"
    ]);
    answer(cache, link, &call).await
}

#[tokio::test]
async fn dovecot_accepts_the_password_over_implicit_tls() {
    let target = dovecot(IMAPS_PORT, TlsMode::Implicit);
    let capabilities = verify(tls(true), &target, &password(PASSWORD), STEP_TIMEOUT)
        .await
        .unwrap();
    assert!(capabilities.has("IMAP4rev2"));
    assert!(capabilities.has("IDLE"));
}

#[tokio::test]
async fn dovecot_upgrades_with_starttls_rfc9051_6_2_1() {
    let target = dovecot(IMAP_PORT, TlsMode::Starttls);
    let capabilities = verify(tls(true), &target, &password(PASSWORD), STEP_TIMEOUT)
        .await
        .unwrap();
    assert!(capabilities.has("IMAP4rev2"));
    assert!(!capabilities.has("STARTTLS"));
}

#[tokio::test]
async fn dovecot_rejects_a_wrong_password() {
    let target = dovecot(IMAPS_PORT, TlsMode::Implicit);
    let error = verify(tls(true), &target, &password("wrong"), STEP_TIMEOUT)
        .await
        .unwrap_err();
    assert!(matches!(error, VerifyError::CredentialRejected), "{error}");
}

#[tokio::test]
async fn dovecot_is_insecure_without_the_dev_ca() {
    let target = dovecot(IMAPS_PORT, TlsMode::Implicit);
    let error = verify(tls(false), &target, &password(PASSWORD), STEP_TIMEOUT)
        .await
        .unwrap_err();
    assert!(
        matches!(error, VerifyError::Insecure(SessionError::Tls(_))),
        "{error}"
    );
}

#[tokio::test]
async fn dovecot_accepts_the_password_on_submission_over_starttls_rfc3207_4() {
    let target = dovecot(SUBMISSION_PORT, TlsMode::Starttls);
    smtp::verify(tls(true), &target, &password(PASSWORD), STEP_TIMEOUT)
        .await
        .unwrap();
}

#[tokio::test]
async fn dovecot_rejects_a_wrong_password_on_submission() {
    let target = dovecot(SUBMISSION_PORT, TlsMode::Starttls);
    let error = smtp::verify(tls(true), &target, &password("wrong"), STEP_TIMEOUT)
        .await
        .unwrap_err();
    assert!(matches!(error, VerifyError::CredentialRejected), "{error}");
}

#[tokio::test]
async fn dovecot_lists_its_mailboxes_and_a_created_one_travels_through_changes() {
    let _account = ACCOUNT.lock().await;
    let wire = utf7::encode(CREATED_NAME);
    clear(&wire).await;
    let mut editor = editor().await;
    let cache = cache("live");
    let link = link();
    let mut session = bridge_session().await;
    assert_eq!(sync(&mut session, &cache).await.unwrap(), 1);
    let list = mailbox_list(&cache, &link).await;
    assert!(
        list.iter().any(|mailbox| mailbox["role"] == "inbox"),
        "{list:?}"
    );
    assert!(
        list.iter().all(|mailbox| mailbox["name"] != CREATED_NAME),
        "{list:?}"
    );
    within(editor.create(&wire)).await.unwrap();
    assert_eq!(sync(&mut session, &cache).await.unwrap(), 2);
    let created = changes_since(&cache, &link, "1").await;
    assert_eq!(created["created"].as_array().unwrap().len(), 1, "{created}");
    assert_eq!(created["destroyed"], json!([]));
    assert_eq!(created["newState"], "2");
    let id = &created["created"][0];
    let mailbox = mailbox_list(&cache, &link)
        .await
        .into_iter()
        .find(|mailbox| mailbox["id"] == *id)
        .unwrap();
    assert_eq!(mailbox["name"], CREATED_NAME);
    assert_eq!(mailbox["myRights"]["mayReadItems"], true);
    within(editor.delete(&wire)).await.unwrap();
    assert_eq!(sync(&mut session, &cache).await.unwrap(), 3);
    let gone = changes_since(&cache, &link, "2").await;
    assert_eq!(gone["destroyed"], created["created"]);
    assert_eq!(gone["created"], json!([]));
    assert_eq!(gone["newState"], "3");
    session.logout().await.unwrap();
    within(editor.logout()).await.unwrap();
}

/// The messages a check appended, told apart by the word in their
/// Message-ID, expunged from both its folders on the second connection.
async fn swept(editor: &mut Editor, (from, to): (&str, &str), word: &str) {
    for folder in [from, to] {
        within(editor.select(folder)).await.unwrap();
        let query = format!("HEADER Message-ID <{word}-");
        let uids = within(editor.uid_search(&query)).await.unwrap();
        if uids.is_empty() {
            continue;
        }
        let set: Vec<String> = uids.iter().map(u32::to_string).collect();
        let set = set.join(",");
        for command in [
            format!("UID STORE {set} +FLAGS.SILENT (\\Deleted)"),
            format!("UID EXPUNGE {set}"),
        ] {
            within(editor.run_command_and_check_ok(&command))
                .await
                .unwrap();
        }
    }
}

/// The second connection with what an earlier run left swept away and
/// the first folder holding one message per number, the last appended
/// with the highest UID.
async fn fresh(folders: (&str, &str), word: &str, numbers: &[u32]) -> Editor {
    let mut editor = editor().await;
    swept(&mut editor, folders, word).await;
    for number in numbers {
        let message = format!(
            "From: {USER}\r\nTo: {USER}\r\nSubject: {word} {number}\r\nMessage-ID: <{word}-{number}@huliho.test>\r\n\r\nBody {number}\r\n"
        );
        within(editor.append(folders.0, None, None, message))
            .await
            .unwrap();
    }
    editor
}

/// The UID of one appended message in the folder the second connection
/// has selected.
async fn uid_of(editor: &mut Editor, word: &str, number: u32) -> u32 {
    let query = format!("HEADER Message-ID <{word}-{number}@huliho.test>");
    let uids = within(editor.uid_search(&query)).await.unwrap();
    assert_eq!(uids.len(), 1, "{query}");
    uids.into_iter().next().unwrap()
}

#[tokio::test]
async fn dovecot_moves_a_message_and_names_its_new_uid_rfc6851_3_2() {
    let _account = ACCOUNT.lock().await;
    let (from, to) = MOVE_FOLDERS;
    let mut editor = fresh(MOVE_FOLDERS, "move", &[1, 2]).await;
    let mut session = bridge_session().await;
    let target = session.status(to, VALIDITY).await.unwrap();
    assert!(!session.select(from).await.unwrap().read_only);
    let uids = session.uid_list().await.unwrap();
    let ask = MoveAsk {
        uids: &uids[..1],
        target: to,
    };
    let moved = session.uid_move(&ask).await.unwrap().unwrap();
    assert_eq!(Some(moved.uid_validity), target.uid_validity);
    assert_eq!(session.uid_list().await.unwrap(), uids[1..]);
    within(editor.select(to)).await.unwrap();
    let arrived = uid_of(&mut editor, "move", 2).await;
    assert_eq!(moved.uids, [(uids[0], arrived)]);
    session.logout().await.unwrap();
    swept(&mut editor, MOVE_FOLDERS, "move").await;
    within(editor.logout()).await.unwrap();
}

#[tokio::test]
async fn dovecot_moves_by_copy_and_uid_expunge_and_keeps_a_message_another_client_marked_rfc4315_2_1()
 {
    let _account = ACCOUNT.lock().await;
    let (from, to) = COPY_FOLDERS;
    let mut editor = fresh(COPY_FOLDERS, "copy", &[1, 2]).await;
    within(editor.select(from)).await.unwrap();
    let marked = uid_of(&mut editor, "copy", 1).await;
    let taken = uid_of(&mut editor, "copy", 2).await;
    let mark = format!("UID STORE {marked} +FLAGS.SILENT (\\Deleted)");
    within(editor.run_command_and_check_ok(&mark))
        .await
        .unwrap();
    let mut session = bridge_session().await;
    session.select(from).await.unwrap();
    let ask = MoveAsk {
        uids: &[taken],
        target: to,
    };
    let copied = session.uid_copy(&ask).await.unwrap().unwrap();
    session.uid_expunge(&[taken]).await.unwrap();
    let left = session.uid_list().await.unwrap();
    assert!(left.contains(&marked) && !left.contains(&taken), "{left:?}");
    within(editor.select(to)).await.unwrap();
    let arrived = uid_of(&mut editor, "copy", 2).await;
    assert_eq!(copied.uids, [(taken, arrived)]);
    session.logout().await.unwrap();
    swept(&mut editor, COPY_FOLDERS, "copy").await;
    within(editor.logout()).await.unwrap();
}
