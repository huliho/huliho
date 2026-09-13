// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The check of an account as it stands on its row, for the retry action
//! and the probe.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use tokio::time::MissedTickBehavior;

use super::{AttemptError, Gate};
use crate::accounts::{self, Account, AccountSettings, Credential, StopCause};
use crate::discovery::Address;
use crate::events::Actor;
use crate::ids::{AccountId, UserId};
use crate::oauth::{self, RefreshError};
use crate::probe::{Probe, ProbeError};
use crate::scope::{self, Scope};
use crate::secrets::Keys;
use crate::store::StoreError;
use crate::upstream::Upstream;

/// What a check of a stored account reaches for.
#[derive(Clone)]
pub struct Reconnect {
    /// The rules and the store they write to.
    pub gate: Gate,
    /// The keys that open a sealed credential.
    pub keys: Arc<Keys>,
    /// The connector every check resolves and connects through.
    pub upstream: Arc<Upstream>,
}

/// The row's side of an attempt.
struct Stored {
    address: Address,
    settings: AccountSettings,
    credential: Credential,
}

impl Reconnect {
    /// Checks the account as stored through the gate and answers the row
    /// as it stands afterwards. A credential the server refused is not
    /// sent again: the user replaces it.
    ///
    /// # Errors
    ///
    /// Returns the attempt's failure once the rules ran; the row then
    /// says whether the account is stopped.
    pub async fn retry(&self, scope: &Scope, actor: &Actor) -> Result<Account, AttemptError> {
        self.gate
            .attempt(scope, actor, self.boxed_check(scope))
            .await?;
        let (store, scope) = (Arc::clone(self.gate.store()), scope.clone());
        tokio::task::spawn_blocking(move || accounts::get(&store, &scope))
            .await
            .map_err(|_| AttemptError::Task)?
            .map_err(AttemptError::from)
    }

    /// One pass over every account stopped on refused connections; the
    /// number that resumed.
    pub async fn probe_once(&self) -> usize {
        let store = Arc::clone(self.gate.store());
        let listed = tokio::task::spawn_blocking(move || accounts::stopped_on_connection(&store))
            .await
            .map_err(|_| AttemptError::Task)
            .and_then(|rows| rows.map_err(AttemptError::from));
        let stopped = match listed {
            Ok(rows) => rows,
            Err(error) => {
                tracing::warn!(%error, "the probe could not list stopped accounts");
                return 0;
            }
        };
        let mut resumed = 0;
        for (user_id, account_id) in stopped {
            let Some(scope) = self.resolve(&user_id, &account_id).await else {
                continue;
            };
            let attempt = self.boxed_check(&scope);
            match self.gate.attempt(&scope, &Actor::System, attempt).await {
                Ok(()) => resumed += 1,
                Err(error) => {
                    tracing::debug!(account = account_id.as_str(), %error, "still stopped");
                }
            }
        }
        resumed
    }

