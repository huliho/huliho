// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A live access token for an OAuth account: the stored one while it
//! lasts, a refreshed one with the rotated tokens written back otherwise.

use std::sync::Arc;

use thiserror::Error;

use super::client::{self, TokenError};
use crate::accounts::{self, Credential, StopCause};
use crate::events::Actor;
use crate::providers::{self, OauthClient};
use crate::scope::Scope;
use crate::secrets::Keys;
use crate::session::MS_PER_MINUTE;
use crate::store::{Store, StoreError, now_ms};
use crate::upstream::Upstream;

/// A token running out within this margin is refreshed first, so a
/// connection never starts on a token that dies underway.
const REFRESH_MARGIN_MS: i64 = MS_PER_MINUTE;

/// Why no live token came back.
#[derive(Debug, Error)]
pub enum RefreshError {
    #[error("the account signs in with a password or a token, not through a provider")]
    NotOauth,
    #[error("the provider refuses the grant; the account is stopped")]
    Revoked,
    #[error("no client is registered for the provider")]
    ProviderMissing,
    #[error("the token could not be refreshed: {0}")]
    Unavailable(TokenError),
    #[error("the store task did not finish")]
    Task,
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// What the store holds for the account, read off the runtime.
struct Stored {
    credential: Credential,
    client: Option<OauthClient>,
}

/// The access token of the account the scope names, refreshed through
/// the provider when it runs out within the margin. A refresh the
/// provider refuses stops the account with cause `credentials`.
///
/// # Errors
///
/// Returns [`RefreshError::Revoked`] after such a stop and the other
/// variants when nothing could be done; nothing is stored then.
pub async fn access_token(
    store: Arc<Store>,
    keys: Arc<Keys>,
    upstream: &Upstream,
    scope: Scope,
) -> Result<String, RefreshError> {
    let stored = {
        let (store, keys, scope) = (Arc::clone(&store), Arc::clone(&keys), scope.clone());
        tokio::task::spawn_blocking(move || read(&store, &keys, &scope))
            .await
            .map_err(|_| RefreshError::Task)??
    };
    let Credential::Oauth2 {
        provider,
        refresh_token,
        access_token,
        expires_at,
    } = stored.credential
    else {
        return Err(RefreshError::NotOauth);
    };
    if expires_at.saturating_sub(now_ms()) > REFRESH_MARGIN_MS {
        return Ok(access_token);
    }
    let client = stored.client.ok_or(RefreshError::ProviderMissing)?;
    let refreshed = client::refresh(upstream.http(), &client, &refresh_token).await;
    tokio::task::spawn_blocking(move || match refreshed {
        Ok(tokens) => {
            let rotated = Credential::Oauth2 {
                provider,
                refresh_token: tokens.refresh_token.unwrap_or(refresh_token),
                access_token: tokens.access_token.clone(),
                expires_at: tokens.expires_at,
            };
            accounts::update_credential(&store, &keys, &scope, &rotated)?;
            Ok(tokens.access_token)
        }
        Err(TokenError::Revoked) => {
            accounts::stop(&store, &scope, StopCause::Credentials, &Actor::System)?;
            Err(RefreshError::Revoked)
        }
        Err(other) => Err(RefreshError::Unavailable(other)),
    })
    .await
    .map_err(|_| RefreshError::Task)?
}

fn read(store: &Store, keys: &Keys, scope: &Scope) -> Result<Stored, RefreshError> {
    let credential = accounts::credential(store, keys, scope)?;
    let client = match &credential {
        Credential::Oauth2 { provider, .. } => providers::client(store, keys, *provider)?,
        Credential::Password { .. } | Credential::Bearer { .. } => None,
    };
    Ok(Stored { credential, client })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_margin_is_one_minute() {
        assert_eq!(REFRESH_MARGIN_MS, 60_000);
    }
}
