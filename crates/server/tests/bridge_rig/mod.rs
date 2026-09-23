// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Fixtures for the bridge routes: an instance over the scripted IMAP
//! server, an IMAP account of the fixture user and the requests a test
//! sends through the router.

use std::io::Write;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, StatusCode, header};
use huliho_imap_bridge::jmap::{CORE_CAPABILITY, HULIHO_CAPABILITY, MAIL_CAPABILITY};
use huliho_imap_bridge::store::AccountKey;
use huliho_imap_bridge::testing::imap::{FakeImap, HOST, Script};
use huliho_imap_bridge::testing::{Mailboxes, USER};
use huliho_server::accounts::{
    self, AccountSettings, Credential, Endpoint, NewAccount, Provider, TlsMode,
};
use huliho_server::api::ApiState;
use huliho_server::auth::{self, LoginOutcome};
use huliho_server::config::UpstreamConfig;
use huliho_server::gate::{Gate, RUN_WINDOW};
use huliho_server::identity;
use huliho_server::ids::{AccountId, UserId};
use huliho_server::scope::{self, Scope};
use huliho_server::store::Store;
use huliho_server::upstream::Upstream;
use serde_json::{Value, json};
use tempfile::{NamedTempFile, TempDir};
use tokio::time::sleep;
use tower::ServiceExt;

use crate::answers::answer;
use crate::common::{api_state, router_on, router_with};
use crate::fake_dns::FakeDns;
use crate::readers;
use crate::signin::{
    LOGIN, PASSWORD as LOGIN_PASSWORD, cookie_of, login_request, sign_in, store_with_account,
    with_cookie, with_owner,
};

const OTHER_LOGIN: &str = "noor@example.com";
const JMAP: &str = "/api/jmap";

/// The address of every fixture account; the fake signs `USER` in.
pub const ADDRESS: &str = "sanne@example.test";

/// How often and how long a test looks for the sync to finish.
const LOOK: Duration = Duration::from_millis(25);
const LOOKS: usize = 800;

/// The instance: the scripted IMAP server, the router and the state
/// behind it; the fixture owner plus a second organization's owner able
/// to sign in.
pub struct Instance {
    pub router: Router,
    pub store: Arc<Store>,
    pub api: ApiState,
    pub fake: FakeImap,
    _ca_file: NamedTempFile,
    /// The data directory of an instance on disk.
    _dir: Option<TempDir>,
}

impl Instance {
    /// Over the given mailbox model on a store in memory, the gate
    /// counting per `window`.
    pub async fn start(mailboxes: Mailboxes, window: Duration) -> Self {
        Self::build(mailboxes, window, store_with_account(), None).await
    }

