// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Fixtures for the retry, the credential replacement and the probe: an
//! instance over scripted IMAP and SMTP servers whose IMAP host can be
//! pointed at the server or at a closed port between requests.

use std::collections::HashMap;
use std::io::Write;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use huliho_imap_bridge::testing::imap::{self, FakeImap};
use huliho_imap_bridge::testing::smtp::{self, FakeSmtp};
use huliho_imap_bridge::testing::{CLOSED, USER};
use huliho_server::accounts::{
    self, AccountSettings, Credential, Endpoint, NewAccount, Provider, TlsMode,
};
use huliho_server::api::ApiState;
use huliho_server::auth::{self, LoginOutcome};
use huliho_server::config::UpstreamConfig;
use huliho_server::gate::{Gate, Reconnect};
use huliho_server::identity;
use huliho_server::ids::{AccountId, UserId};
use huliho_server::scope::{self, Scope};
use huliho_server::store::Store;
use huliho_server::upstream::{Dns, Lookup, SrvTarget, Upstream};
use serde_json::{Value, json};
use tempfile::NamedTempFile;
use tower::ServiceExt;

use crate::answers::answer;
use crate::common::{api_state, router_on, router_with};
use crate::readers;
use crate::signin::{
    LOGIN, PASSWORD as LOGIN_PASSWORD, cookie_of, login_request, sign_in, store_with_account,
    with_cookie,
};

/// The mail address of the fixture account.
pub const ADDRESS: &str = "sanne@example.test";
const OTHER_LOGIN: &str = "noor@example.com";
const ACCOUNTS: &str = "/api/accounts";

/// A resolver whose answers change between requests, so a server can go
/// away and come back under one name.
#[derive(Default)]
struct SwitchDns {
    addresses: Mutex<HashMap<String, SocketAddr>>,
}

impl SwitchDns {
    fn point(&self, host: &str, address: SocketAddr) {
        self.addresses
            .lock()
            .unwrap()
            .insert(host.to_owned(), address);
    }
}

impl Dns for SwitchDns {
    fn addresses<'a>(&'a self, host: &'a str) -> Lookup<'a, Vec<SocketAddr>> {
        let answer = self
            .addresses
            .lock()
            .unwrap()
            .get(host)
            .map(|address| vec![*address])
            .unwrap_or_default();
        Box::pin(std::future::ready(Ok(answer)))
    }

    fn srv<'a>(&'a self, _service: &'a str) -> Lookup<'a, Vec<SrvTarget>> {
        Box::pin(std::future::ready(Ok(Vec::new())))
    }

    fn mx<'a>(&'a self, _domain: &'a str) -> Lookup<'a, Vec<String>> {
        Box::pin(std::future::ready(Ok(Vec::new())))
    }
}

/// The instance: the fakes, the switchable resolver, the router and the
/// state behind it.
pub struct Instance {
    pub router: Router,
    pub imap: FakeImap,
    store: Arc<Store>,
    api: ApiState,
    dns: Arc<SwitchDns>,
    smtp: FakeSmtp,
    _ca_file: NamedTempFile,
}

impl Instance {
    /// Both fakes up and the IMAP host answering from its fake; the
    /// fixture owner plus a second organization's owner able to sign in.
    pub async fn start(script: imap::Script) -> Self {
        let imap = FakeImap::start(script).await;
        let smtp = FakeSmtp::start(smtp::Script::tls()).await;
        let mut ca_file = NamedTempFile::new().unwrap();
        write!(ca_file, "{}{}", imap.ca_pem(), smtp.ca_pem()).unwrap();
        let config = UpstreamConfig {
            allow_private_networks: vec!["127.0.0.0/8".parse().unwrap()],
            additional_ca_file: Some(ca_file.path().to_owned()),
            ..UpstreamConfig::default()
        };
        let dns = Arc::new(SwitchDns::default());
        dns.point(imap::HOST, imap.address);
        dns.point(smtp::HOST, smtp.address);
        let store = store_with_account();
        let (_, other) = identity::create_personal_user(&store, OTHER_LOGIN).unwrap();
        auth::set_password(&store, &other.id, LOGIN_PASSWORD).unwrap();
        let resolver: Arc<dyn Dns> = dns.clone();
        let api = ApiState {
            upstream: Arc::new(Upstream::with_dns(&config, resolver).unwrap()),
            // The fakes refuse within microseconds, so every failure counts
            // and five retries in a row stop the account.
            gate: Gate::with_window(Arc::clone(&store), Duration::ZERO),
            ..api_state(Arc::clone(&store))
        };
        let router = router_with(api.clone());
        Self {
            router,
            imap,
            store,
            api,
            dns,
            smtp,
            _ca_file: ca_file,
        }
    }

