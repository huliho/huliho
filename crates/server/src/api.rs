// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The /api router: session endpoints, their guards and the error shape.

use std::fmt::Display;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::extract::{ConnectInfo, DefaultBodyLimit, FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Json, Response};
use axum::routing::get;
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use serde::{Deserialize, Serialize};
use time::Duration;
use tokio::sync::Semaphore;

use crate::auth::{self, LoginOutcome, MAX_PASSWORD_CHARS};
use crate::identity;
use crate::ids::{OrganizationId, Role, UserId};
use crate::rate::RateLimiter;
use crate::scope;
use crate::secrets::SessionKeys;
use crate::session::{self, SESSION_COOKIE, SessionError, SessionTimeouts};
use crate::store::{Store, now_ms};

/// Nothing on /api carries more than a small form.
const API_BODY_LIMIT_BYTES: usize = 16 * 1024;

/// State-changing requests prove they come from the SPA with this header.
const CSRF_HEADER: &str = "x-requested-with";

/// Longest accepted login name; RFC 5321 caps an address at 254.
const MAX_LOGIN_BYTES: usize = 254;

const MS_PER_SECOND: i64 = 1_000;

/// Each verification holds 19 MiB of argon2 memory, so concurrency is
/// bounded; further attempts queue on the connection instead.
pub const MAX_CONCURRENT_VERIFICATIONS: usize = 4;

/// Everything the session endpoints reach for.
#[derive(Clone)]
pub struct ApiState {
    pub store: Arc<Store>,
    pub keys: Arc<SessionKeys>,
    pub timeouts: SessionTimeouts,
    pub limiter: Arc<RateLimiter>,
    pub verify_gate: Arc<Semaphore>,
}

/// Builds the /api router on the given state.
pub fn router(state: ApiState) -> Router {
    Router::new()
        .route(
            "/session",
            get(current_session)
                .post(create_session)
                .delete(delete_session),
        )
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
            Self::MissingCsrfHeader => "missing_csrf_header",
            Self::RateLimited { .. } => "rate_limited",
            Self::Internal => "internal",
        }
    }

    fn status(&self) -> StatusCode {
        match self {
            Self::InvalidRequest => StatusCode::BAD_REQUEST,
            Self::InvalidCredentials | Self::Unauthenticated => StatusCode::UNAUTHORIZED,
            Self::MissingCsrfHeader => StatusCode::FORBIDDEN,
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
            SessionError::Random | SessionError::Sealing | SessionError::Store(_) => {
                internal(error)
            }
        }
    }
}

fn internal(error: impl Display) -> ApiError {
    tracing::error!(%error, "api request failed");
    ApiError::Internal
}

/// The caller's address, when the listener attached one.
struct ClientAddr(Option<SocketAddr>);

impl<S: Send + Sync> FromRequestParts<S> for ClientAddr {
    type Rejection = std::convert::Infallible;

    fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> + Send {
        let addr = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|info| info.0);
        std::future::ready(Ok(Self(addr)))
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

#[derive(Deserialize)]
struct LoginRequest {
    login: String,
    password: String,
}

async fn create_session(
    State(state): State<ApiState>,
    ClientAddr(addr): ClientAddr,
    jar: CookieJar,
    Json(request): Json<LoginRequest>,
) -> Result<(CookieJar, StatusCode), ApiError> {
    if request.login.is_empty()
        || request.login.len() > MAX_LOGIN_BYTES
        || request.password.chars().count() > MAX_PASSWORD_CHARS
    {
        return Err(ApiError::InvalidRequest);
    }
    let limiter_keys = [
        format!("login:{}", request.login),
        format!(
            "ip:{}",
            addr.map_or("unknown".to_owned(), |a| a.ip().to_string())
        ),
    ];
    let keys: Vec<&str> = limiter_keys.iter().map(String::as_str).collect();
    if let Some(retry_after_ms) = state.limiter.blocked_for(&keys, now_ms()) {
        return Err(ApiError::RateLimited { retry_after_ms });
    }
    let permit = state.verify_gate.acquire().await.map_err(internal)?;
    let store = Arc::clone(&state.store);
    let session_keys = Arc::clone(&state.keys);
    let login = request.login;
    let password = request.password;
    let token = tokio::task::spawn_blocking(move || {
        attempt_login(&store, &session_keys, &login, &password)
    })
    .await
    .map_err(internal)??;
    drop(permit);
    if let Some(token) = token {
        state.limiter.record_success(&keys);
        let jar = jar.add(session_cookie(token, state.timeouts));
        Ok((jar, StatusCode::NO_CONTENT))
    } else {
        state.limiter.record_failure(&keys, now_ms());
        Err(ApiError::InvalidCredentials)
    }
}

fn attempt_login(
    store: &Store,
    keys: &SessionKeys,
    login: &str,
    password: &str,
) -> Result<Option<String>, ApiError> {
    match auth::verify_login(store, login, password).map_err(internal)? {
        LoginOutcome::Verified(user_id) => {
            let token = session::create(store, keys, &user_id)?;
            Ok(Some(token))
        }
        LoginOutcome::Rejected(Some(user_id)) => {
            auth::record_login_failure(store, &user_id).map_err(internal)?;
            Ok(None)
        }
        LoginOutcome::Rejected(None) => Ok(None),
    }
}

#[derive(Serialize)]
struct SessionUser {
    id: UserId,
    login: String,
    role: Role,
}

#[derive(Serialize)]
struct SessionOrganization {
    id: OrganizationId,
    name: String,
}

#[derive(Serialize)]
struct SessionInfo {
    user: SessionUser,
    organization: SessionOrganization,
}

async fn current_session(
    State(state): State<ApiState>,
    jar: CookieJar,
) -> Result<Json<SessionInfo>, ApiError> {
    let token = session_token(&jar)?;
    let store = Arc::clone(&state.store);
    let keys = Arc::clone(&state.keys);
    let timeouts = state.timeouts;
    let info = tokio::task::spawn_blocking(move || -> Result<SessionInfo, ApiError> {
        let user_id = session::authenticate(&store, &keys, timeouts, &token)?;
        let scope = scope::resolve(&store, &user_id, None).map_err(internal)?;
        let user = identity::user(&store, &scope).map_err(internal)?;
        let organization = identity::organization(&store, &scope).map_err(internal)?;
        Ok(SessionInfo {
            user: SessionUser {
                id: user.id,
                login: user.login,
                role: user.role,
            },
            organization: SessionOrganization {
                id: organization.id,
                name: organization.name,
            },
        })
    })
    .await
    .map_err(internal)??;
    Ok(Json(info))
}

async fn delete_session(
    State(state): State<ApiState>,
    jar: CookieJar,
) -> Result<(CookieJar, StatusCode), ApiError> {
    if let Ok(token) = session_token(&jar) {
        let store = Arc::clone(&state.store);
        tokio::task::spawn_blocking(move || session::revoke(&store, &token))
            .await
            .map_err(internal)?
            .map_err(internal)?;
    }
    let jar = jar.remove(Cookie::build(SESSION_COOKIE).path("/"));
    Ok((jar, StatusCode::NO_CONTENT))
}

fn session_token(jar: &CookieJar) -> Result<String, ApiError> {
    jar.get(SESSION_COOKIE)
        .map(|cookie| cookie.value().to_owned())
        .ok_or(ApiError::Unauthenticated)
}

fn session_cookie(token: String, timeouts: SessionTimeouts) -> Cookie<'static> {
    Cookie::build((SESSION_COOKIE, token))
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(Duration::milliseconds(timeouts.absolute_ms))
        .build()
}
