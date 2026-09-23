// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The compose targets as the suite reaches them through the router:
//! an instance on the compose rules with the bridge on quick clocks, one
//! account per target and the JMAP calls a step makes. The second
//! connection that edits the mailbox is `second`.

pub mod corpus;
mod second;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Body;
use axum::http::{Method, StatusCode, header};
use huliho_imap_bridge::jmap::{CORE_CAPABILITY, HULIHO_CAPABILITY, MAIL_CAPABILITY};
use huliho_imap_bridge::runtime::Timing;
use huliho_server::api::ApiState;
use huliho_server::bridge;
use huliho_server::config::UpstreamConfig;
use huliho_server::upstream::Upstream;
use serde_json::{Value, json};
use tokio::time::sleep;

pub use second::{Editor, clear, editor, expunge, flag, seed, within};

use crate::answers::answer;
use crate::common::{api_state, router_on, router_with};
use crate::fake_dns::FakeDns;
use crate::signin::{sign_in, store_with_account, with_cookie};

/// The compose targets on the host's loopback; both certificates name
/// `localhost`.
const HOST: &str = "localhost";
const CYRUS_JMAP_PORT: u16 = 8443;
const CYRUS_IMAPS_PORT: u16 = 9993;
const DOVECOT_IMAPS_PORT: u16 = 31993;
const DOVECOT_SUBMISSION_PORT: u16 = 31587;

/// The one user of both targets.
const MAIL_ADDRESS: &str = "sanne@huliho.test";
const MAIL_PASSWORD: &str = "password";

/// The variable that sizes the latency corpus and its default.
pub const CORPUS_VARIABLE: &str = "HULIHO_LIVE_CORPUS";
const DEFAULT_CORPUS: u32 = 500;

/// How long one look waits for the bridge to notice a change; its
/// refresh is due after as long.
pub const LOOK: Duration = Duration::from_secs(1);

/// How many looks a change from the second connection gets.
pub const LOOKS: usize = 30;

/// How many looks after a failed round the bridge tries again.
const RETRY_LOOKS: u32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Cyrus,
    Dovecot,
}

impl Target {
    fn imaps_port(self) -> u16 {
        match self {
            Self::Cyrus => CYRUS_IMAPS_PORT,
            Self::Dovecot => DOVECOT_IMAPS_PORT,
        }
    }

    /// Whether requests answer from the bridge's cache, which the sync
    /// fills over time.
    pub fn is_bridge(self) -> bool {
        self == Self::Dovecot
    }

    /// The body `POST /api/accounts` takes for the fixture user's
    /// account on the target.
    pub fn body(self) -> Value {
        let target = match self {
            Self::Cyrus => json!({
                "kind": "jmap",
                "sessionUrl": format!("https://{HOST}:{CYRUS_JMAP_PORT}/jmap"),
            }),
            Self::Dovecot => json!({
                "kind": "imap",
                "username": MAIL_ADDRESS,
                "imap": { "host": HOST, "port": DOVECOT_IMAPS_PORT, "tls": "implicit" },
                "smtp": { "host": HOST, "port": DOVECOT_SUBMISSION_PORT, "tls": "starttls" },
            }),
        };
        json!({
            "address": MAIL_ADDRESS,
            "provider": "generic",
            "target": target,
            "credential": { "kind": "password", "password": MAIL_PASSWORD },
        })
    }
}

/// An account through the router: the route's id, the id JMAP calls
/// name and whether the vendor capability may be asked for.
pub struct Mail {
    pub id: String,
    pub account: String,
    vendor: bool,
}

/// The instance: the router and the fixture user's cookie.
pub struct Live {
    pub router: Router,
    pub cookie: String,
}

