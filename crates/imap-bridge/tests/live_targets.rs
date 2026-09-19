// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The checks against the compose Dovecot: the credentials, the mailbox
//! pass and the JMAP methods reading what it wrote.

#![cfg(feature = "live-targets")]

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use huliho_imap_bridge::jmap::{CORE_CAPABILITY, MAIL_CAPABILITY, handle};
use huliho_imap_bridge::mailboxes::sync;
use huliho_imap_bridge::session::{
    ImapSession, STEP_TIMEOUT, Session, SessionError, Target, TlsMode,
};
use huliho_imap_bridge::smtp;
use huliho_imap_bridge::store::{AccountKey, Store};
use huliho_imap_bridge::utf7;
use huliho_imap_bridge::verify::{Credential, VerifyError, verify};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;
use tokio_rustls::rustls::pki_types::pem::PemObject;
use tokio_rustls::rustls::pki_types::{CertificateDer, ServerName};
use tokio_rustls::rustls::{ClientConfig, RootCertStore};

/// The compose Dovecot on the host's loopback; its certificate names
/// `localhost`.
const DOVECOT_HOST: &str = "localhost";
const IMAPS_PORT: u16 = 31993;
const IMAP_PORT: u16 = 31143;
const SUBMISSION_PORT: u16 = 31587;
const USER: &str = "sanne@huliho.test";
const PASSWORD: &str = "password";

/// A mailbox with a character outside ASCII, so the UTF-7 path runs
/// live.
const CREATED_NAME: &str = "Bijlagen é";

fn dev_ca() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/dev-certs/ca.pem")
}

fn tls(trust_dev_ca: bool) -> Arc<ClientConfig> {
    let mut roots = RootCertStore::empty();
    if trust_dev_ca {
        for certificate in CertificateDer::pem_file_iter(dev_ca()).unwrap() {
            roots.add(certificate.unwrap()).unwrap();
        }
    }
    let provider = Arc::new(tokio_rustls::rustls::crypto::ring::default_provider());
    Arc::new(
        ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    )
}

fn dovecot(port: u16, tls: TlsMode) -> Target {
    Target {
        host: DOVECOT_HOST.to_owned(),
        addresses: vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)],
        tls,
    }
}

fn password(password: &str) -> Credential {
    Credential::Password {
        username: USER.to_owned(),
        password: password.to_owned(),
    }
}

/// The bridge's own session, signed in.
async fn bridge_session() -> ImapSession {
    let target = dovecot(IMAPS_PORT, TlsMode::Implicit);
    let mut session = ImapSession::connect(tls(true), &target, STEP_TIMEOUT)
        .await
        .unwrap();
    session.login(USER, PASSWORD).await.unwrap();
    session
}

/// One step of the editor under the step timeout the bridge runs by.
async fn within<T>(step: impl Future<Output = T>) -> T {
    timeout(STEP_TIMEOUT, step)
        .await
        .expect("Dovecot answers within the step timeout")
}

/// A second connection through the client library's typed commands
/// for CREATE and DELETE, which the read path never sends.
async fn editor() -> async_imap::Session<TlsStream<TcpStream>> {
    let tcp = within(TcpStream::connect((DOVECOT_HOST, IMAPS_PORT)))
        .await
        .unwrap();
    let name = ServerName::try_from(DOVECOT_HOST).unwrap();
    let stream = within(TlsConnector::from(tls(true)).connect(name, tcp))
        .await
        .unwrap();
    let mut client = async_imap::Client::new(stream);
    within(client.read_response()).await.unwrap().unwrap();
    within(client.login(USER, PASSWORD))
        .await
        .map_err(|(error, _client)| error)
        .unwrap()
}

/// Removes what an earlier run left, on a connection of its own: the
/// parser refuses Dovecot's NO for a missing name, which holds UTF-8.
async fn clear(wire: &str) {
    let mut editor = editor().await;
    if within(editor.delete(wire)).await.is_ok() {
        within(editor.logout()).await.unwrap();
    }
}

/// The arguments of the first response to one call.
fn answer(store: &Store, key: &AccountKey, call: &Value) -> Value {
    let body = json!({ "using": [CORE_CAPABILITY, MAIL_CAPABILITY], "methodCalls": [call] });
    let bytes = handle(store, key, &serde_json::to_vec(&body).unwrap()).unwrap();
    let response: Value = serde_json::from_slice(&bytes).unwrap();
    response["methodResponses"][0][1].clone()
}

fn mailbox_list(store: &Store, key: &AccountKey) -> Vec<Value> {
    let call = json!(["Mailbox/get", { "accountId": key.as_str(), "ids": null }, "c1"]);
    answer(store, key, &call)["list"]
        .as_array()
        .unwrap()
        .clone()
}

fn changes_since(store: &Store, key: &AccountKey, since: &str) -> Value {
    let call = json!([
        "Mailbox/changes",
        { "accountId": key.as_str(), "sinceState": since },
        "c1"
    ]);
    answer(store, key, &call)
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
    let store = Arc::new(Store::in_memory().unwrap());
    let key = AccountKey::new("live");
    let mut session = bridge_session().await;
    assert_eq!(
        sync(&mut session, Arc::clone(&store), key.clone())
            .await
            .unwrap(),
        1
    );
    let list = mailbox_list(&store, &key);
    assert!(
        list.iter().any(|mailbox| mailbox["role"] == "inbox"),
        "{list:?}"
    );
    assert!(
        list.iter().all(|mailbox| mailbox["name"] != CREATED_NAME),
        "{list:?}"
    );
    within(editor.create(&wire)).await.unwrap();
    assert_eq!(
        sync(&mut session, Arc::clone(&store), key.clone())
            .await
            .unwrap(),
        2
    );
    let created = changes_since(&store, &key, "1");
    assert_eq!(created["created"].as_array().unwrap().len(), 1, "{created}");
    assert_eq!(created["destroyed"], json!([]));
    assert_eq!(created["newState"], "2");
    let id = &created["created"][0];
    let mailbox = mailbox_list(&store, &key)
        .into_iter()
        .find(|mailbox| mailbox["id"] == *id)
        .unwrap();
    assert_eq!(mailbox["name"], CREATED_NAME);
    assert_eq!(mailbox["myRights"]["mayReadItems"], true);
    within(editor.delete(&wire)).await.unwrap();
    assert_eq!(
        sync(&mut session, Arc::clone(&store), key.clone())
            .await
            .unwrap(),
        3
    );
    let gone = changes_since(&store, &key, "2");
    assert_eq!(gone["destroyed"], created["created"]);
    assert_eq!(gone["created"], json!([]));
    assert_eq!(gone["newState"], "3");
    session.logout().await.unwrap();
    within(editor.logout()).await.unwrap();
}