    /// The IMAP host answers from the fake again.
    pub fn server_up(&self) {
        self.dns.point(imap::HOST, self.imap.address);
    }

    /// The IMAP host resolves to a port nothing listens on.
    pub fn server_down(&self) {
        self.dns.point(imap::HOST, CLOSED);
    }

    /// The wiring the binary hands the probe.
    pub fn reconnect(&self) -> Reconnect {
        Reconnect::from(&self.api)
    }

    pub async fn sign_in(&self) -> String {
        sign_in(&self.router).await
    }

    /// The second organization's owner, signed in on a plain router over
    /// the same store, so the fakes see nothing of it.
    pub async fn sign_in_other(&self) -> String {
        let response = router_on(Arc::clone(&self.store))
            .oneshot(login_request(OTHER_LOGIN, LOGIN_PASSWORD))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        cookie_of(&response)
    }

    pub fn user_id(&self) -> UserId {
        match auth::verify_login(&self.store, LOGIN, LOGIN_PASSWORD).unwrap() {
            LoginOutcome::Verified(id) => id,
            _ => panic!("the fixture user signs in"),
        }
    }

    /// An IMAP account of the fixture user on the fakes, sealed with
    /// `password`; its id.
    pub fn add_account(&self, password: &str) -> String {
        let new = NewAccount {
            address: ADDRESS.to_owned(),
            name: "Work".to_owned(),
            provider: Provider::Generic,
            settings: AccountSettings::Imap {
                username: USER.to_owned(),
                imap: Endpoint {
                    host: imap::HOST.to_owned(),
                    port: self.imap.address.port(),
                    tls: TlsMode::Implicit,
                },
                smtp: Endpoint {
                    host: smtp::HOST.to_owned(),
                    port: self.smtp.address.port(),
                    tls: TlsMode::Implicit,
                },
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

    /// The credential sealed on the row.
    pub fn credential(&self, id: &str) -> Credential {
        accounts::credential(&self.store, &self.api.keys, &self.scope(Some(id))).unwrap()
    }

    /// The row's stop cause word; `None` while it runs.
    pub fn stopped_cause(&self, id: &str) -> Option<String> {
        readers::stopped_cause(&self.store, &self.scope(Some(id)))
    }

    /// The account events as `(type, actor)`, oldest first.
    pub fn account_events(&self) -> Vec<(String, String)> {
        readers::account_events(&self.store, &self.scope(None))
    }

    pub async fn retry(&self, cookie: &str, id: &str) -> (StatusCode, Value) {
        let request = with_cookie(Method::POST, &format!("{ACCOUNTS}/{id}/retry"), cookie);
        self.answer(request).await
    }

    pub async fn put_credentials(
        &self,
        cookie: &str,
        id: &str,
        credential: &Value,
    ) -> (StatusCode, Value) {
        let mut request = with_cookie(Method::PUT, &format!("{ACCOUNTS}/{id}/credentials"), cookie);
        request
            .headers_mut()
            .insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
        *request.body_mut() = Body::from(json!({ "credential": credential }).to_string());
        self.answer(request).await
    }

    fn scope(&self, id: Option<&str>) -> Scope {
        let account = id.map(|id| AccountId::from(id.to_owned()));
        scope::resolve(&self.store, &self.user_id(), account.as_ref()).unwrap()
    }

    async fn answer(&self, request: Request<Body>) -> (StatusCode, Value) {
        answer(&self.router, request).await
    }
}

/// A password credential as the client sends it.
pub fn password(password: &str) -> Value {
    json!({ "kind": "password", "password": password })
}
