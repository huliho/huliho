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
use std::time::Duration;

use reqwest::StatusCode;
use thiserror::Error;
use tokio::sync::{Mutex as AccountLock, OwnedMutexGuard};
use tokio::time::Instant;

pub use reconnect::Reconnect;

use crate::accounts::{self, Account, StopCause};
use crate::events::Actor;
use crate::ids::AccountId;
use crate::probe::ProbeError;
use crate::scope::Scope;
use crate::store::{Store, StoreError};

/// Windows of refused, timed-out or TLS-failed attempts that stop an
/// account: enough to outlast a blip, few enough to leave a server that
/// is down alone.
pub const MAX_REFUSED_RUN: u32 = 5;

/// Failures this close together are one failure: a burst of requests
/// against a server that is down says no more than one request does.
pub const RUN_WINDOW: Duration = Duration::from_secs(10);

/// Why an attempt failed: the upstream refused, answered with an error
/// of its own or this instance could not carry the attempt out.
#[derive(Debug, Error)]
pub enum AttemptError {
    /// The upstream's own refusal.
    #[error(transparent)]
    Upstream(#[from] ProbeError),
    /// The upstream answered with a server error of its own.
    #[error("the server answered {0}")]
    Failed(StatusCode),
    /// The row is stopped with this cause; nothing connected.
    #[error("the account is stopped")]
    Stopped(StopCause),
    /// The store failed, reading the row or writing a rule.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// The task that ran the store work never came back.
    #[error("the store task did not finish")]
    Task,
}

/// The rule a failure falls under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// The upstream judged the credential and said no.
    Credential,
    /// The upstream was not reached or not reached securely.
    Connection,
    /// Neither: nothing here can tell what the account is in for.
    Undecided,
}

impl AttemptError {
    /// The rule this failure falls under.
    #[must_use]
    pub fn fault(&self) -> Fault {
        match self {
            Self::Upstream(ProbeError::CredentialRejected) => Fault::Credential,
            Self::Upstream(ProbeError::Unreachable(_) | ProbeError::Insecure(_)) => {
                Fault::Connection
            }
            Self::Upstream(ProbeError::Unsupported(_) | ProbeError::SmtpAuthUnavailable)
            | Self::Failed(_)
            | Self::Stopped(_)
            | Self::Store(_)
            | Self::Task => Fault::Undecided,
        }
    }
}

/// The run of one account: the windows that counted and when the open
/// one started.
struct Run {
    windows: u32,
    opened: Instant,
}

/// What the gate keeps between attempts: the run per account and one
/// lock per account, so attempts on an account never overlap.
#[derive(Default)]
struct Memory {
    runs: HashMap<AccountId, Run>,
    locks: HashMap<AccountId, Arc<AccountLock<()>>>,
}

/// One outcome as the rules take it.
struct Outcome {
    fault: Option<Fault>,
    /// Whether a pass resumes the row without a run in memory: a check
    /// of a stopped row does, a proxied request on a running row does
    /// not.
    resume: bool,
    /// When the outcome landed, on the runtime's clock.
    at: Instant,
}

impl Outcome {
    fn now(fault: Option<Fault>, resume: bool) -> Self {
        Self {
            fault,
            resume,
            at: Instant::now(),
        }
    }
}

/// The one place the reconnect state of an account is decided.
#[derive(Clone)]
pub struct Gate {
    store: Arc<Store>,
    memory: Arc<Mutex<Memory>>,
    window: Duration,
}

impl Gate {
    /// A gate over the store the rules write to, remembering nothing yet.
    #[must_use]
    pub fn new(store: Arc<Store>) -> Self {
        Self::with_window(store, RUN_WINDOW)
    }

    /// A gate that counts connection failures per `window` instead of
    /// [`RUN_WINDOW`].
    #[must_use]
    pub fn with_window(store: Arc<Store>, window: Duration) -> Self {
        Self {
            store,
            memory: Arc::default(),
            window,
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
    /// the fifth window of connection failures stops it with cause
    /// `connection`; a pass resets the run and resumes a stopped account.
    /// `actor` signs the credential stop and the resume; the run stop is
    /// the system's.
    ///
    /// # Errors
    ///
    /// Returns the attempt's own failure once the rules ran. A rule that
    /// could not be written answers the store's failure instead; a scope
    /// without an account answers [`StoreError::MissingAccount`] and a
    /// store task that did not finish [`AttemptError::Task`].
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
        self.settle(scope, actor, Outcome::now(fault, true)).await?;
        outcome
    }

    /// One outcome from a caller that held no lock: the rules of
    /// [`Gate::attempt`] apply. A pass costs nothing unless a run stands,
    /// since the caller read the row as running.
    ///
    /// # Errors
    ///
    /// Returns the store's failure when a rule could not be written,
    /// [`StoreError::MissingAccount`] for a scope without an account and
    /// [`AttemptError::Task`] when the store task did not finish.
    pub async fn observe(
        &self,
        scope: &Scope,
        actor: &Actor,
        fault: Option<Fault>,
    ) -> Result<(), AttemptError> {
        self.settle(scope, actor, Outcome::now(fault, false)).await
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

    /// The rules on the store's thread.
    async fn settle(
        &self,
        scope: &Scope,
        actor: &Actor,
        outcome: Outcome,
    ) -> Result<(), AttemptError> {
        let (gate, scope, actor) = (self.clone(), scope.clone(), actor.clone());
        tokio::task::spawn_blocking(move || gate.apply(&scope, &actor, &outcome))
            .await
            .map_err(|_| AttemptError::Task)?
            .map_err(AttemptError::from)
    }

    fn apply(&self, scope: &Scope, actor: &Actor, outcome: &Outcome) -> Result<(), StoreError> {
        let account_id = scope.account()?;
        match outcome.fault {
            None if outcome.resume || self.remembers(account_id) => {
                self.passed(scope, actor)?;
            }
            None | Some(Fault::Undecided) => {}
            Some(Fault::Credential) => {
                self.memory().runs.remove(account_id);
                self.stop(scope, StopCause::Credentials, actor)?;
            }
            Some(Fault::Connection) => {
                if self.count(account_id, outcome.at) >= MAX_REFUSED_RUN {
                    self.memory().runs.remove(account_id);
                    self.stop(scope, StopCause::Connection, &Actor::System)?;
                }
            }
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

    fn remembers(&self, account_id: &AccountId) -> bool {
        self.memory().runs.contains_key(account_id)
    }

    /// One more window on the account's run, unless the open window
    /// absorbs this failure; the windows so far.
    fn count(&self, account_id: &AccountId, at: Instant) -> u32 {
        let mut memory = self.memory();
        match memory.runs.get_mut(account_id) {
            Some(run) if at.duration_since(run.opened) < self.window => run.windows,
            Some(run) => {
                run.windows = run.windows.saturating_add(1);
                run.opened = at;
                run.windows
            }
            None => {
                memory.runs.insert(
                    account_id.clone(),
                    Run {
                        windows: 1,
                        opened: at,
                    },
                );
                1
            }
        }
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
            AttemptError::Failed(StatusCode::SERVICE_UNAVAILABLE),
            AttemptError::Stopped(StopCause::Connection),
        ] {
            assert_eq!(error.fault(), Fault::Undecided);
        }
    }

    #[test]
    fn the_run_stops_at_five() {
        assert_eq!(MAX_REFUSED_RUN, 5);
    }

    #[test]
    fn the_window_is_ten_seconds() {
        assert_eq!(RUN_WINDOW, Duration::from_secs(10));
    }
}
