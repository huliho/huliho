// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The signed-in user's connected accounts: the list, the removal and
//! the add, which checks the credential upstream before storing.

use std::num::NonZeroU32;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
use serde::{Deserialize, Serialize};
use url::Host;

use super::{ApiError, ApiState, ClientInfo, Full, MAX_NAME_CHARS, internal, upstream_keys};
use crate::accounts::{
    self, Account, AccountKind, AccountSettings, AuthMethod, Credential, Endpoint, NewAccount,
    Provider, StopCause,
};
use crate::discovery::{Address, MAX_ADDRESS_BYTES, named_host};
use crate::ids::AccountId;
use crate::presets;
use crate::probe::Probe;
use crate::scope;
use crate::session;
use crate::store::now_ms;

/// Longer than any app password or API token; the body limit stops the
/// rest.
const MAX_CREDENTIAL_BYTES: usize = 1024;

/// One account as the list shows it; never a credential, never the
/// connection settings.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AccountView {
    id: AccountId,
    address: String,
    name: String,
    provider: Provider,
    kind: AccountKind,
    auth_method: AuthMethod,
    stopped_cause: Option<StopCause>,
    stopped_at: Option<i64>,
    created_at: i64,
}

impl From<Account> for AccountView {
    fn from(account: Account) -> Self {
        Self {
            id: account.id,
            address: account.address,
            name: account.name,
            provider: account.provider,
            kind: account.kind,
            auth_method: account.auth_method,
            stopped_cause: account.stopped_cause,
            stopped_at: account.stopped_at,
            created_at: account.created_at,
        }
    }
}

/// The wire shape of `GET /accounts`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AccountList {
    accounts: Vec<AccountView>,
    /// How often a stopped account is checked, so the page can say so.
    probe_interval_minutes: NonZeroU32,
}

/// What the client sends to connect an account: the target it confirmed
/// and the credential, once.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AddRequest {
    address: String,
    #[serde(default)]
    name: Option<String>,
    provider: Provider,
    target: AccountSettings,
    credential: Credential,
}

pub(super) async fn list_accounts(
    State(state): State<ApiState>,
    auth: Full,
) -> Result<Json<AccountList>, ApiError> {
    let store = Arc::clone(&state.store);
    let views = tokio::task::spawn_blocking(move || -> Result<Vec<AccountView>, ApiError> {
        let scope = scope::resolve(&store, &auth.session.user_id, None)?;
        let rows = accounts::list(&store, &scope)?;
        Ok(rows.into_iter().map(AccountView::from).collect())
    })
    .await
    .map_err(internal)??;
    Ok(Json(AccountList {
        accounts: views,
        probe_interval_minutes: state.probe_interval_minutes,
    }))
}

pub(super) async fn add_account(
    State(state): State<ApiState>,
    client: ClientInfo,
    auth: Full,
    Json(request): Json<AddRequest>,
) -> Result<(StatusCode, Json<AccountView>), ApiError> {
    let (address, new) = new_account(request)?;
    let limiter_keys = upstream_keys(&auth.session.user_id, &client);
    let keys: Vec<&str> = limiter_keys.iter().map(String::as_str).collect();
    let now = now_ms();
    if let Some(retry_after_ms) = state.limiter.blocked_for(&keys, now) {
        return Err(ApiError::RateLimited { retry_after_ms });
    }
    // Every connect counts before it runs, like a discovery; a pass
    // proves the user holds a credential the target accepts.
    state.limiter.record_failure(&keys, now);
    Probe::new(Arc::clone(&state.upstream))
        .check(&address, &new.settings, &new.credential)
        .await?;
    state.limiter.record_success(&keys);
    let store = Arc::clone(&state.store);
    let sealing = Arc::clone(&state.keys);
    let account = tokio::task::spawn_blocking(move || -> Result<Account, ApiError> {
        let scope = scope::resolve(&store, &auth.session.user_id, None)?;
        session::touch(&store, &scope, &auth.session, client.address)?;
        Ok(accounts::add(&store, &sealing, &scope, &new)?)
    })
    .await
    .map_err(internal)??;
    Ok((StatusCode::CREATED, Json(AccountView::from(account))))
}

pub(super) async fn remove_account(
    State(state): State<ApiState>,
    client: ClientInfo,
    auth: Full,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let store = Arc::clone(&state.store);
    tokio::task::spawn_blocking(move || -> Result<(), ApiError> {
        let account_id = AccountId::from(id);
        let scope = scope::resolve(&store, &auth.session.user_id, Some(&account_id))?;
        session::touch(&store, &scope, &auth.session, client.address)?;
        accounts::remove(&store, &scope)?;
        Ok(())
    })
    .await
    .map_err(internal)??;
    Ok(StatusCode::NO_CONTENT)
}