    /// On a file-backed store in a directory of its own, the one
    /// database the instance and the bridge share.
    pub async fn on_disk(mailboxes: Mailboxes) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = with_owner(Store::open(dir.path()).unwrap());
        Self::build(mailboxes, RUN_WINDOW, store, Some(dir)).await
    }

    async fn build(
        mailboxes: Mailboxes,
        window: Duration,
        store: Arc<Store>,
        dir: Option<TempDir>,
    ) -> Self {
        let fake = FakeImap::start(Script {
            mailboxes,
            ..Script::tls()
        })
        .await;
        let mut dns = FakeDns::default();
        // Port 0 leaves the port to the account's own endpoint.
        let loopback = SocketAddr::new(fake.address.ip(), 0);
        dns.addresses.insert(HOST.to_owned(), vec![loopback]);
        let mut ca_file = NamedTempFile::new().unwrap();
        ca_file.write_all(fake.ca_pem().as_bytes()).unwrap();
        let config = UpstreamConfig {
            allow_private_networks: vec!["127.0.0.0/8".parse().unwrap()],
            additional_ca_file: Some(ca_file.path().to_owned()),
            ..UpstreamConfig::default()
        };
        let (_, other) = identity::create_personal_user(&store, OTHER_LOGIN).unwrap();
        auth::set_password(&store, &other.id, LOGIN_PASSWORD).unwrap();
        let api = ApiState {
            upstream: Arc::new(Upstream::with_dns(&config, Arc::new(dns)).unwrap()),
            gate: Gate::with_window(Arc::clone(&store), window),
            ..api_state(Arc::clone(&store))
        };
        let router = router_with(api.clone());
        Self {
            router,
            store,
            api,
            fake,
            _ca_file: ca_file,
            _dir: dir,
        }
    }

    /// An IMAP account of the fixture user on the scripted server,
    /// sealed with `password`; its id.
    pub fn add_account(&self, password: &str) -> String {
        self.add_account_at(self.fake.address.port(), password)
    }

    /// An IMAP account whose server sits on `port` of the fake's host.
    pub fn add_account_at(&self, port: u16, password: &str) -> String {
        let endpoint = Endpoint {
            host: HOST.to_owned(),
            port,
            tls: TlsMode::Implicit,
        };
        let new = NewAccount {
            address: ADDRESS.to_owned(),
            name: "Work".to_owned(),
            provider: Provider::Generic,
            settings: AccountSettings::Imap {
                username: USER.to_owned(),
                imap: endpoint.clone(),
                smtp: endpoint,
            },
            credential: Credential::Password {
                password: password.to_owned(),
            },
        };
        accounts::add(&self.store, &self.api.keys, &self.scope(None), &new)
            .unwrap()
            .id
            .as_str()
            .to_owned()
    }

    pub async fn sign_in(&self) -> String {
        sign_in(&self.router).await
    }

    /// The second organization's owner, signed in on a plain router over
    /// the same store, so the scripted server sees nothing of it.
    pub async fn sign_in_other(&self) -> String {
        let response = router_on(Arc::clone(&self.store))
            .oneshot(login_request(OTHER_LOGIN, LOGIN_PASSWORD))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        cookie_of(&response)
    }

    /// The account's session object.
    pub async fn session(&self, cookie: &str, id: &str) -> (StatusCode, Value) {
        let request = with_cookie(Method::GET, &format!("{JMAP}/{id}/session"), cookie);
        answer(&self.router, request).await
    }

    /// One Request object on the account's endpoint.
    pub async fn request(&self, cookie: &str, id: &str, body: &Value) -> (StatusCode, Value) {
        let mut request = with_cookie(Method::POST, &format!("{JMAP}/{id}"), cookie);
        request
            .headers_mut()
            .insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
        *request.body_mut() = Body::from(body.to_string());
        answer(&self.router, request).await
    }

    /// The arguments of the first response to one call under every
    /// capability, with the status.
    pub async fn call(&self, cookie: &str, id: &str, call: Value) -> (StatusCode, Value) {
        let (status, mut answer) = self.request(cookie, id, &using(&[call])).await;
        (status, answer["methodResponses"][0][1].take())
    }

    /// Waits until the inbox is synced through; whether it is.
    pub async fn wait_synced(&self, cookie: &str, id: &str) -> bool {
        for _ in 0..LOOKS {
            let (_, answer) = self.call(cookie, id, mailboxes(id)).await;
            let synced = answer["list"].as_array().is_some_and(|list| {
                let inbox = list.iter().find(|mailbox| mailbox["role"] == "inbox");
                inbox.is_some_and(|inbox| {
                    inbox["syncedEmails"]
                        .as_u64()
                        .is_some_and(|synced| synced > 0)
                        && inbox["syncedEmails"] == inbox["totalEmails"]
                })
            });
            if synced {
                return true;
            }
            sleep(LOOK).await;
        }
        false
    }

    /// The row's stop cause word; `None` while it runs.
    pub fn stopped_cause(&self, id: &str) -> Option<String> {
        readers::stopped_cause(&self.store, &self.scope(Some(id)))
    }

    /// The account events as `(type, actor)`, oldest first.
    pub fn account_events(&self) -> Vec<(String, String)> {
        readers::account_events(&self.store, &self.scope(None))
    }

    /// How many sign-ins the scripted server saw.
    pub fn logins(&self) -> usize {
        self.fake
            .lines()
            .iter()
            .filter(|line| line.contains(" LOGIN "))
            .count()
    }

    /// How many mailbox rows the bridge's store holds for the account.
    pub fn bridge_rows(&self, id: &str) -> usize {
        self.api
            .bridge_store
            .mailbox_snapshot(&AccountKey::new(id))
            .unwrap()
            .rows
            .len()
    }

    pub fn user_id(&self) -> UserId {
        match auth::verify_login(&self.store, LOGIN, LOGIN_PASSWORD).unwrap() {
            LoginOutcome::Verified(id) => id,
            _ => panic!("the fixture user signs in"),
        }
    }

    pub fn scope(&self, id: Option<&str>) -> Scope {
        let account = id.map(|id| AccountId::from(id.to_owned()));
        scope::resolve(&self.store, &self.user_id(), account.as_ref()).unwrap()
    }
}

/// A Request object with these calls under every capability.
pub fn using(calls: &[Value]) -> Value {
    json!({
        "using": [CORE_CAPABILITY, MAIL_CAPABILITY, HULIHO_CAPABILITY],
        "methodCalls": calls,
    })
}

/// A `Mailbox/get` call for every mailbox of the account.
pub fn mailboxes(id: &str) -> Value {
    json!(["Mailbox/get", { "accountId": id, "ids": null }, "c1"])
}
