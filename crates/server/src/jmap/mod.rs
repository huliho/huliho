// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The JMAP proxy: one endpoint per account. The browser sends plain
//! JMAP; the proxy adds the account's credential, keeps the upstream
//! within its limits and reports every outcome to the connection gate.
//! For an IMAP account the in-process bridge answers the same routes.

mod endpoints;
mod session;

use std::sync::Arc;

use axum::body::Bytes;
use reqwest::header::{CONTENT_TYPE, HeaderMap};
use reqwest::{Response, StatusCode};
use serde_json::{Map, Value};
use url::{Host, Url};

pub use endpoints::Endpoints;
pub(crate) use session::urls;

use crate::accounts::{self, Account, AccountSettings, Credential};
use crate::discovery::Address;
use crate::events::Actor;
use crate::gate::{AttemptError, Reconnect};
use crate::ids::AccountId;
use crate::probe::{self, MAX_SESSION_BYTES, ProbeError};
use crate::scope::Scope;
use crate::upstream::{BodyError, UpstreamError, read_bounded};

/// A Request object of one MiB at most: room for an `Email/set` with a
/// body, far above what the other API routes take. The bridge's own
/// bound, so both paths take the same body.
pub const JMAP_REQUEST_LIMIT: usize = huliho_imap_bridge::jmap::MAX_SIZE_REQUEST;

/// A Response object of sixteen MiB at most: an `Email/get` window with
/// bodies fits, an upstream that answers without end does not.
pub const JMAP_RESPONSE_LIMIT: usize = 16 * 1024 * 1024;

/// Requests in flight per account, on the native path and through the
/// bridge alike; the value the bridge advertises as
/// `maxConcurrentRequests`.
pub const MAX_CONCURRENT_REQUESTS: usize = huliho_imap_bridge::jmap::MAX_CONCURRENT_REQUESTS;

/// The media type of every JMAP request and answer (RFC 8620 section
/// 3.2).
pub(crate) const JSON: &str = "application/json";

/// Whether a content type is JSON, its parameters aside.
#[must_use]
pub(crate) fn is_json(headers: &HeaderMap) -> bool {
    headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|essence| essence.trim().eq_ignore_ascii_case(JSON))
}

/// The wiring of one proxied request.
#[derive(Clone)]
pub struct Proxy {
    /// The gate, the keys and the connector, as the reconnect check has
    /// them.
    pub wiring: Reconnect,
    /// What the proxy keeps per account.
    pub endpoints: Arc<Endpoints>,
}

/// The row's side of a request.
struct Stored {
    account_id: AccountId,
    address: Address,
    session_url: Url,
    credential: Credential,
}

/// The session object for the browser and the API endpoint the
/// upstream named.
struct Fetched {
    session: Vec<u8>,
    api_url: Url,
}

impl Proxy {
    /// The account's session object as the browser may see it: the URLs
    /// pointing here, the capabilities the proxy carries, the limits it
    /// enforces. The gate sees the outcome, signed by the scope's user.
    /// The caller read the row as running.
    ///
    /// # Errors
    ///
    /// Returns [`ProbeError::Unsupported`] for a row the proxy cannot
    /// serve and the store's failure or [`AttemptError::Task`] when the
    /// row cannot be read, all before anything connects. Otherwise the
    /// attempt's failure once the gate saw it or the gate's own store
    /// failure.
    pub async fn session(&self, account: Account, scope: &Scope) -> Result<Vec<u8>, AttemptError> {
        let stored = self.read(scope, account).await?;
        let outcome = self.fetch(&stored, scope).await;
        self.observe(scope, &outcome).await?;
        Ok(outcome?.session)
    }

    /// One Request object forwarded to the API endpoint with the
    /// account's credential; the Response object as the upstream sent
    /// it. The gate sees the outcome.
    ///
    /// # Errors
    ///
    /// As [`Proxy::session`].
    pub async fn forward(
        &self,
        account: Account,
        scope: &Scope,
        body: Bytes,
    ) -> Result<Vec<u8>, AttemptError> {
        let stored = self.read(scope, account).await?;
        let outcome = self.post(&stored, scope, body).await;
        self.observe(scope, &outcome).await?;
        outcome
    }

