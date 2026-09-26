// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The /api router: its guards and extractors.

mod accounts;
mod consent_page;
mod discover;
mod error;
mod jmap;
mod login;
mod oauth;
mod password;
mod preferences;
mod providers;
mod reconnect;
mod sessions;
mod users;

use std::net::{IpAddr, SocketAddr};
use std::num::NonZeroU32;
use std::sync::{Arc, OnceLock};

use axum::Router;
use axum::extract::{ConnectInfo, DefaultBodyLimit, FromRequestParts, Request};
use axum::http::request::Parts;
use axum::http::{Method, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum_extra::extract::cookie::CookieJar;
use tokio::sync::Semaphore;
use url::Url;

use error::{ApiError, internal};

use crate::bridge::{self, ServerBridge};
use crate::gate::{Gate, Reconnect};
use crate::ids::UserId;
use crate::jmap::{Endpoints, JMAP_REQUEST_LIMIT, Proxy};
use crate::oauth::Consents;
use crate::rate::RateLimiter;
use crate::secrets::Keys;
use crate::session::{self, SESSION_COOKIE, Session, SessionTimeouts};
use crate::store::Store;
use crate::upstream::Upstream;

/// Nothing on /api carries more than a small form.
const API_BODY_LIMIT_BYTES: usize = 16 * 1024;

/// State-changing requests prove they come from the SPA with this header.
const CSRF_HEADER: &str = "x-requested-with";

/// A display name longer than this is a paragraph, not a name.
const MAX_NAME_CHARS: usize = 100;

/// Longer than any app password, API token or OAuth client secret; the
/// body limit stops the rest.
const MAX_CREDENTIAL_BYTES: usize = 1024;

/// Each verification holds 19 MiB of argon2 memory, so concurrency is
/// bounded; further attempts queue on the connection instead.
pub const MAX_CONCURRENT_VERIFICATIONS: usize = 4;

/// Everything the endpoints reach for.
#[derive(Clone)]
pub struct ApiState {
    pub store: Arc<Store>,
    pub keys: Arc<Keys>,
    pub timeouts: SessionTimeouts,
    pub limiter: Arc<RateLimiter>,
    pub verify_gate: Arc<Semaphore>,
    /// From the config; the account list tells the page.
    pub probe_interval_minutes: NonZeroU32,
    /// From the config; without it no sign-in provider is available.
    pub public_url: Option<Url>,
    pub upstream: Arc<Upstream>,
    /// The consents in flight, one process wide.
    pub consents: Arc<Consents>,
    /// The connection gate, one process wide.
    pub gate: Gate,
    /// What the JMAP proxy keeps per account, one process wide.
    pub endpoints: Arc<Endpoints>,
    /// The bridge's own connection to the database, opened with the
    /// store.
    pub bridge_store: Arc<huliho_imap_bridge::store::Store>,
    /// The in-process bridge, wired on first use from the state as it
    /// stands then, so its runtime shares the gate, the keys and the
    /// resolver of the routes.
    pub bridge: Arc<OnceLock<ServerBridge>>,
}

impl ApiState {
    /// The bridge, wired on first use.
    #[must_use]
    pub fn bridge(&self) -> &ServerBridge {
        self.bridge.get_or_init(|| bridge::open(self))
    }
}

/// The wiring of a check on a stored account, as the routes and the
/// probe share it.
impl From<&ApiState> for Reconnect {
    fn from(state: &ApiState) -> Self {
        Self {
            gate: state.gate.clone(),
            keys: Arc::clone(&state.keys),
            upstream: Arc::clone(&state.upstream),
        }
    }
}

/// The wiring of a proxied request: the reconnect wiring plus what the
/// proxy keeps per account.
impl From<&ApiState> for Proxy {
    fn from(state: &ApiState) -> Self {
        Self {
            wiring: Reconnect::from(state),
            endpoints: Arc::clone(&state.endpoints),
        }
    }
}

/// Builds the /api router on the given state.
pub fn router(state: ApiState) -> Router {
    Router::new()
        .route(
            "/session",
            get(login::current_session)
                .post(login::create_session)
                .delete(login::delete_session),
        )
        .route(
            "/sessions",
            get(sessions::list_sessions).delete(sessions::revoke_other_sessions),
        )
        .route("/sessions/{id}", delete(sessions::revoke_session))
        .route("/password", put(password::change_password))
        .route(
            "/accounts",
            get(accounts::list_accounts).post(accounts::add_account),
        )
        .route("/accounts/discover", post(discover::discover))
        .route("/accounts/{id}", delete(accounts::remove_account))
        .route("/accounts/{id}/retry", post(reconnect::retry_account))
        .route(
            "/accounts/{id}/credentials",
            put(reconnect::replace_credentials),
        )
        .route("/accounts/oauth/start", post(oauth::start_consent))
        .route(
            "/accounts/oauth/pending/{state}",
            get(oauth::pending_consent).delete(oauth::end_consent),
        )
        .route("/auth-providers", get(providers::list_providers))
        .route("/auth-providers/{provider}", put(providers::set_provider))
        .route("/jmap/{id}/session", get(jmap::session))
        .route(
            "/jmap/{id}",
            post(jmap::request).layer(DefaultBodyLimit::max(JMAP_REQUEST_LIMIT)),
        )
        .route("/preferences", get(preferences::list_preferences))
        .route("/preferences/{key}", put(preferences::set_preference))
        .route("/users", get(users::list_users).post(users::create_user))
        .route("/users/{id}/password-reset", post(users::reset_password))
        .layer(axum::middleware::from_fn(require_csrf_header))
        .layer(DefaultBodyLimit::max(API_BODY_LIMIT_BYTES))
        .with_state(state)
}

/// The routes a browser reaches by navigation rather than through the
/// app: the provider callback. No CSRF header applies to a GET.
pub fn browser_router(state: ApiState) -> Router {
    Router::new()
        .route("/auth/{provider}/callback", get(oauth::callback))
        .with_state(state)
}

/// What the listener and the request headers say about the client.
struct ClientInfo {
    address: Option<IpAddr>,
    user_agent: String,
}

impl ClientInfo {
    /// The limiter key shared by every credential check from this address.
    fn address_key(&self) -> String {
        let address = self
            .address
            .map_or("unknown".to_owned(), |address| address.to_string());
        format!("ip:{address}")
    }
}

/// The limiter keys discovery and connect share, so nobody scans hosts
/// through the instance.
fn upstream_keys(user_id: &UserId, client: &ClientInfo) -> [String; 2] {
    [
        format!("discover:{}", user_id.as_str()),
        client.address_key(),
    ]
}

/// A secret the client typed: not empty, printable and bounded.
fn secret_fits(secret: &str) -> bool {
    !secret.is_empty()
        && secret.len() <= MAX_CREDENTIAL_BYTES
        && !secret.chars().any(char::is_control)
}

impl<S: Send + Sync> FromRequestParts<S> for ClientInfo {
    type Rejection = std::convert::Infallible;

    fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> + Send {
        let address = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|info| info.0.ip());
        let user_agent = parts
            .headers
            .get(header::USER_AGENT)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        std::future::ready(Ok(Self {
            address,
            user_agent,
        }))
    }
}

