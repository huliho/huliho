// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The error shape of /api: stable machine-readable codes; the client
//! owns the wording.

use std::fmt::Display;

use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Json, Response};
use serde::Serialize;

use crate::accounts::StopCause;
use crate::auth::AuthError;
use crate::gate::AttemptError;
use crate::probe::ProbeError;
use crate::session::SessionError;
use crate::store::{MS_PER_SECOND, StoreError};

/// Stable machine-readable errors; the client owns the wording.
#[derive(Debug)]
pub(super) enum ApiError {
    InvalidRequest,
    InvalidCredentials,
    Unauthenticated,
    Forbidden,
    PasswordChangeRequired,
    NotFound,
    LoginTaken,
    MissingCsrfHeader,
    RateLimited { retry_after_ms: i64 },
    ProviderNotConfigured,
    StillStopped { cause: StopCause },
    UpstreamCredentials,
    UpstreamUnreachable,
    UpstreamInsecure,
    UpstreamUnsupported,
    UpstreamFailed,
    SmtpAuthUnavailable,
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
            Self::ProviderNotConfigured => "provider_not_configured",
            Self::StillStopped { .. } => "still_stopped",
            Self::UpstreamCredentials => "upstream_credentials",
            Self::UpstreamUnreachable => "upstream_unreachable",
            Self::UpstreamInsecure => "upstream_insecure",
            Self::UpstreamUnsupported => "upstream_unsupported",
            Self::UpstreamFailed => "upstream_failed",
            Self::SmtpAuthUnavailable => "smtp_auth_unavailable",
            Self::Internal => "internal",
        }
    }

    fn status(&self) -> StatusCode {
        match self {
            Self::InvalidRequest
            | Self::UpstreamInsecure
            | Self::UpstreamUnsupported
            | Self::SmtpAuthUnavailable => StatusCode::BAD_REQUEST,
            Self::InvalidCredentials | Self::Unauthenticated | Self::UpstreamCredentials => {
                StatusCode::UNAUTHORIZED
            }
            Self::UpstreamUnreachable | Self::UpstreamFailed => StatusCode::BAD_GATEWAY,
            Self::Forbidden | Self::PasswordChangeRequired | Self::MissingCsrfHeader => {
                StatusCode::FORBIDDEN
            }
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::LoginTaken | Self::ProviderNotConfigured | Self::StillStopped { .. } => {
                StatusCode::CONFLICT
            }
            Self::RateLimited { .. } => StatusCode::TOO_MANY_REQUESTS,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

#[derive(Serialize)]
struct ErrorBody {
    error: &'static str,
    /// The stop cause, on `still_stopped` only.
    #[serde(skip_serializing_if = "Option::is_none")]
    cause: Option<StopCause>,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let cause = match &self {
            Self::StillStopped { cause } => Some(*cause),
            _ => None,
        };
        let body = Json(ErrorBody {
            error: self.code(),
            cause,
        });
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

impl From<ProbeError> for ApiError {
    fn from(error: ProbeError) -> Self {
        match error {
            ProbeError::CredentialRejected => Self::UpstreamCredentials,
            ProbeError::Unreachable(_) => Self::UpstreamUnreachable,
            ProbeError::Insecure(_) => Self::UpstreamInsecure,
            ProbeError::Unsupported(_) => Self::UpstreamUnsupported,
            ProbeError::SmtpAuthUnavailable => Self::SmtpAuthUnavailable,
        }
    }
}

impl From<AttemptError> for ApiError {
    fn from(error: AttemptError) -> Self {
        match error {
            AttemptError::Upstream(inner) => Self::from(inner),
            AttemptError::Store(inner) => Self::from(inner),
            AttemptError::Failed(_) => Self::UpstreamFailed,
            AttemptError::Stopped(cause) => Self::StillStopped { cause },
            AttemptError::Task => internal(AttemptError::Task),
        }
    }
}

pub(super) fn internal(error: impl Display) -> ApiError {
    tracing::error!(%error, "api request failed");
    ApiError::Internal
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_attempt_error_has_one_api_word() {
        assert!(matches!(
            ApiError::from(AttemptError::Failed(StatusCode::BAD_GATEWAY)),
            ApiError::UpstreamFailed
        ));
        assert_eq!(ApiError::UpstreamFailed.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(ApiError::UpstreamFailed.code(), "upstream_failed");
        assert!(matches!(
            ApiError::from(AttemptError::Stopped(StopCause::Credentials)),
            ApiError::StillStopped {
                cause: StopCause::Credentials
            }
        ));
        assert!(matches!(
            ApiError::from(AttemptError::from(ProbeError::CredentialRejected)),
            ApiError::UpstreamCredentials
        ));
        assert!(matches!(
            ApiError::from(AttemptError::from(StoreError::NotFound)),
            ApiError::NotFound
        ));
        assert!(matches!(
            ApiError::from(AttemptError::Task),
            ApiError::Internal
        ));
    }
}