    async fn observe<T>(
        &self,
        scope: &Scope,
        outcome: &Result<T, AttemptError>,
    ) -> Result<(), AttemptError> {
        let fault = outcome.as_ref().err().map(AttemptError::fault);
        let actor = Actor::User(scope.user_id().clone());
        self.wiring.gate.observe(scope, &actor, fault).await
    }

    async fn read(&self, scope: &Scope, account: Account) -> Result<Stored, AttemptError> {
        let store = Arc::clone(self.wiring.gate.store());
        let (keys, scope) = (Arc::clone(&self.wiring.keys), scope.clone());
        tokio::task::spawn_blocking(move || -> Result<Stored, AttemptError> {
            let AccountSettings::Jmap { session_url } = accounts::settings(&store, &scope)? else {
                return Err(unsupported("an IMAP account has no proxy route"));
            };
            let address = Address::parse(&account.address)
                .map_err(|_| unsupported("the row carries no usable address"))?;
            Ok(Stored {
                account_id: account.id,
                address,
                session_url,
                credential: accounts::credential(&store, &keys, &scope)?,
            })
        })
        .await
        .map_err(|_| AttemptError::Task)?
    }

    /// The credential with a live access token where the row holds
    /// provider tokens, refreshed under the account's lock so two
    /// requests never redeem one refresh token; the lock covers the
    /// refresh alone.
    async fn live_credential(
        &self,
        scope: &Scope,
        stored: Credential,
    ) -> Result<Credential, AttemptError> {
        if !matches!(stored, Credential::Oauth2 { .. }) {
            return Ok(stored);
        }
        let _held = self.wiring.gate.hold(scope.account()?).await;
        self.wiring.live_credential(scope, stored).await
    }

    async fn fetch(&self, stored: &Stored, scope: &Scope) -> Result<Fetched, AttemptError> {
        let credential = self
            .live_credential(scope, stored.credential.clone())
            .await?;
        self.fetch_with(stored, &credential).await
    }

    /// The session object with the credential in hand: rewritten for the
    /// browser, its API endpoint checked and remembered.
    async fn fetch_with(
        &self,
        stored: &Stored,
        credential: &Credential,
    ) -> Result<Fetched, AttemptError> {
        let request = self.wiring.upstream.http().get(stored.session_url.clone());
        let response = probe::send(request, &stored.address, credential).await?;
        let body = answered(&stored.account_id, response, MAX_SESSION_BYTES).await?;
        let mut object: Map<String, Value> = serde_json::from_slice(&body)
            .map_err(|_| unsupported("the answer is not a session object"))?;
        let named = session::rewrite(&mut object, &stored.account_id)
            .ok_or_else(|| unsupported("the session object names no API endpoint"))?;
        let api_url = self.api_endpoint(&stored.session_url, &named).await?;
        self.endpoints.remember(&stored.account_id, api_url.clone());
        let session =
            serde_json::to_vec(&object).map_err(|error| AttemptError::Store(error.into()))?;
        Ok(Fetched { session, api_url })
    }

    /// The API endpoint the session object named, as a target this
    /// instance may reach: HTTPS on a named host without user
    /// information, outside the private networks.
    async fn api_endpoint(&self, session_url: &Url, named: &str) -> Result<Url, AttemptError> {
        let url = session_url
            .join(named)
            .map_err(|_| unsupported("the API endpoint is not a URL"))?;
        let host = match url.host() {
            Some(Host::Domain(host)) => host.to_owned(),
            _ => return Err(unsupported("the API endpoint names no host")),
        };
        let bare = url.username().is_empty() && url.password().is_none();
        if url.scheme() != "https" || !bare {
            return Err(unsupported("the API endpoint is not a plain https URL"));
        }
        let port = url
            .port_or_known_default()
            .ok_or_else(|| unsupported("the API endpoint names no port"))?;
        self.wiring
            .upstream
            .resolve(&host, port)
            .await
            .map_err(|error| match error {
                UpstreamError::PrivateNetwork { .. } => unsupported(
                    "the API endpoint lies inside a network this instance does not reach",
                ),
                _ => ProbeError::Unreachable("the API endpoint does not resolve".to_owned()).into(),
            })?;
        Ok(url)
    }

