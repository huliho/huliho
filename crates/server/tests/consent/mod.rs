// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Fixtures for the consent tests: the fixture instance with both
//! sign-in clients registered, its public URL set and the Google hosts
//! answered by fakes on the loopback.

use std::io::Write;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use huliho_imap_bridge::testing::imap::{self, FakeImap};
use huliho_imap_bridge::testing::smtp::{self, FakeSmtp};
use huliho_server::api::ApiState;
use huliho_server::auth::{self, LoginOutcome};
use huliho_server::config::UpstreamConfig;
use huliho_server::ids::UserId;
use huliho_server::providers::{self, OauthClient, OauthProvider};
use huliho_server::store::Store;
use huliho_server::upstream::Upstream;
use huliho_server::{identity, scope};
use serde_json::{Value, json};
use tempfile::NamedTempFile;
use tower::ServiceExt;
use url::{Url, form_urlencoded};

use crate::common::{api_state, router_with};
use crate::fake_dns::FakeDns;
use crate::signin::{
    LOGIN, PASSWORD, body_text, cookie_of, login_request, sign_in, store_with_account, with_cookie,
};
use crate::tls_server::TlsServer;
use crate::token_endpoint::{Answer, CLIENT_ID, CLIENT_SECRET, CODE, Forms, routes};

const START: &str = "/api/accounts/oauth/start";
const PENDING: &str = "/api/accounts/oauth/pending";
const ACCOUNTS: &str = "/api/accounts";
const PUBLIC_URL: &str = "https://mail.example.test";
/// The hosts the Google preset and the Google token endpoint name.
const TOKEN_HOST: &str = "oauth2.googleapis.com";
const IMAP_HOST: &str = "imap.gmail.com";
const SMTP_HOST: &str = "smtp.gmail.com";
const OTHER_LOGIN: &str = "noor@example.com";
pub const GMAIL_ADDRESS: &str = "sanne@gmail.com";

/// A callback as the browser makes it: no CSRF header, a cookie when
/// signed in, the language the window prefers.
pub struct Visit<'a> {
    pub cookie: Option<&'a str>,
    pub provider: &'a str,
    pub query: &'a str,
    pub language: Option<&'a str>,
}

/// The fixture instance: both clients registered, the public URL set,
/// the fixture owner plus a second organization's owner able to sign
/// in, the three Google hosts answered by fakes.
pub struct Rig {
    pub store: Arc<Store>,
    pub api: ApiState,
    pub router: Router,
    pub forms: Forms,
    pub imap: FakeImap,
    pub smtp: FakeSmtp,
    token_server: TlsServer,
    _ca_file: NamedTempFile,
}

impl Rig {
    pub async fn start(answer: Answer, imap: imap::Script, smtp: smtp::Script) -> Self {
        let forms = Forms::default();
        let token_server = TlsServer::start(routes(answer, Arc::clone(&forms))).await;
        // The Gmail preset signs in with the address as the user.
        let imap = FakeImap::start_as(
            imap::Script {
                user: GMAIL_ADDRESS,
                ..imap
            },
            IMAP_HOST,
        )
        .await;
        let smtp = FakeSmtp::start_as(
            smtp::Script {
                user: GMAIL_ADDRESS,
                ..smtp
            },
            SMTP_HOST,
        )
        .await;
        let server_ca_path = token_server.config(true).additional_ca_file.unwrap();
        let server_ca = std::fs::read_to_string(server_ca_path).unwrap();
        let mut ca_file = NamedTempFile::new().unwrap();
        write!(ca_file, "{}{}{server_ca}", imap.ca_pem(), smtp.ca_pem()).unwrap();
        let config = UpstreamConfig {
            allow_private_networks: vec!["127.0.0.0/8".parse().unwrap()],
            additional_ca_file: Some(ca_file.path().to_owned()),
            ..UpstreamConfig::default()
        };
        let mut dns = FakeDns::default();
        dns.addresses
            .insert(TOKEN_HOST.to_owned(), vec![token_server.address]);
        dns.addresses
            .insert(IMAP_HOST.to_owned(), vec![imap.address]);
        dns.addresses
            .insert(SMTP_HOST.to_owned(), vec![smtp.address]);
        let store = store_with_account();
        let (_, other) = identity::create_personal_user(&store, OTHER_LOGIN).unwrap();
        auth::set_password(&store, &other.id, PASSWORD).unwrap();
        let api = ApiState {
            upstream: Arc::new(Upstream::with_dns(&config, Arc::new(dns)).unwrap()),
            public_url: Some(Url::parse(PUBLIC_URL).unwrap()),
            ..api_state(Arc::clone(&store))
        };
        register_clients(&store, &api);
        let router = router_with(api.clone());
        Self {
            store,
            api,
            router,
            forms,
            imap,
            smtp,
            token_server,
            _ca_file: ca_file,
        }
    }

