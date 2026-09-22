// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The live Gmail test account: named by the environment, reached over
//! the public roots and edited on a connection of its own, as a second
//! client would.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use huliho_imap_bridge::session::{
    ImapSession, ListReturn, STEP_TIMEOUT, Session, Target, TlsMode,
};
use tokio::net::TcpStream;
use tokio::time::{sleep, timeout};
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::rustls::{ClientConfig, RootCertStore};

use super::Connection;
use super::corpus::{Change, LABEL, MARKER, seed, seeds};

/// The variable that names the account.
pub const ADDRESS: &str = "HULIHO_LIVE_GMAIL_ADDRESS";

/// The variable that holds its app password.
pub const APP_PASSWORD: &str = "HULIHO_LIVE_GMAIL_APP_PASSWORD";

/// The variable that names the file a run writes the transcript to.
pub const RECORD: &str = "HULIHO_RECORD_TRANSCRIPT";

const HOST: &str = "imap.gmail.com";
const PORT: u16 = 993;

/// Gmail settles an edit made on one connection before another sees it;
/// this is the wait after the edit and between two looks for it.
pub const SETTLE: Duration = Duration::from_secs(2);

/// The folder a delivery lands in.
const INBOX: &str = "INBOX";

/// The second connection: the client library's typed commands, which
/// the read path never sends.
pub type Editor = async_imap::Session<TlsStream<TcpStream>>;

/// The account as the environment names it.
pub struct Account {
    address: String,
    password: String,
}

/// The wire names of the three stores as this account spells them.
pub struct Stores {
    pub all_mail: String,
    pub spam: String,
    pub trash: String,
}

impl Account {
    /// The account, when both variables are set.
    pub fn from_env() -> Option<Self> {
        let address = std::env::var(ADDRESS)
            .ok()
            .filter(|value| !value.is_empty())?;
        let password = std::env::var(APP_PASSWORD)
            .ok()
            .filter(|value| !value.is_empty())?;
        Some(Self { address, password })
    }

    /// Where a run writes the transcript, when asked to.
    pub fn record_path() -> Option<PathBuf> {
        std::env::var_os(RECORD).map(PathBuf::from)
    }

    /// How the bridge reaches the account.
    pub async fn connection(&self) -> Connection {
        let addresses = tokio::net::lookup_host((HOST, PORT))
            .await
            .unwrap()
            .collect();
        Connection {
            tls: roots(),
            target: Target {
                host: HOST.to_owned(),
                addresses,
                tls: TlsMode::Implicit,
            },
            user: self.address.clone(),
            password: self.password.clone(),
            step: STEP_TIMEOUT,
        }
    }

    /// The second connection.
    pub async fn editor(&self) -> Editor {
        let tcp = within(TcpStream::connect((HOST, PORT))).await.unwrap();
        let name = ServerName::try_from(HOST).unwrap();
        let stream = within(TlsConnector::from(roots()).connect(name, tcp))
            .await
            .unwrap();
        let mut client = async_imap::Client::new(stream);
        within(client.read_response()).await.unwrap().unwrap();
        within(client.login(&self.address, &self.password))
            .await
            .map_err(|(error, _client)| error)
            .unwrap()
    }
}

/// The public roots, as the server trusts them.
fn roots() -> Arc<ClientConfig> {
    let roots = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let provider = Arc::new(tokio_rustls::rustls::crypto::ring::default_provider());
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
        .expect("Gmail answers within the step timeout")
}

/// A name as a quoted string (RFC 9051 section 4.3).
fn quoted(name: &str) -> String {
    format!("\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\""))
}

impl Stores {
    /// The stores by their attributes, read on a session of the
    /// bridge's own that no recorder sees.
    pub async fn discover(connection: &Connection) -> Self {
        let mut session = ImapSession::connect(
            Arc::clone(&connection.tls),
            &connection.target,
            connection.step,
        )
        .await
        .unwrap();
        session
            .login(&connection.user, &connection.password)
            .await
            .unwrap();
        let listing = session.list(ListReturn::default()).await.unwrap();
        session.logout().await.unwrap();
        let named = |attribute: &str| {
            listing
                .entries
                .iter()
                .find(|entry| entry.attributes.iter().any(|found| found == attribute))
                .unwrap_or_else(|| panic!("no folder carries {attribute}"))
                .name
                .clone()
        };
        Self {
            all_mail: named("\\ALL"),
            spam: named("\\JUNK"),
            trash: named("\\TRASH"),
        }
    }
}

