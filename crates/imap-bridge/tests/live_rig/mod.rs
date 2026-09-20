// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The compose Dovecot as the live tests reach it: the trust, the
//! bridge's own session, a second connection that edits the account
//! and one JMAP call against a store.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use huliho_imap_bridge::jmap::{CORE_CAPABILITY, HULIHO_CAPABILITY, MAIL_CAPABILITY, handle};
use huliho_imap_bridge::session::{ImapSession, STEP_TIMEOUT, Session, Target, TlsMode};
use huliho_imap_bridge::store::{AccountKey, Store};
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

/// The second connection: the client library's typed commands, which
/// the read path never sends.
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
    let target = dovecot(IMAPS_PORT, TlsMode::Implicit);
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

/// A second connection through the client library's typed commands
/// for CREATE and DELETE, which the read path never sends.
pub async fn editor() -> Editor {
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

/// The arguments of the first response to one call.
pub fn answer(store: &Store, key: &AccountKey, call: &Value) -> Value {
    let using = [CORE_CAPABILITY, MAIL_CAPABILITY, HULIHO_CAPABILITY];
    let body = json!({ "using": using, "methodCalls": [call] });
    let bytes = handle(
        store,
        &TestSealer::default(),
        key,
        &serde_json::to_vec(&body).unwrap(),
    )
    .unwrap();
    let response: Value = serde_json::from_slice(&bytes).unwrap();
    response["methodResponses"][0][1].clone()
}

pub fn mailbox_list(store: &Store, key: &AccountKey) -> Vec<Value> {
    let call = json!(["Mailbox/get", { "accountId": key.as_str(), "ids": null }, "c1"]);
    answer(store, key, &call)["list"]
        .as_array()
        .unwrap()
        .clone()
}