impl Live {
    /// On the compose rules: the dev CA trusted, the loopback allowed,
    /// `localhost` answered by the fake resolver and the bridge on quick
    /// clocks, so a change from the second connection shows within a
    /// few looks.
    pub async fn start() -> Self {
        let api = ApiState {
            upstream: Arc::new(compose_upstream()),
            ..api_state(store_with_account())
        };
        let quick = Timing {
            interval: LOOK,
            retry_interval: LOOK * RETRY_LOOKS,
            ..Timing::default()
        };
        assert!(
            api.bridge
                .set(bridge::open_with_timing(&api, quick))
                .is_ok()
        );
        let router = router_with(api);
        let cookie = sign_in(&router).await;
        Self { router, cookie }
    }

    /// On the default rules and the runtime's own clocks: the public
    /// resolver and the public roots.
    pub async fn public() -> Self {
        let router = router_on(store_with_account());
        let cookie = sign_in(&router).await;
        Self { router, cookie }
    }

    /// An account added through the router with this body; the bridge
    /// serves it when `vendor` holds.
    pub async fn add(&self, body: &Value, vendor: bool) -> Mail {
        let (status, row) = self.post("/api/accounts", body).await;
        assert_eq!(status, StatusCode::CREATED, "{row}");
        let id = row["id"].as_str().unwrap().to_owned();
        let request = with_cookie(
            Method::GET,
            &format!("/api/jmap/{id}/session"),
            &self.cookie,
        );
        let (status, session) = answer(&self.router, request).await;
        assert_eq!(status, StatusCode::OK, "{session}");
        let account = session["primaryAccounts"][MAIL_CAPABILITY]
            .as_str()
            .unwrap()
            .to_owned();
        Mail {
            id,
            account,
            vendor,
        }
    }

    pub async fn remove(&self, mail: &Mail) {
        let request = with_cookie(
            Method::DELETE,
            &format!("/api/accounts/{}", mail.id),
            &self.cookie,
        );
        let (status, body) = answer(&self.router, request).await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    }

    async fn post(&self, uri: &str, body: &Value) -> (StatusCode, Value) {
        let mut request = with_cookie(Method::POST, uri, &self.cookie);
        request
            .headers_mut()
            .insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
        *request.body_mut() = Body::from(body.to_string());
        answer(&self.router, request).await
    }

    /// The arguments of the first response to one call; a method error
    /// fails the test.
    pub async fn call(&self, mail: &Mail, method: &str, arguments: Value) -> Value {
        let mut using = vec![CORE_CAPABILITY, MAIL_CAPABILITY];
        if mail.vendor {
            using.push(HULIHO_CAPABILITY);
        }
        let body = json!({ "using": using, "methodCalls": [[method, arguments, "c1"]] });
        let (status, mut answer) = self.post(&format!("/api/jmap/{}", mail.id), &body).await;
        assert_eq!(status, StatusCode::OK, "{answer}");
        let mut response = answer["methodResponses"][0].take();
        assert_eq!(response[0], method, "{response}");
        response[1].take()
    }

    pub async fn mailboxes(&self, mail: &Mail) -> Vec<Value> {
        let arguments = json!({ "accountId": mail.account, "ids": null });
        self.call(mail, "Mailbox/get", arguments).await["list"]
            .as_array()
            .unwrap()
            .clone()
    }

    /// The mailbox list once the account shows one: a proxied account at
    /// once, the bridge after its first pass.
    pub async fn wait_listed(&self, mail: &Mail, patience: Duration) -> Vec<Value> {
        let until = Instant::now() + patience;
        loop {
            let mailboxes = self.mailboxes(mail).await;
            if !mailboxes.is_empty() {
                return mailboxes;
            }
            assert!(Instant::now() < until, "the account lists no mailbox");
            sleep(LOOK).await;
        }
    }

