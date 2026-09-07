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

    #[cfg(test)]
    fn len(&self) -> usize {
        self.lock().len()
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, Consent>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_000_000;

    fn user(name: &str) -> UserId {
        UserId::from(name.to_owned())
    }

    fn account(id: &str) -> AccountId {
        AccountId::from(id.to_owned())
    }

    fn filed(consents: &Consents, state: &str, name: &str, now: i64) {
        consents.insert(
            NewConsent {
                state: state.to_owned(),
                verifier: format!("verifier-{state}"),
                user_id: user(name),
                provider: OauthProvider::Google,
                account_provider: Provider::Gmail,
                address: Address::parse("sanne@gmail.com").unwrap(),
                account_id: None,
            },
            now,
        );
    }

    fn claim(
        consents: &Consents,
        name: &str,
        provider: OauthProvider,
        state: &str,
    ) -> Option<Claimed> {
        consents.claim(
            Claimant {
                user_id: &user(name),
                provider,
            },
            state,
            NOW,
        )
    }

    #[test]
    fn a_consent_is_pending_for_its_owner_and_absent_for_anyone_else() {
        let consents = Consents::default();
        filed(&consents, "s1", "mira", NOW);
        assert_eq!(
            consents.outcome(&user("mira"), "s1", NOW),
            Some(Outcome::Pending)
        );
        assert_eq!(consents.outcome(&user("noor"), "s1", NOW), None);
        assert_eq!(consents.outcome(&user("mira"), "s2", NOW), None);
    }

    #[test]
    fn a_claim_happens_once_for_the_owner_and_the_provider() {
        let consents = Consents::default();
        filed(&consents, "s1", "mira", NOW);
        assert!(claim(&consents, "noor", OauthProvider::Google, "s1").is_none());
        assert!(claim(&consents, "mira", OauthProvider::Microsoft, "s1").is_none());
        let claimed = claim(&consents, "mira", OauthProvider::Google, "s1").unwrap();
        assert_eq!(claimed.verifier, "verifier-s1");
        assert_eq!(claimed.provider, OauthProvider::Google);
        assert_eq!(claimed.account_provider, Provider::Gmail);
        assert_eq!(claimed.address.to_string(), "sanne@gmail.com");
        assert!(claim(&consents, "mira", OauthProvider::Google, "s1").is_none());
        assert_eq!(
            consents.outcome(&user("mira"), "s1", NOW),
            Some(Outcome::Pending)
        );
        consents.settle("s1", Ok(account("a")));
        assert_eq!(
            consents.outcome(&user("mira"), "s1", NOW),
            Some(Outcome::Done {
                account_id: account("a")
            })
        );
    }

    #[test]
    fn a_denied_consent_names_its_cause() {
        let consents = Consents::default();
        filed(&consents, "s1", "mira", NOW);
        claim(&consents, "mira", OauthProvider::Google, "s1").unwrap();
        consents.settle("s1", Err(DeniedCause::SmtpAuthUnavailable));
        assert_eq!(
            consents.outcome(&user("mira"), "s1", NOW),
            Some(Outcome::Denied {
                cause: DeniedCause::SmtpAuthUnavailable
            })
        );
    }

    #[test]
    fn a_consent_expires_after_ten_minutes_and_leaves_on_the_next_insert() {
        let consents = Consents::default();
        filed(&consents, "s1", "mira", NOW);
        let later = NOW + CONSENT_TTL_MS;
        assert_eq!(consents.outcome(&user("mira"), "s1", later), None);
        let claimant = Claimant {
            user_id: &user("mira"),
            provider: OauthProvider::Google,
        };
        assert!(consents.claim(claimant, "s1", later).is_none());
        filed(&consents, "s2", "mira", later);
        assert_eq!(consents.len(), 1);
    }

    #[test]
    fn a_fifth_consent_evicts_the_users_settled_or_oldest_and_nobody_elses() {
        let consents = Consents::default();
        for (index, state) in ["a", "b", "c", "d"].into_iter().enumerate() {
            filed(
                &consents,
                state,
                "mira",
                NOW + i64::try_from(index).unwrap(),
            );
        }
        filed(&consents, "noor", "noor", NOW);
        filed(&consents, "e", "mira", NOW + 10);
        assert_eq!(consents.outcome(&user("mira"), "a", NOW + 10), None);
        for state in ["b", "c", "d", "e"] {
            assert_eq!(
                consents.outcome(&user("mira"), state, NOW + 10),
                Some(Outcome::Pending),
                "{state}"
            );
        }
        assert_eq!(
            consents.outcome(&user("noor"), "noor", NOW + 10),
            Some(Outcome::Pending)
        );
        assert_eq!(consents.len(), 5);
        consents.settle("c", Err(DeniedCause::AccessDenied));
        filed(&consents, "f", "mira", NOW + 11);
        assert_eq!(consents.outcome(&user("mira"), "c", NOW + 11), None);
        assert_eq!(
            consents.outcome(&user("mira"), "b", NOW + 11),
            Some(Outcome::Pending)
        );
    }

    #[test]
    fn the_outcomes_serialize_to_their_words() {
        assert_eq!(
            serde_json::to_string(&Outcome::Pending).unwrap(),
            r#"{"status":"pending"}"#
        );
        assert_eq!(
            serde_json::to_string(&Outcome::Done {
                account_id: account("a")
            })
            .unwrap(),
            r#"{"status":"done","accountId":"a"}"#
        );
        assert_eq!(
            serde_json::to_string(&Outcome::Denied {
                cause: DeniedCause::NoRefreshToken
            })
            .unwrap(),
            r#"{"status":"denied","cause":"noRefreshToken"}"#
        );
    }

    #[test]
    fn every_probe_error_has_one_denied_word() {
        let cases = [
            (
                ProbeError::CredentialRejected,
                DeniedCause::UpstreamCredentials,
            ),
            (
                ProbeError::Unreachable(String::new()),
                DeniedCause::UpstreamUnreachable,
            ),
            (
                ProbeError::Insecure(String::new()),
                DeniedCause::UpstreamInsecure,
            ),
            (
                ProbeError::Unsupported(String::new()),
                DeniedCause::UpstreamUnsupported,
            ),
            (
                ProbeError::SmtpAuthUnavailable,
                DeniedCause::SmtpAuthUnavailable,
            ),
        ];
        for (error, cause) in cases {
            assert_eq!(DeniedCause::from(error), cause);
        }
    }
}
