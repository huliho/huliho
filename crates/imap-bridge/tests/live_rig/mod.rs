// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The compose Dovecot as the live tests reach it: the trust, the
//! bridge's own session, a second connection that edits the account
//! and one JMAP call against a store. The compose Cyrus answers on its
//! IMAPS port the same way.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use huliho_imap_bridge::jmap::{CORE_CAPABILITY, HULIHO_CAPABILITY, MAIL_CAPABILITY, handle};
use huliho_imap_bridge::runtime::Link;
use huliho_imap_bridge::session::{ImapSession, STEP_TIMEOUT, Session, Target, TlsMode};
use huliho_imap_bridge::store::{AccountKey, Store};
use huliho_imap_bridge::sync::Cache;
use huliho_imap_bridge::testing::TestConnector;
use huliho_imap_bridge::testing::seal::TestSealer;
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
pub const IMAPS_PORT: u16 = 31993;
pub const USER: &str = "sanne@huliho.test";
pub const PASSWORD: &str = "password";

/// The second connection: a session of the client library, driven by
/// its typed commands and by raw command lines.
pub type Editor = async_imap::Session<TlsStream<TcpStream>>;

fn dev_ca() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/dev-certs/ca.pem")
}

pub fn tls(trust_dev_ca: bool) -> Arc<ClientConfig> {
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

pub fn dovecot(port: u16, tls: TlsMode) -> Target {
    Target {
        host: DOVECOT_HOST.to_owned(),
        addresses: vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)],
        tls,
    }
}

/// The bridge's own session, signed in.
pub async fn bridge_session() -> ImapSession {
    bridge_session_on(IMAPS_PORT).await
}

/// The same on another IMAPS port of the host's loopback: the compose
/// Cyrus takes the same user under the same certificate authority.
pub async fn bridge_session_on(port: u16) -> ImapSession {
    let target = dovecot(port, TlsMode::Implicit);
    let mut session = ImapSession::connect(tls(true), &target, STEP_TIMEOUT)
        .await
        .unwrap();
    session.login(USER, PASSWORD).await.unwrap();
    session
}

/// One step of the editor under the step timeout the bridge runs by.
pub async fn within<T>(step: impl Future<Output = T>) -> T {
    timeout(STEP_TIMEOUT, step)
        .await
        .expect("Dovecot answers within the step timeout")
}

/// A second connection that edits the account as another client would:
/// it creates and deletes folders, appends, searches, marks and
/// expunges messages.
pub async fn editor() -> Editor {
    editor_on(IMAPS_PORT).await
}

/// The same on another IMAPS port, as [`bridge_session_on`] has it.
pub async fn editor_on(port: u16) -> Editor {
    let tcp = within(TcpStream::connect((DOVECOT_HOST, port)))
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

/// Appends a message on the editor as a non-synchronizing literal (RFC
/// 7888), with the flag list or the date between the name and the
/// literal as written (RFC 3501 section 6.3.11). The client library's
/// own APPEND fails on any line ahead of the continuation request, such
/// as the untagged NO Dovecot may send there (RFC 3501 section 7).
pub async fn append(editor: &mut Editor, mailbox: &str, written: Option<&str>, message: &str) {
    let written = written
        .map(|written| format!(" {written}"))
        .unwrap_or_default();
    let size = message.len();
    let command = format!("APPEND \"{mailbox}\"{written} {{{size}+}}\r\n{message}");
    within(editor.run_command_and_check_ok(&command))
        .await
        .unwrap();
}

/// A cache over a fresh store under the test sealer.
pub fn cache(key: &str) -> Cache {
    Cache {
        store: Arc::new(Store::in_memory().unwrap()),
        sealer: Arc::new(TestSealer::default()),
        key: AccountKey::new(key),
        gmail: false,
    }
}

/// A link to the compose Dovecot that refreshes on every `/changes`
/// call.
pub fn link() -> Link<TestConnector> {
    let connector = TestConnector::Server {
        tls: tls(true),
        target: dovecot(IMAPS_PORT, TlsMode::Implicit),
        step: STEP_TIMEOUT,
        user: USER.to_owned(),
        password: PASSWORD.to_owned(),
    };
    Link::with_interval(connector, std::time::Duration::ZERO)
}

/// The arguments of the first response to one call.
pub async fn answer(cache: &Cache, link: &Link<TestConnector>, call: &Value) -> Value {
    let using = [CORE_CAPABILITY, MAIL_CAPABILITY, HULIHO_CAPABILITY];
    let body = json!({ "using": using, "methodCalls": [call] });
    let bytes = handle(cache, link, &serde_json::to_vec(&body).unwrap(), "live")
        .await
        .unwrap();
    let response: Value = serde_json::from_slice(&bytes).unwrap();
    response["methodResponses"][0][1].clone()
}

pub async fn mailbox_list(cache: &Cache, link: &Link<TestConnector>) -> Vec<Value> {
    let key = cache.key.as_str();
    let call = json!(["Mailbox/get", { "accountId": key, "ids": null }, "c1"]);
    answer(cache, link, &call).await["list"]
        .as_array()
        .unwrap()
        .clone()
}