    /// Waits until the mailbox with that role holds every message the
    /// server has, then answers it; a proxied account has them at once.
    pub async fn wait_synced(&self, mail: &Mail, role: &str, patience: Duration) -> Value {
        let until = Instant::now() + patience;
        loop {
            let found = self
                .mailboxes(mail)
                .await
                .into_iter()
                .find(|mailbox| mailbox["role"] == role);
            if let Some(mailbox) = &found {
                let total = mailbox["totalEmails"].as_u64();
                let synced = mailbox["syncedEmails"].as_u64();
                if total.is_some() && (!mail.vendor || (synced == total && synced > Some(0))) {
                    return found.unwrap_or_default();
                }
            }
            assert!(Instant::now() < until, "{role} is not synced: {found:?}");
            sleep(LOOK).await;
        }
    }

    /// `Email/get` of these ids with the named properties.
    pub async fn emails(&self, mail: &Mail, ids: &[String], properties: &[&str]) -> Vec<Value> {
        let arguments = json!({ "accountId": mail.account, "ids": ids, "properties": properties });
        self.call(mail, "Email/get", arguments).await["list"]
            .as_array()
            .unwrap()
            .clone()
    }

    /// The state every `/changes` call since now is measured from.
    pub async fn state(&self, mail: &Mail) -> String {
        let arguments = json!({ "accountId": mail.account, "ids": [] });
        self.call(mail, "Email/get", arguments).await["state"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    /// A window of a mailbox by `receivedAt`, newest first.
    pub async fn query(&self, mail: &Mail, window: Window<'_>) -> Value {
        let arguments = json!({
            "accountId": mail.account,
            "filter": { "inMailbox": window.mailbox },
            "sort": [{ "property": "receivedAt", "isAscending": false }],
            "position": window.position,
            "limit": window.limit,
            "calculateTotal": window.total,
            "collapseThreads": window.collapse,
        });
        self.call(mail, "Email/query", arguments).await
    }

    /// `Email/changes` since a state, asked again a look apart until the
    /// answer shows what the second connection did or the looks run out.
    pub async fn changes_showing(
        &self,
        mail: &Mail,
        since: &str,
        shows: impl Fn(&Value) -> bool,
    ) -> Value {
        let arguments = json!({ "accountId": mail.account, "sinceState": since });
        let mut changed = self.call(mail, "Email/changes", arguments.clone()).await;
        for _ in 1..LOOKS {
            if shows(&changed) {
                break;
            }
            sleep(LOOK).await;
            changed = self.call(mail, "Email/changes", arguments.clone()).await;
        }
        changed
    }
}

/// What a window asks for; the total costs one walk over the mailbox,
/// so a client asks it once per list.
#[derive(Clone, Copy)]
pub struct Window<'a> {
    pub mailbox: &'a str,
    pub position: usize,
    pub limit: usize,
    pub collapse: bool,
    pub total: bool,
}

fn dev_ca() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/dev-certs/ca.pem")
}

/// The router's rules for the compose targets; port 0 leaves the port
/// to the account's own endpoint.
fn compose_upstream() -> Upstream {
    let mut dns = FakeDns::default();
    let loopback = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
    dns.addresses.insert(HOST.to_owned(), vec![loopback]);
    let config = UpstreamConfig {
        allow_private_networks: vec!["127.0.0.0/8".parse().unwrap()],
        additional_ca_file: Some(dev_ca()),
        ..UpstreamConfig::default()
    };
    Upstream::with_dns(&config, Arc::new(dns)).unwrap()
}

/// The corpus size the variable names, five hundred without it.
pub fn corpus_size() -> u32 {
    std::env::var(CORPUS_VARIABLE)
        .ok()
        .map_or(DEFAULT_CORPUS, |value| {
            value.parse().expect("a number of messages")
        })
}

/// The corpus number of an email from its `messageId`; `None` for any
/// other mail.
pub fn number_of(email: &Value) -> Option<u32> {
    corpus::number_of(email["messageId"][0].as_str()?)
}

/// The ids a `/changes` or `/query` answer lists under that name.
pub fn listed(answer: &Value, list: &str) -> Vec<String> {
    answer[list]
        .as_array()
        .unwrap_or_else(|| panic!("no {list} in {answer}"))
        .iter()
        .map(|id| id.as_str().unwrap().to_owned())
        .collect()
}
