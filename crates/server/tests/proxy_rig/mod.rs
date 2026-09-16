// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Fixtures for the proxy tests: an instance over the JMAP fixture
//! server (with room for more TLS servers by host) and the requests a
//! test sends through the router.

use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use huliho_server::accounts::{self, AccountSettings, Credential, NewAccount, Provider};
use huliho_server::api::ApiState;
use huliho_server::auth::{self, LoginOutcome};
use huliho_server::config::UpstreamConfig;
use huliho_server::gate::Gate;
use huliho_server::identity;
use huliho_server::ids::{AccountId, UserId};
use huliho_server::scope::{self, Scope};
use huliho_server::store::Store;
use huliho_server::upstream::Upstream;
use serde_json::{Value, json};
use tempfile::NamedTempFile;
use tower::ServiceExt;

use crate::common::{api_state, router_on, router_with};
use crate::fake_dns::FakeDns;
use crate::jmap_upstream::{ADDRESS, HOST, INWARD_HOST, JmapUpstream, UPSTREAM_ACCOUNT};
use crate::readers;
use crate::signin::{
    LOGIN, PASSWORD as LOGIN_PASSWORD, cookie_of, login_request, sign_in, store_with_account,
    with_cookie,
};
use crate::tls_server::TlsServer;

const OTHER_LOGIN: &str = "noor@example.com";
const JMAP: &str = "/api/jmap";

/// A private address for the inward host; the network rule refuses it
/// before anything connects.
const INWARD_ADDRESS: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 0);

/// How the instance is built: the gate's window and the TLS servers the
/// resolver answers besides the JMAP one, by host.
pub struct Setup<'a> {
    pub window: Duration,
    pub servers: &'a [(&'a str, &'a TlsServer)],
}

/// The instance: the JMAP fixture server, the router and the state
/// behind it; the fixture owner plus a second organization's owner able
/// to sign in.
pub struct Instance {
    pub router: Router,
    pub store: Arc<Store>,
    pub api: ApiState,
    pub upstream: JmapUpstream,
    _ca_file: NamedTempFile,
}

impl Instance {
    pub async fn start(setup: Setup<'_>) -> Self {
        let upstream = JmapUpstream::start().await;
        let mut dns = FakeDns::default();
        let loopback = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
        dns.addresses.insert(HOST.to_owned(), vec![loopback]);
        dns.addresses
            .insert(INWARD_HOST.to_owned(), vec![INWARD_ADDRESS]);
        let mut ca_file = NamedTempFile::new().unwrap();
        ca_file
            .write_all(ca_pem(&upstream.config()).as_bytes())
            .unwrap();
        for (host, server) in setup.servers {
            dns.addresses
                .insert((*host).to_owned(), vec![server.address]);
            ca_file
                .write_all(ca_pem(&server.config(true)).as_bytes())
                .unwrap();
        }
        let config = UpstreamConfig {
            allow_private_networks: vec!["127.0.0.0/8".parse().unwrap()],
            additional_ca_file: Some(ca_file.path().to_owned()),
            ..UpstreamConfig::default()
        };
        let store = store_with_account();
        let (_, other) = identity::create_personal_user(&store, OTHER_LOGIN).unwrap();
        auth::set_password(&store, &other.id, LOGIN_PASSWORD).unwrap();
        let api = ApiState {
            upstream: Arc::new(Upstream::with_dns(&config, Arc::new(dns)).unwrap()),
            gate: Gate::with_window(Arc::clone(&store), setup.window),
            ..api_state(Arc::clone(&store))
        };
        let router = router_with(api.clone());
        Self {
            router,
            store,
            api,
            upstream,
            _ca_file: ca_file,
        }
    }

    pub async fn sign_in(&self) -> String {
        sign_in(&self.router).await
    }

    /// A JMAP account of the fixture user on the fixture server, sealed
    /// with `credential`; its id.
    pub fn add_account(&self, credential: Credential) -> String {
        let new = NewAccount {
            address: ADDRESS.to_owned(),
            name: "Work".to_owned(),
            provider: Provider::Generic,
            settings: AccountSettings::Jmap {
                session_url: self.upstream.session_url(),
            },
            credential,
        };
        accounts::add(&self.store, &self.api.keys, &self.scope(None), &new)
            .unwrap()
            .id
            .as_str()
            .to_owned()
    }

    /// The second organization's owner, signed in on a plain router over
    /// the same store, so the fixture server sees nothing of it.
    pub async fn sign_in_other(&self) -> String {
        let response = router_on(Arc::clone(&self.store))
            .oneshot(login_request(OTHER_LOGIN, LOGIN_PASSWORD))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        cookie_of(&response)
    }

    /// The account's session object through the proxy.
    pub async fn session(&self, cookie: &str, id: &str) -> (StatusCode, Value) {
        self.answer(with_cookie(
            Method::GET,
            &format!("{JMAP}/{id}/session"),
            cookie,
        ))
        .await
    }

    pub fn user_id(&self) -> UserId {
        match auth::verify_login(&self.store, LOGIN, LOGIN_PASSWORD).unwrap() {
            LoginOutcome::Verified(id) => id,
            _ => panic!("the fixture user signs in"),
        }
    }

    /// One Request object through the proxy.
    pub async fn request(&self, cookie: &str, id: &str, body: &Value) -> (StatusCode, Value) {
        let mut request = with_cookie(Method::POST, &format!("{JMAP}/{id}"), cookie);
        request
            .headers_mut()
            .insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
        *request.body_mut() = Body::from(body.to_string());
        self.answer(request).await
    }

    /// The row's stop cause word; `None` while it runs.
    pub fn stopped_cause(&self, id: &str) -> Option<String> {
        readers::stopped_cause(&self.store, &self.scope(Some(id)))
    }

    /// The account events as `(type, actor)`, oldest first.
    pub fn account_events(&self) -> Vec<(String, String)> {
        readers::account_events(&self.store, &self.scope(None))
    }

    fn scope(&self, id: Option<&str>) -> Scope {
        let account = id.map(|id| AccountId::from(id.to_owned()));
        scope::resolve(&self.store, &self.user_id(), account.as_ref()).unwrap()
    }

    async fn answer(&self, request: Request<Body>) -> (StatusCode, Value) {
        readers::answer(&self.router, request).await
    }
}

/// The CA the given rules trust, as PEM text.
fn ca_pem(config: &UpstreamConfig) -> String {
    std::fs::read_to_string(config.additional_ca_file.as_ref().unwrap()).unwrap()
}

/// A Request object with one `Email/query` call (RFC 8620 section 3.3).
pub fn query_request() -> Value {
    json!({
        "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
        "methodCalls": [["Email/query", { "accountId": UPSTREAM_ACCOUNT, "limit": 10 }, "c1"]]
    })
}
