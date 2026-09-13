// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The connection gate: every upstream attempt on an account hands its
//! outcome in here, where the stop rules apply. The run of refused
//! connections lives in memory; the stop lives on the row.

mod reconnect;

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use thiserror::Error;
use tokio::sync::{Mutex as AccountLock, OwnedMutexGuard};

pub use reconnect::Reconnect;

use crate::accounts::{self, Account, StopCause};
use crate::events::Actor;
use crate::ids::AccountId;
use crate::probe::ProbeError;
use crate::scope::Scope;
use crate::store::{Store, StoreError};

/// Consecutive refused, timed-out or TLS-failed attempts that stop an
/// account: enough to outlast a blip, few enough to leave a server that
/// is down alone.
pub const MAX_REFUSED_RUN: u32 = 5;

/// Why an attempt failed: the upstream refused or this instance could
/// not carry the attempt out.
#[derive(Debug, Error)]
pub enum AttemptError {
    /// The upstream's own refusal.
    #[error(transparent)]
    Upstream(#[from] ProbeError),
    /// The store failed, reading the row or writing a rule.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// The task that ran the store work never came back.
    #[error("the store task did not finish")]
    Task,
}

/// The rule a failure falls under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fault {
    /// The upstream judged the credential and said no.
    Credential,
    /// The upstream was not reached or not reached securely.
    Connection,
    /// Neither: nothing here can tell what the account is in for.
    Undecided,
}

impl AttemptError {
    fn fault(&self) -> Fault {
        match self {
            Self::Upstream(ProbeError::CredentialRejected) => Fault::Credential,
            Self::Upstream(ProbeError::Unreachable(_) | ProbeError::Insecure(_)) => {
                Fault::Connection
            }
            Self::Upstream(ProbeError::Unsupported(_) | ProbeError::SmtpAuthUnavailable)
            | Self::Store(_)
            | Self::Task => Fault::Undecided,
        }
    }
}

/// What the gate keeps between attempts: the refused run per account and
/// one lock per account, so attempts on an account never overlap.
#[derive(Default)]
struct Memory {
    runs: HashMap<AccountId, u32>,
    locks: HashMap<AccountId, Arc<AccountLock<()>>>,
}

/// The one place the reconnect state of an account is decided.
#[derive(Clone)]
pub struct Gate {
    store: Arc<Store>,
    memory: Arc<Mutex<Memory>>,
}

impl Gate {
    /// A gate over the store the rules write to, remembering nothing yet.
    #[must_use]
    pub fn new(store: Arc<Store>) -> Self {
        Self {
            store,
            memory: Arc::default(),
        }
    }

    /// The store the rules write to.
    #[must_use]
    pub fn store(&self) -> &Arc<Store> {
        &self.store
    }

    /// Runs one upstream attempt on the scope's account under the
    /// account's lock and applies the rules to its outcome. A rejected
    /// credential stops the account with cause `credentials` at once;
    /// the fifth consecutive connection failure stops it with cause
    /// `connection`; a pass resets the run and resumes a stopped account.
    /// `actor` signs the credential stop and the resume; the run stop is
    /// the system's.
    ///
    /// # Errors
    ///
    /// Returns the attempt's own failure once the rules ran. A rule that
    /// could not be written answers the store's failure instead.
    pub async fn attempt<T, F>(
        &self,
        scope: &Scope,
        actor: &Actor,
        attempt: F,
    ) -> Result<T, AttemptError>
    where
        F: Future<Output = Result<T, AttemptError>>,
    {
        let account_id = scope.account()?.clone();
        let _held = self.hold(&account_id).await;
        let outcome = attempt.await;
        let fault = outcome.as_ref().err().map(AttemptError::fault);
        let (gate, scope, actor) = (self.clone(), scope.clone(), actor.clone());
        tokio::task::spawn_blocking(move || gate.apply(&scope, &actor, fault))
            .await
            .map_err(|_| AttemptError::Task)??;
        outcome
    }

    /// The account's lock. Attempts and credential changes on one account
    /// run one at a time, so a refresh never redeems a token twice.
    pub async fn hold(&self, account_id: &AccountId) -> OwnedMutexGuard<()> {
        let lock = {
            let mut memory = self.memory();
            Arc::clone(memory.locks.entry(account_id.clone()).or_default())
        };
        lock.lock_owned().await
    }

    /// A passing attempt outside [`Gate::attempt`]: the run resets and a
    /// stopped account resumes. Answers the row as it stands.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::MissingAccount`] for a scope without an
    /// account and [`StoreError::NotFound`] when the row is gone.
    pub fn passed(&self, scope: &Scope, actor: &Actor) -> Result<Account, StoreError> {
        let account_id = scope.account()?;
        self.memory().runs.remove(account_id);
        let resumed = accounts::resume(&self.store, scope, actor)?;
        if resumed.was_stopped {
            tracing::info!(account = account_id.as_str(), "account resumed");
        }
        Ok(resumed.account)
    }

    /// Drops what the gate remembers of an account once its row left. An
    /// attempt still holding the old lock keeps it to itself.
    pub fn forget(&self, account_id: &AccountId) {
        let mut memory = self.memory();
        memory.runs.remove(account_id);
        memory.locks.remove(account_id);
    }

    fn apply(&self, scope: &Scope, actor: &Actor, fault: Option<Fault>) -> Result<(), StoreError> {
        let account_id = scope.account()?;
        match fault {
            None => {
                self.passed(scope, actor)?;
            }
            Some(Fault::Credential) => {
                self.memory().runs.remove(account_id);
                self.stop(scope, StopCause::Credentials, actor)?;
            }
            Some(Fault::Connection) => {
                if self.count(account_id) >= MAX_REFUSED_RUN {
                    self.memory().runs.remove(account_id);
                    self.stop(scope, StopCause::Connection, &Actor::System)?;
                }
            }
            Some(Fault::Undecided) => {}
        }
        Ok(())
    }

    fn stop(&self, scope: &Scope, cause: StopCause, actor: &Actor) -> Result<(), StoreError> {
        if accounts::stop(&self.store, scope, cause, actor)? {
            tracing::info!(
                account = scope.account()?.as_str(),
                cause = cause.as_str(),
                "account stopped"
            );
        }
        Ok(())
    }

    /// One more failure on the account's run; the length so far.
    fn count(&self, account_id: &AccountId) -> u32 {
        let mut memory = self.memory();
        let run = memory.runs.entry(account_id.clone()).or_default();
        *run = run.saturating_add(1);
        *run
    }

    fn memory(&self) -> MutexGuard<'_, Memory> {
        self.memory.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_failure_falls_under_one_rule() {
        let rejected: AttemptError = ProbeError::CredentialRejected.into();
        assert_eq!(rejected.fault(), Fault::Credential);
        for error in [
            ProbeError::Unreachable("closed".to_owned()),
            ProbeError::Insecure("refused".to_owned()),
        ] {
            assert_eq!(AttemptError::from(error).fault(), Fault::Connection);
        }
        for error in [
            AttemptError::from(ProbeError::Unsupported("odd".to_owned())),
            AttemptError::from(ProbeError::SmtpAuthUnavailable),
            AttemptError::from(StoreError::NotFound),
            AttemptError::Task,
        ] {
            assert_eq!(error.fault(), Fault::Undecided);
        }
    }

    #[test]
    fn the_run_stops_at_five() {
        assert_eq!(MAX_REFUSED_RUN, 5);
    }
}