/// Removes what an earlier run left: every message with the marker in
/// the three stores goes to the trash and leaves it, and the label goes.
pub async fn clear(editor: &mut Editor, stores: &Stores) {
    for store in [&stores.all_mail, &stores.spam] {
        let uids = marked(editor, store).await;
        if !uids.is_empty() {
            let command = format!("UID MOVE {} {}", uids.join(","), quoted(&stores.trash));
            within(editor.run_command_and_check_ok(&command))
                .await
                .unwrap();
        }
    }
    let uids = marked(editor, &stores.trash).await;
    if !uids.is_empty() {
        let command = format!("UID STORE {} +FLAGS.SILENT (\\Deleted)", uids.join(","));
        within(editor.run_command_and_check_ok(&command))
            .await
            .unwrap();
        within(editor.run_command_and_check_ok("EXPUNGE"))
            .await
            .unwrap();
    }
    // A label that is not there answers NO, which is the state wanted.
    let _ = within(editor.delete(LABEL)).await;
}

/// The UIDs in `store` that carry the marker; the store stays selected.
async fn marked(editor: &mut Editor, store: &str) -> Vec<String> {
    within(editor.select(store)).await.unwrap();
    let query = format!("HEADER {} {}", MARKER.0, MARKER.1);
    let mut uids: Vec<u32> = within(editor.uid_search(&query))
        .await
        .unwrap()
        .into_iter()
        .collect();
    uids.sort_unstable();
    uids.iter().map(u32::to_string).collect()
}

/// Puts the corpus in the account: the label and the five messages in
/// the inbox with their dates and flags.
pub async fn seed_account(editor: &mut Editor) {
    within(editor.create(LABEL)).await.unwrap();
    for message in seeds() {
        deliver(editor, message.number).await;
    }
}

async fn deliver(editor: &mut Editor, number: u32) {
    let message = seed(number);
    let flags = message.seen.then_some("(\\Seen)");
    let date = quoted(&message.internal_date());
    within(editor.append(INBOX, flags, Some(&date), message.rfc5322()))
        .await
        .unwrap();
}

/// The UID of the message with that number in the selected store.
async fn uid_of(editor: &mut Editor, number: u32) -> u32 {
    let query = format!("HEADER Message-ID {}", seed(number).message_id());
    let uids = within(editor.uid_search(&query)).await.unwrap();
    assert_eq!(uids.len(), 1, "{query}");
    uids.into_iter().next().unwrap()
}

/// One edit as a second client makes it, then the wait for Gmail to
/// settle.
pub async fn apply(editor: &mut Editor, stores: &Stores, change: Change) {
    match change {
        Change::Deliver(number) => deliver(editor, number).await,
        Change::Flag(number, flag) => {
            within(editor.select(&stores.all_mail)).await.unwrap();
            let uid = uid_of(editor, number).await;
            let command = format!("UID STORE {uid} +FLAGS.SILENT ({flag})");
            within(editor.run_command_and_check_ok(&command))
                .await
                .unwrap();
        }
        Change::Label(number) => {
            within(editor.select(&stores.all_mail)).await.unwrap();
            let uid = uid_of(editor, number).await;
            let command = format!("UID STORE {uid} +X-GM-LABELS ({})", quoted(LABEL));
            within(editor.run_command_and_check_ok(&command))
                .await
                .unwrap();
        }
        Change::MoveToSpam(number) => {
            within(editor.select(&stores.all_mail)).await.unwrap();
            let uid = uid_of(editor, number).await;
            let command = format!("UID MOVE {uid} {}", quoted(&stores.spam));
            within(editor.run_command_and_check_ok(&command))
                .await
                .unwrap();
        }
        Change::DeleteForever(number) => {
            within(editor.select(&stores.all_mail)).await.unwrap();
            let uid = uid_of(editor, number).await;
            let command = format!("UID MOVE {uid} {}", quoted(&stores.trash));
            within(editor.run_command_and_check_ok(&command))
                .await
                .unwrap();
            within(editor.select(&stores.trash)).await.unwrap();
            let uid = uid_of(editor, number).await;
            let command = format!("UID STORE {uid} +FLAGS.SILENT (\\Deleted)");
            within(editor.run_command_and_check_ok(&command))
                .await
                .unwrap();
            within(editor.run_command_and_check_ok("EXPUNGE"))
                .await
                .unwrap();
        }
    }
    sleep(SETTLE).await;
}