    /// The probe: at once, so a restart checks stopped accounts without
    /// waiting, then every `interval`. A pass that runs long delays the
    /// next tick rather than firing it right away.
    pub async fn probe_periodically(self, interval: Duration) {
        let mut ticks = tokio::time::interval(interval);
        ticks.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            ticks.tick().await;
            let resumed = self.probe_once().await;
            if resumed > 0 {
                tracing::info!(resumed, "the probe resumed stopped accounts");
            }
        }
    }

    /// The scope of a listed row; `None` when the row left meanwhile or
    /// the store failed, which the next pass sees again.
    async fn resolve(&self, user_id: &UserId, account_id: &AccountId) -> Option<Scope> {
        let store = Arc::clone(self.gate.store());
        let (user_id, account_id) = (user_id.clone(), account_id.clone());
        let resolved = tokio::task::spawn_blocking(move || {
            scope::resolve(&store, &user_id, Some(&account_id))
        })
        .await;
        match resolved {
            Ok(Ok(scope)) => Some(scope),
            Ok(Err(StoreError::NotFound)) => None,
            Ok(Err(error)) => {
                tracing::warn!(%error, "the probe could not resolve an account");
                None
            }
            Err(error) => {
                tracing::warn!(%error, "the probe task failed");
                None
            }
        }
    }

    /// The check on the heap: it carries the TLS and HTTP state of a whole
    /// connection, too much for the stack of a handler.
    fn boxed_check<'a>(
        &'a self,
        scope: &'a Scope,
    ) -> Pin<Box<dyn Future<Output = Result<(), AttemptError>> + Send + 'a>> {
        Box::pin(self.check_stored(scope))
    }

    /// One check of the stored account: the row's target with the row's
    /// credential, an OAuth token refreshed first when it is about to run
    /// out.
    async fn check_stored(&self, scope: &Scope) -> Result<(), AttemptError> {
        let stored = self.read(scope).await?;
        let credential = self.live_credential(scope, stored.credential).await?;
        Probe::new(Arc::clone(&self.upstream))
            .check(&stored.address, &stored.settings, &credential)
            .await
            .map_err(AttemptError::from)
    }

    async fn read(&self, scope: &Scope) -> Result<Stored, AttemptError> {
        let store = Arc::clone(self.gate.store());
        let (keys, scope) = (Arc::clone(&self.keys), scope.clone());
        tokio::task::spawn_blocking(move || -> Result<Stored, AttemptError> {
            let account = accounts::get(&store, &scope)?;
            // Sending a refused credential again invites a lockout at the
            // provider; the verdict stands until the user replaces it.
            if account.stopped_cause == Some(StopCause::Credentials) {
                return Err(ProbeError::CredentialRejected.into());
            }
            let address = Address::parse(&account.address).map_err(|_| {
                ProbeError::Unsupported("the row carries no usable address".to_owned())
            })?;
            Ok(Stored {
                address,
                settings: accounts::settings(&store, &scope)?,
                credential: accounts::credential(&store, &keys, &scope)?,
            })
        })
        .await
        .map_err(|_| AttemptError::Task)?
    }

    /// The credential with a live access token where the row holds OAuth
    /// tokens. The check reads the access token alone, so the rest of the
    /// blob rides along as stored; the refresh itself writes the rotated
    /// tokens to the row.
    async fn live_credential(
        &self,
        scope: &Scope,
        stored: Credential,
    ) -> Result<Credential, AttemptError> {
        let (provider, refresh_token, expires_at) = match stored {
            Credential::Oauth2 {
                provider,
                refresh_token,
                expires_at,
                ..
            } => (provider, refresh_token, expires_at),
            other => return Ok(other),
        };
        let access_token = oauth::access_token(
            Arc::clone(self.gate.store()),
            Arc::clone(&self.keys),
            &self.upstream,
            scope.clone(),
        )
        .await
        .map_err(refresh_error)?;
        Ok(Credential::Oauth2 {
            provider,
            refresh_token,
            access_token,
            expires_at,
        })
    }
}

/// A refresh that failed, in the gate's terms: a refused grant is the
/// credential's verdict and the account is stopped already; a provider
/// that could not be reached or is not registered is a connection
/// failure; the rest decides nothing.
fn refresh_error(error: RefreshError) -> AttemptError {
    match error {
        RefreshError::Revoked => ProbeError::CredentialRejected.into(),
        RefreshError::Unavailable(_) | RefreshError::ProviderMissing => {
            ProbeError::Unreachable("the token could not be refreshed".to_owned()).into()
        }
        RefreshError::NotOauth => {
            ProbeError::Unsupported("the row holds no provider tokens".to_owned()).into()
        }
        RefreshError::Store(inner) => AttemptError::Store(inner),
        RefreshError::Task => AttemptError::Task,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_refresh_reads_as_the_gate_needs_it() {
        assert!(matches!(
            refresh_error(RefreshError::Revoked),
            AttemptError::Upstream(ProbeError::CredentialRejected)
        ));
        assert!(matches!(
            refresh_error(RefreshError::ProviderMissing),
            AttemptError::Upstream(ProbeError::Unreachable(_))
        ));
        assert!(matches!(
            refresh_error(RefreshError::NotOauth),
            AttemptError::Upstream(ProbeError::Unsupported(_))
        ));
        assert!(matches!(
            refresh_error(RefreshError::Store(StoreError::Tampered)),
            AttemptError::Store(StoreError::Tampered)
        ));
        assert!(matches!(
            refresh_error(RefreshError::Task),
            AttemptError::Task
        ));
    }
}
