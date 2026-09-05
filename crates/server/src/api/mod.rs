// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The /api router: its guards, extractors and the error shape.

mod accounts;
mod discover;
mod login;
mod password;
mod sessions;
mod users;

use std::fmt::Display;
use std::net::{IpAddr, SocketAddr};
use std::num::NonZeroU32;
use std::sync::Arc;

use axum::Router;
use axum::extract::{ConnectInfo, DefaultBodyLimit, FromRequestParts, Request};
use axum::http::request::Parts;
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{delete, get, post, put};
use axum_extra::extract::cookie::CookieJar;
use serde::Serialize;
use tokio::sync::Semaphore;
use url::Url;

use crate::auth::AuthError;
use crate::rate::RateLimiter;
use crate::secrets::Keys;
use crate::session::{self, SESSION_COOKIE, Session, SessionError, SessionTimeouts};
use crate::store::{MS_PER_SECOND, Store, StoreError};
use crate::upstream::Upstream;

/// Nothing on /api carries more than a small form.
const API_BODY_LIMIT_BYTES: usize = 16 * 1024;

/// State-changing requests prove they come from the SPA with this header.
const CSRF_HEADER: &str = "x-requested-with";

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
        .route("/accounts", get(accounts::list_accounts))
        .route("/accounts/discover", post(discover::discover))
        .route("/accounts/{id}", delete(accounts::remove_account))
        .route("/users", get(users::list_users).post(users::create_user))
        .route("/users/{id}/password-reset", post(users::reset_password))
        .layer(axum::middleware::from_fn(require_csrf_header))
        .layer(DefaultBodyLimit::max(API_BODY_LIMIT_BYTES))
        .with_state(state)
}

/// Stable machine-readable errors; the client owns the wording.
#[derive(Debug)]
enum ApiError {
    InvalidRequest,
    InvalidCredentials,
    Unauthenticated,
    Forbidden,
    PasswordChangeRequired,
    NotFound,
    LoginTaken,
    MissingCsrfHeader,
    RateLimited { retry_after_ms: i64 },
    Internal,
}

impl ApiError {
    fn code(&self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::InvalidCredentials => "invalid_credentials",
            Self::Unauthenticated => "unauthenticated",
            Self::Forbidden => "forbidden",
            Self::PasswordChangeRequired => "password_change_required",
            Self::NotFound => "not_found",
            Self::LoginTaken => "login_taken",
            Self::MissingCsrfHeader => "missing_csrf_header",
            Self::RateLimited { .. } => "rate_limited",
            Self::Internal => "internal",
        }
    }

    fn status(&self) -> StatusCode {
        match self {
            Self::InvalidRequest => StatusCode::BAD_REQUEST,
            Self::InvalidCredentials | Self::Unauthenticated => StatusCode::UNAUTHORIZED,
            Self::Forbidden | Self::PasswordChangeRequired | Self::MissingCsrfHeader => {
                StatusCode::FORBIDDEN
            }
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::LoginTaken => StatusCode::CONFLICT,
            Self::RateLimited { .. } => StatusCode::TOO_MANY_REQUESTS,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

#[derive(Serialize)]
struct ErrorBody {
    error: &'static str,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(ErrorBody { error: self.code() });
        let mut response = (self.status(), body).into_response();
        if let Self::RateLimited { retry_after_ms } = self {
            let seconds = ((retry_after_ms + MS_PER_SECOND - 1) / MS_PER_SECOND).max(1);
            if let Ok(value) = HeaderValue::from_str(&seconds.to_string()) {
                response.headers_mut().insert(header::RETRY_AFTER, value);
            }
        }
        response
    }
}

impl From<SessionError> for ApiError {
    fn from(error: SessionError) -> Self {
        match error {
            SessionError::Unauthenticated => Self::Unauthenticated,
            SessionError::Store(inner) => Self::from(inner),
        }
    }
}

impl From<AuthError> for ApiError {
    fn from(error: AuthError) -> Self {
        match error {
            AuthError::PasswordLength | AuthError::OwnPassword => Self::InvalidRequest,
            AuthError::Store(inner) => Self::from(inner),
            AuthError::Random | AuthError::Hash(_) => internal(error),
        }
    }
}

impl From<StoreError> for ApiError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::NotFound => Self::NotFound,
            StoreError::Forbidden => Self::Forbidden,
            StoreError::CurrentSession => Self::InvalidRequest,
            StoreError::LoginTaken => Self::LoginTaken,
            StoreError::DataDirectory { .. }
            | StoreError::Database(_)
            | StoreError::Migration(_)
            | StoreError::Encoding(_)
            | StoreError::Random
            | StoreError::Sealing
            | StoreError::Tampered
            | StoreError::Poisoned
            | StoreError::LastOwner
            | StoreError::MissingAccount => internal(error),
        }
    }
}

fn internal(error: impl Display) -> ApiError {
    tracing::error!(%error, "api request failed");
    ApiError::Internal
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
