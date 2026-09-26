// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The consents in flight: bounded, ten minutes each, bound to the user
//! who started them.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use serde::Serialize;

use crate::accounts::Provider;
use crate::discovery::Address;
use crate::ids::{AccountId, UserId};
use crate::probe::ProbeError;
use crate::providers::OauthProvider;
use crate::session::MS_PER_MINUTE;

/// A consent not finished within this time is gone; the card offers a
/// fresh start. A restart forgets them all.
const CONSENT_TTL_MS: i64 = 10 * MS_PER_MINUTE;

/// A user has at most a few consent windows open; one gives way when
/// one more starts, so the map stays bounded by the user count.
const MAX_CONSENTS_PER_USER: usize = 4;

/// What a start files: the state the card polls, the PKCE verifier the
/// exchange needs and whose consent it is.
pub struct NewConsent {
    pub state: String,
    pub verifier: String,
    pub user_id: UserId,
    pub provider: OauthProvider,
    pub account_provider: Provider,
    pub address: Address,
    /// Set for a reconnect: the row whose tokens the consent replaces.
    pub account_id: Option<AccountId>,
}

/// Who may take a consent over: the signed-in user, for the provider the
/// callback path names.
#[derive(Clone, Copy)]
pub struct Claimant<'a> {
    pub user_id: &'a UserId,
    pub provider: OauthProvider,
}

/// A consent as the callback takes it over, once.
pub struct Claimed {
    pub provider: OauthProvider,
    pub account_provider: Provider,
    pub address: Address,
    pub account_id: Option<AccountId>,
    pub verifier: String,
}

/// Why a consent ended without an account; stable words the card
/// renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DeniedCause {
    /// The provider sent an error instead of a code.
    AccessDenied,
    /// The code did not turn into tokens.
    ExchangeFailed,
    /// No refresh token came, so the account would die with the first
    /// access token.
    NoRefreshToken,
    UpstreamCredentials,
    UpstreamUnreachable,
    UpstreamInsecure,
    UpstreamUnsupported,
    SmtpAuthUnavailable,
    /// The instance failed after the check; the log has the cause.
    Failed,
}

impl From<ProbeError> for DeniedCause {
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

/// The answer to the poll.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Outcome {
    Pending,
    Done { account_id: AccountId },
    Denied { cause: DeniedCause },
}

/// Waiting after the start, exchanging once the callback claimed it,
/// then settled either way.
enum Stage {
    Waiting,
    Exchanging,
    Done(AccountId),
    Denied(DeniedCause),
}

struct Consent {
    user_id: UserId,
    provider: OauthProvider,
    account_provider: Provider,
    address: Address,
    account_id: Option<AccountId>,
    verifier: String,
    created_at: i64,
    stage: Stage,
}

impl Consent {
    fn live(&self, now: i64) -> bool {
        now.saturating_sub(self.created_at) < CONSENT_TTL_MS
    }

    fn open(&self) -> bool {
        matches!(self.stage, Stage::Waiting | Stage::Exchanging)
    }
}

/// The consents in flight, keyed by their state.
#[derive(Default)]
pub struct Consents {
    entries: Mutex<HashMap<String, Consent>>,
}

impl Consents {
    /// Files a started consent under its state. Expired entries leave.
    /// Past the cap the user's settled consents give way first, then the
    /// oldest open one: the card reads a result within seconds, while a
    /// window may stay open for minutes.
    pub fn insert(&self, new: NewConsent, now: i64) {
        let mut entries = self.lock();
        entries.retain(|_, consent| consent.live(now));
        let mut own: Vec<(String, (bool, i64))> = entries
            .iter()
            .filter(|(_, consent)| consent.user_id == new.user_id)
            .map(|(state, consent)| (state.clone(), (consent.open(), consent.created_at)))
            .collect();
        own.sort_by_key(|(_, order)| *order);
        let surplus = own.len().saturating_sub(MAX_CONSENTS_PER_USER - 1);
        for (oldest, _) in own.iter().take(surplus) {
            entries.remove(oldest);
        }
        entries.insert(
            new.state,
            Consent {
                user_id: new.user_id,
                provider: new.provider,
                account_provider: new.account_provider,
                address: new.address,
                account_id: new.account_id,
                verifier: new.verifier,
                created_at: now,
                stage: Stage::Waiting,
            },
        );
    }

    /// Hands the consent to the callback: the state must be waiting, live
    /// and the claimant's own for this provider. A second claim gets
    /// nothing.
    pub fn claim(&self, claimant: Claimant<'_>, state: &str, now: i64) -> Option<Claimed> {
        let mut entries = self.lock();
        let consent = entries.get_mut(state)?;
        let claimable = consent.live(now)
            && consent.user_id == *claimant.user_id
            && consent.provider == claimant.provider
            && matches!(consent.stage, Stage::Waiting);
        if !claimable {
            return None;
        }
        consent.stage = Stage::Exchanging;
        Some(Claimed {
            provider: claimant.provider,
            account_provider: consent.account_provider,
            address: consent.address.clone(),
            account_id: consent.account_id.clone(),
            verifier: std::mem::take(&mut consent.verifier),
        })
    }

    /// Records how a claimed consent ended.
    pub fn settle(&self, state: &str, outcome: Result<AccountId, DeniedCause>) {
        if let Some(consent) = self.lock().get_mut(state) {
            consent.stage = match outcome {
                Ok(account_id) => Stage::Done(account_id),
                Err(cause) => Stage::Denied(cause),
            };
        }
    }

    /// Where the user's own live consent stands; `None` for anyone
    /// else's, an unknown one or an expired one.
    pub fn outcome(&self, user_id: &UserId, state: &str, now: i64) -> Option<Outcome> {
        let entries = self.lock();
        let consent = entries.get(state)?;
        if consent.user_id != *user_id || !consent.live(now) {
            return None;
        }
        Some(match &consent.stage {
            Stage::Waiting | Stage::Exchanging => Outcome::Pending,
            Stage::Done(account_id) => Outcome::Done {
                account_id: account_id.clone(),
            },
            Stage::Denied(cause) => Outcome::Denied { cause: *cause },
        })
    }

    /// Ends the user's own open consent, so its callback finds nothing.
    /// `false` for anyone else's, a claimed, a settled, an unknown or an
    /// expired one, which all read alike from outside.
    pub fn end(&self, user_id: &UserId, state: &str, now: i64) -> bool {
        let mut entries = self.lock();
        let open = entries.get(state).is_some_and(|consent| {
            consent.user_id == *user_id
                && consent.live(now)
                && matches!(consent.stage, Stage::Waiting)
        });
        if open {
            entries.remove(state);
        }
        open
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.lock().len()
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, Consent>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests;