    async fn post(
        &self,
        stored: &Stored,
        scope: &Scope,
        body: Bytes,
    ) -> Result<Vec<u8>, AttemptError> {
        let credential = self
            .live_credential(scope, stored.credential.clone())
            .await?;
        let api_url = match self.endpoints.api_url(&stored.account_id) {
            Some(url) => url,
            None => self.fetch_with(stored, &credential).await?.api_url,
        };
        let request = self
            .wiring
            .upstream
            .http()
            .post(api_url)
            .header(CONTENT_TYPE, JSON)
            .body(body);
        let response = probe::send(request, &stored.address, &credential).await?;
        answered(&stored.account_id, response, JMAP_RESPONSE_LIMIT).await
    }
}

/// The upstream's answer to a proxied call: a JSON body within `limit`
/// on a 200; the credential's verdict on a 401; a server error of the
/// upstream's own on a 5xx; anything else is not a JMAP answer.
async fn answered(
    account_id: &AccountId,
    response: Response,
    limit: usize,
) -> Result<Vec<u8>, AttemptError> {
    let status = response.status();
    if status != StatusCode::OK {
        tracing::debug!(
            account = account_id.as_str(),
            status = status.as_u16(),
            "the upstream answered a proxied request with an error"
        );
    }
    match status {
        StatusCode::OK => {}
        StatusCode::UNAUTHORIZED => return Err(ProbeError::CredentialRejected.into()),
        status if status.is_server_error() => return Err(AttemptError::Failed(status)),
        status => return Err(unsupported(format!("the endpoint answered {status}"))),
    }
    if !is_json(response.headers()) {
        return Err(unsupported("the answer is not JSON"));
    }
    read_bounded(response, limit)
        .await
        .map_err(|error| match error {
            BodyError::TooLarge => unsupported("the answer is larger than the proxy carries"),
            BodyError::ReadFailed => {
                ProbeError::Unreachable("the answer ended early".to_owned()).into()
            }
        })
}

/// A server that is not usable through the proxy, in fixed words.
fn unsupported(cause: impl Into<String>) -> AttemptError {
    ProbeError::Unsupported(cause.into()).into()
}

#[cfg(test)]
mod tests {
    use http_body_util::{Full, Limited};
    use reqwest::header::HeaderValue;

    use super::*;
    use crate::gate::Fault;

    fn answer(body: reqwest::Body) -> Response {
        let http = axum::http::Response::builder()
            .header(CONTENT_TYPE, JSON)
            .body(body)
            .unwrap();
        Response::from(http)
    }

    fn headers(content_type: Option<&str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(value) = content_type {
            headers.insert(CONTENT_TYPE, HeaderValue::from_str(value).unwrap());
        }
        headers
    }

    #[test]
    fn json_is_read_by_its_essence_rfc8620_3_2() {
        for value in [
            "application/json",
            "application/json; charset=utf-8",
            "APPLICATION/JSON",
        ] {
            assert!(is_json(&headers(Some(value))), "{value}");
        }
        for value in ["text/html", "application/problem+json", ""] {
            assert!(!is_json(&headers(Some(value))), "{value:?}");
        }
        assert!(!is_json(&headers(None)));
    }

    #[test]
    fn the_limits_are_the_documented_ones_and_the_bridges() {
        assert_eq!(JMAP_REQUEST_LIMIT, 1_048_576);
        assert_eq!(JMAP_RESPONSE_LIMIT, 16 * 1_048_576);
        assert_eq!(MAX_CONCURRENT_REQUESTS, 4);
    }

    #[test]
    fn unsupported_reads_as_the_gate_needs_it() {
        let error = unsupported("odd");
        assert!(matches!(
            error,
            AttemptError::Upstream(ProbeError::Unsupported(_))
        ));
    }

    #[tokio::test]
    async fn a_cut_answer_is_a_connection_fault_and_a_long_one_decides_nothing() {
        let account = AccountId::from("alpha".to_owned());
        let failing = Limited::new(Full::new(Bytes::from_static(b"{}")), 1);
        let cut = answered(&account, answer(reqwest::Body::wrap(failing)), 64).await;
        assert_eq!(cut.unwrap_err().fault(), Fault::Connection);
        let long = answered(&account, answer("{}".into()), 1).await;
        assert_eq!(long.unwrap_err().fault(), Fault::Undecided);
    }
}
