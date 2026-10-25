// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The checks against the compose Dovecot: the credentials, the mailbox
//! pass, the JMAP methods reading what it wrote, the session's moves and
//! its search.

#![cfg(feature = "live-targets")]

mod live_rig;

use std::fmt::Write as _;

use huliho_imap_bridge::mailboxes::sync;
use huliho_imap_bridge::runtime::Link;
use huliho_imap_bridge::session::{
    Literals, MoveAsk, STEP_TIMEOUT, SearchAsk, Searching, Session, SessionError, StatusItems,
    TlsMode,
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

/// The folder the search check seeds; no other check works in it.
const SEARCH_FOLDERS: (&str, &str) = ("Junk", "Junk");

/// A word outside ASCII two seeded bodies carry and the sender two of
/// the messages come from.
const SEARCH_WORD: &str = "zoëquarter";
const SEARCH_SENDER: &str = "anna.search@huliho.test";

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

/// The second connection with the three messages of the search check in
/// its folder: the word from the sender, the word alone and the sender
/// alone.
async fn seeded() -> Editor {
    let mut editor = editor().await;
    swept(&mut editor, SEARCH_FOLDERS, "search").await;
    // No folder stays selected while the messages go in: the client
    // library reads any line ahead of the continuation request as a
    // failed append.
    within(editor.close()).await.unwrap();
    let bodies = [
        (1, SEARCH_SENDER, format!("The {SEARCH_WORD} begins.")),
        (2, USER, format!("The {SEARCH_WORD} ends.")),
        (3, SEARCH_SENDER, "Nothing to report.".to_owned()),
    ];
    for (number, from, body) in bodies {
        let message = format!(
            "From: {from}\r\nTo: {USER}\r\nSubject: search {number}\r\nMessage-ID: <search-{number}@huliho.test>\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: 8bit\r\n\r\n{body}\r\n"
        );
        within(editor.append(SEARCH_FOLDERS.0, None, None, message))
            .await
            .unwrap();
    }
    editor
}

/// What the second connection's own UID SEARCH answers for the text and
/// the sender, highest first: the word as a literal, the sender quoted.
async fn searched_by_hand(editor: &mut Editor, text: &str, from: &str) -> Vec<u32> {
    let mut query = String::from("CHARSET UTF-8");
    if !text.is_empty() {
        let _ = write!(query, " TEXT {{{}+}}\r\n{text}", text.len());
    }
    if !from.is_empty() {
        let _ = write!(query, " FROM \"{from}\"");
    }
    let mut uids: Vec<u32> = within(editor.uid_search(&query))
        .await
        .unwrap()
        .into_iter()
        .collect();
    uids.sort_unstable_by(|a, b| b.cmp(a));
    uids
}

#[tokio::test]
async fn dovecot_searches_a_word_and_a_sender_in_both_literal_forms_as_a_second_client_does_rfc3501_6_4_4()
 {
    let _account = ACCOUNT.lock().await;
    let mut editor = seeded().await;
    within(editor.select(SEARCH_FOLDERS.0)).await.unwrap();
    let mut seeded_uids = Vec::new();
    for number in 1..=3 {
        seeded_uids.push(uid_of(&mut editor, "search", number).await);
    }
    let mut session = bridge_session().await;
    let searching = session.capabilities().await.unwrap().searching();
    assert_eq!(searching.literals, Literals::Plus);
    session.examine(SEARCH_FOLDERS.0).await.unwrap();
    let sender = "anna.search";
    let cases: [(&str, &str, &[usize]); 3] = [
        (SEARCH_WORD, "", &[1, 2]),
        ("", sender, &[1, 3]),
        (SEARCH_WORD, sender, &[1]),
    ];
    for literals in [Literals::Plus, Literals::Synchronizing] {
        for (text, from, numbers) in cases {
            let mut wanted: Vec<u32> = numbers.iter().map(|n| seeded_uids[n - 1]).collect();
            wanted.sort_unstable_by(|a, b| b.cmp(a));
            let ask = SearchAsk {
                text,
                from,
                unseen: false,
                flagged: false,
                since: None,
                before: None,
                gmail: None,
                server: Searching {
                    literals,
                    ..searching
                },
            };
            let found = session.uid_search(&ask).await.unwrap();
            assert_eq!(found, wanted, "{literals:?} {text:?} {from:?}");
            let own = searched_by_hand(&mut editor, text, from).await;
            assert_eq!(own, wanted, "{text:?} {from:?}");
        }
    }
    session.logout().await.unwrap();
    swept(&mut editor, SEARCH_FOLDERS, "search").await;
    within(editor.logout()).await.unwrap();
}
