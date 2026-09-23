// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The task of one account: it syncs every store folder whose first
//! sync is not done, one batch per turn on the conversation so a request
//! gets in between, connects again after a failure and logs the
//! conversation out once nobody used it for a while.

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Semaphore;
use tokio::time::{sleep, timeout};

use super::bridge::Account;
use super::{ConnectError, Connector, FOLDER_FAILURE_BOUND, Timing};
use crate::mailboxes::SyncError;
use crate::session::Session;
use crate::store::MailboxRow;
use crate::sync::{Cache, FolderSync, Step, blocking};

/// How a round over the account's folders ended.
enum Round {
    /// Every store folder is done; the task waits to be woken.
    Done,
    /// Something stood in the way; the task tries again after the
    /// retry interval.
    Retry,
}

/// How one folder ended.
enum Outcome {
    /// Done, or nothing to do until the next mailbox pass.
    Done,
    /// The host holds the account back.
    Held,
    /// The failures passed their bound; the next round tries again.
    GivenUp,
    /// The store failed; the round ends here.
    Failed(SyncError),
}

/// Why one turn on the conversation did not end in a step.
enum Attempt {
    /// The host opened no session.
    Connect,
    /// The session failed or ran past the deadline; it is dropped.
    Session,
    /// The store or its task failed.
    Store(SyncError),
}

/// One folder as its turns take it.
struct Turn<'a> {
    cache: &'a Cache,
    row: &'a MailboxRow,
    deadline: Duration,
}

/// Runs the account until the task is stopped.
pub(super) async fn run<C: Connector>(
    account: Arc<Account<C>>,
    syncing: Arc<Semaphore>,
    timing: Timing,
) {
    loop {
        let Ok(permit) = Arc::clone(&syncing).acquire_owned().await else {
            return;
        };
        let round = round(&account, timing).await;
        drop(permit);
        let limit = match round {
            Round::Done => None,
            Round::Retry => Some(timing.retry_interval),
        };
        wait(&account, timing, limit).await;
    }
}

/// One round: the refresh brings the rows up when one is due, then
/// every store folder whose first sync is not done, in tree order.
async fn round<C: Connector>(account: &Account<C>, timing: Timing) -> Round {
    let key = account.cache.key.as_str();
    if let Err(error) = account.link.refresh(&account.cache).await {
        tracing::warn!(account = key, %error, "the refresh ahead of the sync failed");
        return Round::Retry;
    }
    let (store, owned) = (Arc::clone(&account.cache.store), account.cache.key.clone());
    let snapshot = match blocking(move || store.mailbox_snapshot(&owned)).await {
        Ok(snapshot) => snapshot,
        Err(error) => {
            tracing::warn!(account = key, %error, "the mailbox rows could not be read");
            return Round::Retry;
        }
    };
    // No rows means no mailbox pass has succeeded yet.
    if snapshot.rows.is_empty() {
        return Round::Retry;
    }
    let waiting: Vec<&MailboxRow> = snapshot
        .rows
        .iter()
        .filter(|row| row.facts.store && !snapshot.done.contains(&row.id))
        .collect();
    folders(account, timing, &waiting).await
}

/// The folders of one round, each to its end or its bound.
async fn folders<C: Connector>(
    account: &Account<C>,
    timing: Timing,
    waiting: &[&MailboxRow],
) -> Round {
    let key = account.cache.key.as_str();
    let mut retry = false;
    for row in waiting {
        match folder(account, timing, row).await {
            Outcome::Done => {}
            Outcome::Held => return Round::Retry,
            Outcome::GivenUp => {
                tracing::debug!(account = key, folder = %row.id, "the folder waits for the next round");
                retry = true;
            }
            Outcome::Failed(error) => {
                tracing::warn!(account = key, %error, "the sync of a folder failed");
                return Round::Retry;
            }
        }
    }
    if retry { Round::Retry } else { Round::Done }
}

/// One folder to its end: every batch takes the conversation for its
/// own turn and hands it back, a failure drops the session and connects
/// again after a pause, inside the failure bound.
async fn folder<C: Connector>(account: &Account<C>, timing: Timing, row: &MailboxRow) -> Outcome {
    let turn = Turn {
        cache: &account.cache,
        row,
        deadline: timing.deadline,
    };
    let mut sync = None;
    let mut failures = 0;
    loop {
        let mut wire = account.sync_wire().lock().await;
        let step = match wire.session(&account.cache).await {
            Ok(session) => attempt(session, &turn, &mut sync).await,
            Err(ConnectError::Held) => return Outcome::Held,
            Err(ConnectError::Failed(_)) => Err(Attempt::Connect),
        };
        match step {
            Ok(Step::More) => {
                drop(wire);
                continue;
            }
            Ok(Step::Done | Step::Stale) => return Outcome::Done,
            Err(Attempt::Store(error)) => return Outcome::Failed(error),
            Err(Attempt::Connect) => {}
            Err(Attempt::Session) => wire.drop_session(),
        }
        drop(wire);
        failures += 1;
        if failures >= FOLDER_FAILURE_BOUND {
            return Outcome::GivenUp;
        }
        sleep(timing.reconnect_pause).await;
    }
}

/// One turn: the folder opened or selected again, then one batch. Every
/// turn selects the folder afresh, since the conversation may have
/// selected another one in between.
async fn attempt<S: Session>(
    session: &mut S,
    turn: &Turn<'_>,
    sync: &mut Option<FolderSync>,
) -> Result<Step, Attempt> {
    let work = async {
        if sync.is_none() {
            match FolderSync::open(session, turn.cache, turn.row).await? {
                Some(opened) => *sync = Some(opened),
                None => return Ok(Step::Stale),
            }
        } else if let Some(current) = sync.as_mut()
            && !current.resume(session).await?
        {
            return Ok(Step::Stale);
        }
        match sync.as_mut() {
            Some(current) => current.batch(session, turn.cache).await,
            None => Ok(Step::Stale),
        }
    };
    match timeout(turn.deadline, work).await {
        Ok(Ok(step)) => Ok(step),
        Ok(Err(SyncError::Session(_))) | Err(_) => Err(Attempt::Session),
        Ok(Err(other)) => Err(Attempt::Store(other)),
    }
}

/// Waits to be woken and logs out whatever conversation stood idle past
/// its bound meanwhile; with a `limit` the wait ends there at the
/// latest.
async fn wait<C: Connector>(account: &Account<C>, timing: Timing, limit: Option<Duration>) {
    let until = limit.map(|limit| Instant::now() + limit);
    loop {
        let mut nap = account.link.close_idle(timing.idle_close).await;
        if let Some(own) = &account.own {
            nap = nap.min(own.lock().await.close_idle(timing.idle_close).await);
        }
        if let Some(until) = until {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return;
            }
            nap = nap.min(left);
        }
        if timeout(nap, account.link.wake().notified()).await.is_ok() {
            return;
        }
    }
}