    /// Every request the token endpoint saw, as `METHOD host path`.
    pub fn token_requests(&self) -> Vec<String> {
        self.token_server.requests()
    }

    pub async fn sign_in(&self) -> String {
        sign_in(&self.router).await
    }

    /// The second organization's owner, who holds no flag and no
    /// accounts.
    pub async fn sign_in_other(&self) -> String {
        let response = self
            .router
            .clone()
            .oneshot(login_request(OTHER_LOGIN, PASSWORD))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        cookie_of(&response)
    }

    pub fn user_id(&self) -> UserId {
        user_id(&self.store, LOGIN)
    }

    pub fn other_user_id(&self) -> UserId {
        user_id(&self.store, OTHER_LOGIN)
    }

    pub async fn start_consent(&self, cookie: &str, body: &Value) -> (StatusCode, Value) {
        let mut request = with_cookie(Method::POST, START, cookie);
        request
            .headers_mut()
            .insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
        *request.body_mut() = Body::from(body.to_string());
        self.answer(request).await
    }

    pub async fn pending(&self, cookie: &str, state: &str) -> (StatusCode, Value) {
        self.answer(with_cookie(
            Method::GET,
            &format!("{PENDING}/{state}"),
            cookie,
        ))
        .await
    }

    pub async fn callback(&self, visit: Visit<'_>) -> (StatusCode, String) {
        let mut request =
            Request::get(format!("/auth/{}/callback?{}", visit.provider, visit.query));
        if let Some(cookie) = visit.cookie {
            request = request.header(header::COOKIE, cookie);
        }
        if let Some(language) = visit.language {
            request = request.header(header::ACCEPT_LANGUAGE, language);
        }
        let response = self
            .router
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        (status, body_text(response).await)
    }

    /// The rows the list answers for the cookie's user.
    pub async fn accounts(&self, cookie: &str) -> Vec<Value> {
        let (status, listed) = self
            .answer(with_cookie(Method::GET, ACCOUNTS, cookie))
            .await;
        assert_eq!(status, StatusCode::OK, "{listed}");
        listed["accounts"].as_array().unwrap().clone()
    }

    async fn answer(&self, request: Request<Body>) -> (StatusCode, Value) {
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let text = body_text(response).await;
        (
            status,
            serde_json::from_str(&text).unwrap_or(Value::String(text)),
        )
    }
}

/// Both clients on the instance, registered by the fixture owner while
/// the flag is theirs; the flag leaves again so the tests run as an
/// ordinary user.
fn register_clients(store: &Store, api: &ApiState) {
    identity::grant_instance_admin(store, LOGIN).unwrap();
    let scope = scope::resolve(store, &user_id(store, LOGIN), None).unwrap();
    for provider in [OauthProvider::Google, OauthProvider::Microsoft] {
        let client = OauthClient {
            provider,
            id: CLIENT_ID.to_owned(),
            secret: CLIENT_SECRET.to_owned(),
        };
        providers::set_client(store, &api.keys, &scope, &client).unwrap();
    }
    identity::revoke_instance_admin(store, LOGIN).unwrap();
}

fn user_id(store: &Store, login: &str) -> UserId {
    match auth::verify_login(store, login, PASSWORD).unwrap() {
        LoginOutcome::Verified(id) => id,
        _ => panic!("the fixture user signs in"),
    }
}

/// The start body for a Gmail consent.
pub fn gmail_start_body() -> Value {
    json!({ "provider": "gmail", "address": GMAIL_ADDRESS })
}

/// `code` and `state` as Google appends them to the redirect URI.
pub fn google_query(state: &str) -> String {
    let code: String = form_urlencoded::byte_serialize(CODE.as_bytes()).collect();
    format!("code={code}&state={state}")
}