/// The request as a row to add: the address parsed, the name trimmed or
/// defaulted, the target and the credential checked for shape.
fn new_account(request: AddRequest) -> Result<(Address, NewAccount), ApiError> {
    let address = Address::parse(&request.address).map_err(|_| ApiError::InvalidRequest)?;
    let name = match request
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        Some(name) if name.chars().count() <= MAX_NAME_CHARS => name.to_owned(),
        Some(_) => return Err(ApiError::InvalidRequest),
        None => presets::default_name(request.provider, &address),
    };
    if !credential_fits(&request.credential, &request.target) {
        return Err(ApiError::InvalidRequest);
    }
    let settings = normalized(request.target).ok_or(ApiError::InvalidRequest)?;
    let new = NewAccount {
        address: address.to_string(),
        name,
        provider: request.provider,
        settings,
        credential: request.credential,
    };
    Ok((address, new))
}

/// A target names hosts, never address literals, speaks TLS and carries
/// no credential of its own; host names come back in their ASCII form.
fn normalized(target: AccountSettings) -> Option<AccountSettings> {
    match target {
        AccountSettings::Jmap { session_url } => {
            let named = matches!(session_url.host(), Some(Host::Domain(_)));
            let bare = session_url.username().is_empty() && session_url.password().is_none();
            (session_url.scheme() == "https" && named && bare)
                .then_some(AccountSettings::Jmap { session_url })
        }
        AccountSettings::Imap {
            username,
            imap,
            smtp,
        } => {
            if !username_fits(&username) {
                return None;
            }
            Some(AccountSettings::Imap {
                username,
                imap: named_endpoint(&imap)?,
                smtp: named_endpoint(&smtp)?,
            })
        }
    }
}

fn named_endpoint(endpoint: &Endpoint) -> Option<Endpoint> {
    let host = named_host(&endpoint.host)?;
    (endpoint.port != 0).then_some(Endpoint {
        host,
        port: endpoint.port,
        tls: endpoint.tls,
    })
}

fn username_fits(username: &str) -> bool {
    !username.is_empty()
        && username.len() <= MAX_ADDRESS_BYTES
        && !username
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
}

/// The secret is bounded and printable; a token signs in over JMAP only.
fn credential_fits(credential: &Credential, target: &AccountSettings) -> bool {
    let secret = match credential {
        Credential::Password { password } => password,
        Credential::Bearer { token } => {
            if matches!(target, AccountSettings::Imap { .. }) {
                return false;
            }
            token
        }
    };
    !secret.is_empty()
        && secret.len() <= MAX_CREDENTIAL_BYTES
        && !secret.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::TlsMode;

    fn endpoint(host: &str, port: u16) -> Endpoint {
        Endpoint {
            host: host.to_owned(),
            port,
            tls: TlsMode::Implicit,
        }
    }

    #[test]
    fn a_host_name_is_kept_in_its_ascii_form_and_a_literal_refused() {
        let named = named_endpoint(&endpoint("IMAP.Bücher.example", 993)).unwrap();
        assert_eq!(named.host, "imap.xn--bcher-kva.example");
        assert!(named_endpoint(&endpoint("127.0.0.1", 993)).is_none());
        assert!(named_endpoint(&endpoint("imap.example.test", 0)).is_none());
    }

    #[test]
    fn a_session_url_is_https_on_a_name_without_userinfo() {
        for (url, valid) in [
            ("https://api.example.test/jmap/session", true),
            ("http://api.example.test/jmap/session", false),
            ("https://127.0.0.1/jmap/session", false),
            ("https://sanne:secret@api.example.test/jmap/session", false),
        ] {
            let target = AccountSettings::Jmap {
                session_url: url.parse().unwrap(),
            };
            assert_eq!(normalized(target).is_some(), valid, "{url}");
        }
    }

    #[test]
    fn a_token_signs_in_over_jmap_only() {
        let token = Credential::Bearer {
            token: "t".to_owned(),
        };
        let jmap = AccountSettings::Jmap {
            session_url: "https://api.example.test/jmap/session".parse().unwrap(),
        };
        let imap = AccountSettings::Imap {
            username: "sanne".to_owned(),
            imap: endpoint("imap.example.test", 993),
            smtp: endpoint("smtp.example.test", 465),
        };
        assert!(credential_fits(&token, &jmap));
        assert!(!credential_fits(&token, &imap));
        let empty = Credential::Password {
            password: String::new(),
        };
        assert!(!credential_fits(&empty, &imap));
    }
}
