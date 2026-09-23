// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The second connection to a target: the client library's typed
//! commands, which the read path never sends, seeding the corpus and
//! editing it the way another client would.

use std::fmt::Write as _;
use std::sync::Arc;

use huliho_imap_bridge::session::STEP_TIMEOUT;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConfig, RootCertStore};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;

use super::corpus::{MARKER, Seed};
use super::{HOST, MAIL_ADDRESS, MAIL_PASSWORD, Target, dev_ca};

/// The folder the corpus lives in.
const INBOX: &str = "INBOX";

pub type Editor = async_imap::Session<TlsStream<TcpStream>>;

/// The dev CA as the editor trusts it.
fn trust() -> Arc<ClientConfig> {
    let mut roots = RootCertStore::empty();
    for certificate in CertificateDer::pem_file_iter(dev_ca()).unwrap() {
        roots.add(certificate.unwrap()).unwrap();
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    Arc::new(
        ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    )
}

/// One step of the editor under the step timeout the bridge runs by.
pub async fn within<T>(step: impl Future<Output = T>) -> T {
    timeout(STEP_TIMEOUT, step)
        .await
        .expect("the target answers within the step timeout")
}

/// The second connection to the target, signed in.
pub async fn editor(target: Target) -> Editor {
    let tcp = within(TcpStream::connect((HOST, target.imaps_port())))
        .await
        .unwrap();
    let name = ServerName::try_from(HOST).unwrap();
    let stream = within(TlsConnector::from(trust()).connect(name, tcp))
        .await
        .unwrap();
    let mut client = async_imap::Client::new(stream);
    within(client.read_response()).await.unwrap().unwrap();
    within(client.login(MAIL_ADDRESS, MAIL_PASSWORD))
        .await
        .map_err(|(error, _client)| error)
        .unwrap()
}

/// UIDs as one sequence set, runs folded into ranges (RFC 3501
/// section 9), so a large corpus fits one command line.
fn sequence_set(mut uids: Vec<u32>) -> String {
    uids.sort_unstable();
    let mut set = String::new();
    let mut index = 0;
    while index < uids.len() {
        let start = uids[index];
        let mut end = start;
        while uids.get(index + 1) == Some(&(end + 1)) {
            index += 1;
            end = uids[index];
        }
        if !set.is_empty() {
            set.push(',');
        }
        if start == end {
            let _ = write!(set, "{start}");
        } else {
            let _ = write!(set, "{start}:{end}");
        }
        index += 1;
    }
    set
}

/// Expunges every corpus message the target holds, so the inbox is left
/// as it was found; the inbox stays selected.
pub async fn clear(editor: &mut Editor) {
    within(editor.select(INBOX)).await.unwrap();
    let query = format!("HEADER {} {}", MARKER.0, MARKER.1);
    let uids: Vec<u32> = within(editor.uid_search(&query))
        .await
        .unwrap()
        .into_iter()
        .collect();
    if uids.is_empty() {
        return;
    }
    let set = sequence_set(uids);
    within(editor.run_command_and_check_ok(&format!("UID STORE {set} +FLAGS.SILENT (\\Deleted)")))
        .await
        .unwrap();
    within(editor.run_command_and_check_ok(&format!("UID EXPUNGE {set}")))
        .await
        .unwrap();
}

/// Appends the corpus messages `from..=through` to the inbox.
pub async fn seed(editor: &mut Editor, from: u32, through: u32) {
    for number in from..=through {
        let message = Seed(number);
        let date = format!("\"{}\"", message.internal_date());
        within(editor.append(INBOX, None, Some(&date), message.rfc5322()))
            .await
            .unwrap();
    }
}

/// The UID of the corpus message with that number; the inbox stays
/// selected.
async fn uid_of(editor: &mut Editor, number: u32) -> u32 {
    within(editor.select(INBOX)).await.unwrap();
    let query = format!("HEADER Message-ID \"{}\"", Seed(number).message_id());
    let uids = within(editor.uid_search(&query)).await.unwrap();
    assert_eq!(uids.len(), 1, "{query}");
    uids.into_iter().next().unwrap()
}

/// Adds the flag to the corpus message.
pub async fn flag(editor: &mut Editor, number: u32, flag: &str) {
    let uid = uid_of(editor, number).await;
    within(editor.run_command_and_check_ok(&format!("UID STORE {uid} +FLAGS.SILENT ({flag})")))
        .await
        .unwrap();
}

/// Expunges the corpus message (RFC 4315 section 2.1).
pub async fn expunge(editor: &mut Editor, number: u32) {
    let uid = uid_of(editor, number).await;
    within(editor.run_command_and_check_ok(&format!("UID STORE {uid} +FLAGS.SILENT (\\Deleted)")))
        .await
        .unwrap();
    within(editor.run_command_and_check_ok(&format!("UID EXPUNGE {uid}")))
        .await
        .unwrap();
}
