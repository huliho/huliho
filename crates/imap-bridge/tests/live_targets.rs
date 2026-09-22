// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The checks against the compose Dovecot: the credentials, the mailbox
//! pass and the JMAP methods reading what it wrote.

#![cfg(feature = "live-targets")]

mod live_rig;

use huliho_imap_bridge::mailboxes::sync;
use huliho_imap_bridge::runtime::Link;
use huliho_imap_bridge::session::{STEP_TIMEOUT, Session, SessionError, TlsMode};
use huliho_imap_bridge::smtp;
use huliho_imap_bridge::sync::Cache;
use huliho_imap_bridge::testing::TestConnector;
use huliho_imap_bridge::utf7;
use huliho_imap_bridge::verify::{Credential, VerifyError, verify};
use live_rig::{
    IMAPS_PORT, PASSWORD, USER, answer, bridge_session, cache, dovecot, editor, link, mailbox_list,
    tls, within,
};
use serde_json::{Value, json};

const IMAP_PORT: u16 = 31143;
const SUBMISSION_PORT: u16 = 31587;

/// A mailbox with a character outside ASCII, so the UTF-7 path runs
/// live.
const CREATED_NAME: &str = "Bijlagen é";

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