/// The session behind the request cookie, resolved before the handler
/// runs; a missing, expired or revoked cookie answers 401. Only the
/// session endpoint and the password change take this one; sign-out
/// needs no session and every other handler takes [`Full`].
struct Authenticated {
    session: Session,
}

impl FromRequestParts<ApiState> for Authenticated {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &ApiState) -> Result<Self, ApiError> {
        let jar = CookieJar::from_request_parts(parts, state)
            .await
            .unwrap_or_default();
        let token = session_token(&jar)?;
        let store = Arc::clone(&state.store);
        let keys = Arc::clone(&state.keys);
        let timeouts = state.timeouts;
        let session = tokio::task::spawn_blocking(move || {
            session::authenticate(&store, &keys, timeouts, &token)
        })
        .await
        .map_err(internal)??;
        Ok(Self { session })
    }
}

/// A session that may do everything; one opened by a one-time password
/// is refused until the password is changed.
struct Full {
    session: Session,
}

impl FromRequestParts<ApiState> for Full {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &ApiState) -> Result<Self, ApiError> {
        let Authenticated { session } = Authenticated::from_request_parts(parts, state).await?;
        if session.password_change_required {
            return Err(ApiError::PasswordChangeRequired);
        }
        Ok(Self { session })
    }
}

/// The client and the full session behind a call that touches the
/// session, taken together so a handler with a path and a body stays
/// within the argument budget.
struct Caller {
    client: ClientInfo,
    session: Session,
}

impl FromRequestParts<ApiState> for Caller {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &ApiState) -> Result<Self, ApiError> {
        let Ok(client) = ClientInfo::from_request_parts(parts, state).await;
        let Full { session } = Full::from_request_parts(parts, state).await?;
        Ok(Self { client, session })
    }
}

async fn require_csrf_header(request: Request, next: Next) -> Response {
    let safe = matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    );
    if !safe && !request.headers().contains_key(CSRF_HEADER) {
        return ApiError::MissingCsrfHeader.into_response();
    }
    next.run(request).await
}

fn session_token(jar: &CookieJar) -> Result<String, ApiError> {
    jar.get(SESSION_COOKIE)
        .map(|cookie| cookie.value().to_owned())
        .ok_or(ApiError::Unauthenticated)
}
