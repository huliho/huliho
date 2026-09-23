// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The runtime of the bridge: where connections come from, the one
//! conversation an account keeps between requests and the task that
//! syncs its folders in the background. The host hands out connections;
//! the bridge never sees a credential and never resolves a host. A
//! request takes the conversation only for a refresh or a preview
//! fetch, so one that reads the cache never waits on IMAP.

mod bridge;
mod worker;

use std::future::Future;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use thiserror::Error;
use tokio::sync::Notify;
use tokio::time::timeout;

pub use bridge::{Bridge, Registration};

use crate::gmail;
use crate::mailboxes::SyncError;
use crate::session::{Session, SessionError};
use crate::store::{AccountKey, MailboxId};
use crate::sync::refresh::{Given, Refreshed, Refresher};
use crate::sync::{Cache, NARROWING_BUDGET};

/// A `/changes` call refreshes its account at most this often; until
/// the server pushes, a client polls about twice as slowly.
pub const REFRESH_INTERVAL: Duration = Duration::from_secs(30);

/// The connections one Gmail account may hold at once, far under
/// Gmail's own cap of fifteen: the conversation the requests take and
/// one of the header sync's own, so a large store never holds up the
/// reading pane.
pub const GMAIL_CONNECTIONS: usize = 2;

/// A kept session is logged out once nothing used it for this long.
pub const SESSION_IDLE_CLOSE: Duration = Duration::from_mins(5);

/// What one turn on the conversation may take: a refresh, a preview
/// fetch or one batch of the header sync. It also bounds one UID FETCH
/// answer, which has per-line timeouts alone.
pub const CONVERSATION_DEADLINE: Duration = Duration::from_secs(60);

/// The accounts whose header sync runs at once, process wide.
pub const SYNC_PARALLELISM: usize = 2;

/// The failures one run of a folder may cost before the folder waits
/// for the next round: above the narrowing budget, so a folder full of
/// hostile messages narrows to its end, with room for a flaky link.
pub const FOLDER_FAILURE_BOUND: usize = NARROWING_BUDGET + 16;

/// The pause before a fresh connection after a failure.
pub const RECONNECT_PAUSE: Duration = Duration::from_secs(2);

/// How long an account waits for its next round after a round ended in
/// failures or found the host holding the account back.
pub const RETRY_INTERVAL: Duration = Duration::from_mins(5);

/// The clocks of the runtime as one value, so a test can shorten them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    /// Between two refreshes of one account.
    pub interval: Duration,
    /// What one turn on the conversation may take.
    pub deadline: Duration,
    /// How long a session stays open unused.
    pub idle_close: Duration,
    /// How long a task waits after a round that did not finish.
    pub retry_interval: Duration,
    /// The pause before connecting again after a failure.
    pub reconnect_pause: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            interval: REFRESH_INTERVAL,
            deadline: CONVERSATION_DEADLINE,
            idle_close: SESSION_IDLE_CLOSE,
            retry_interval: RETRY_INTERVAL,
            reconnect_pause: RECONNECT_PAUSE,
        }
    }
}

/// Why the host opened no session.
#[derive(Debug, Error)]
pub enum ConnectError {
    /// The host holds the account back: it is stopped, gone or not an
    /// account of this kind. Nothing was tried.
    #[error("the host holds the account back")]
    Held,
    /// The host tried and the connection or the sign-in failed; the
    /// host saw the outcome wherever it counts them.
    #[error(transparent)]
    Failed(#[from] SessionError),
}

/// Where connections come from: the host signs in with the credential
/// it keeps and reports every outcome wherever it counts them.
pub trait Connector: Send + Sync {
    /// The session the host opens.
    type Session: Session;

    /// A signed-in session for the account.
    ///
    /// # Errors
    ///
    /// Returns why no session could be opened; the account's requests
    /// answer from the cache then.
    fn connect(
        &self,
        key: &AccountKey,
    ) -> impl Future<Output = Result<Self::Session, ConnectError>> + Send;
}

/// A shared connector connects like the one it wraps, so two
/// conversations of one account draw on one host.
impl<C: Connector> Connector for Arc<C> {
    type Session = C::Session;

    fn connect(
        &self,
        key: &AccountKey,
    ) -> impl Future<Output = Result<Self::Session, ConnectError>> + Send {
        C::connect(self, key)
    }
}

/// What the requests of one account share: the conversation behind a
/// lock and the mailbox the client looked at last.
pub struct Link<C: Connector> {
    pub(crate) wire: tokio::sync::Mutex<Wire<C>>,
    viewed: Mutex<Option<MailboxId>>,
    /// Rings when a refresh finds a store folder whose first sync is not
    /// done, so the account's task picks it up.
    wake: Notify,
}

/// The conversation of one account and what the refresh remembers of
/// it.
pub struct Wire<C: Connector> {
    connector: C,
    session: Option<C::Session>,
    /// Whether the server advertises CONDSTORE, read once per session.
    condstore: bool,
    /// Whether the host's word that the account is a Gmail account holds
    /// on this server, read with it.
    gmail: bool,
    timing: Timing,
    refreshed: Option<Instant>,
    last_used: Option<Instant>,
    refresher: Refresher,
}

impl<C: Connector> Link<C> {
    /// A link on the runtime's clocks.
    pub fn new(connector: C) -> Self {
        Self::with_timing(connector, Timing::default())
    }

    /// A link that refreshes at most once per `interval`.
    pub fn with_interval(connector: C, interval: Duration) -> Self {
        Self::with_timing(
            connector,
            Timing {
                interval,
                ..Timing::default()
            },
        )
    }

    /// A link on the given clocks.
    pub fn with_timing(connector: C, timing: Timing) -> Self {
        Self {
            wire: tokio::sync::Mutex::new(Wire::new(connector, timing)),
            viewed: Mutex::new(None),
            wake: Notify::new(),
        }
    }

    /// Notes the mailbox a query just ranged over.
    pub(crate) fn view(&self, mailbox: MailboxId) {
        *self.viewed.lock().unwrap_or_else(PoisonError::into_inner) = Some(mailbox);
    }

    /// The mailbox the client looked at last.
    pub(crate) fn viewed(&self) -> Option<MailboxId> {
        self.viewed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// What a refresh rings when a store folder waits for its first sync.
    pub(crate) fn wake(&self) -> &Notify {
        &self.wake
    }

    /// Refreshes the account when one is due, within the deadline. A
    /// connection that cannot be opened leaves the cache as it is; the
    /// host's connector saw the failure.
    ///
    /// # Errors
    ///
    /// Returns the store's failure or `Task`.
    pub async fn refresh(&self, cache: &Cache) -> Result<(), SyncError> {
        let viewed = self.viewed();
        let mut wire = self.wire.lock().await;
        if !wire.due() {
            return Ok(());
        }
        if wire.session(cache).await.is_err() {
            return Ok(());
        }
        let given = Given {
            condstore: wire.condstore,
            gmail: wire.gmail,
            viewed: viewed.as_ref(),
        };
        let deadline = wire.timing.deadline;
        let Wire {
            session, refresher, ..
        } = &mut *wire;
        let Some(session) = session.as_mut() else {
            return Ok(());
        };
        let refreshed = match timeout(deadline, refresher.refresh(session, cache, given)).await {
            Ok(refreshed) => refreshed?,
            // The stream may hold the rest of an answer, so the session goes.
            Err(_elapsed) => Refreshed {
                stands: false,
                undone: false,
            },
        };
        if !refreshed.stands {
            wire.drop_session();
        }
        if refreshed.undone {
            self.wake.notify_one();
        }
        Ok(())
    }

    /// Logs the kept session out once nothing used it for `bound`; how
    /// long to wait before the next look.
    pub(crate) async fn close_idle(&self, bound: Duration) -> Duration {
        self.wire.lock().await.close_idle(bound).await
    }
}

impl<C: Connector> Wire<C> {
    pub(crate) fn new(connector: C, timing: Timing) -> Self {
        Self {
            connector,
            session: None,
            condstore: false,
            gmail: false,
            timing,
            refreshed: None,
            last_used: None,
            refresher: Refresher::default(),
        }
    }

    /// The session of the account: the one kept from an earlier use
    /// while it still answers NOOP, a fresh one otherwise.
    ///
    /// # Errors
    ///
    /// Returns the connector's failure or the failure of the first read
    /// on a fresh session.
    pub(crate) async fn session(&mut self, cache: &Cache) -> Result<&mut C::Session, ConnectError> {
        let mut kept = self.session.take();
        if let Some(session) = kept.as_mut()
            && session.noop().await.is_err()
        {
            kept = None;
        }
        let session = if let Some(kept) = kept {
            kept
        } else {
            let mut fresh = self.connector.connect(&cache.key).await?;
            let capabilities = fresh.capabilities().await?;
            self.condstore = capabilities.has("CONDSTORE");
            self.gmail = gmail::confirmed(cache.gmail, &capabilities);
            fresh
        };
        self.last_used = Some(Instant::now());
        Ok(self.session.insert(session))
    }

    /// Drops the session after a failure, since its stream may hold
    /// unread lines; the next use connects again.
    pub(crate) fn drop_session(&mut self) {
        self.session = None;
    }

    /// What one turn on this conversation may take.
    pub(crate) fn deadline(&self) -> Duration {
        self.timing.deadline
    }

    /// Whether a refresh may run now; one that may counts as run,
    /// whatever comes of it, so a server that fails is not asked again
    /// before the interval is over.
    fn due(&mut self) -> bool {
        let now = Instant::now();
        let due = self
            .refreshed
            .is_none_or(|last| now.duration_since(last) >= self.timing.interval);
        if due {
            self.refreshed = Some(now);
        }
        due
    }

    /// Logs the session out once nothing used it for `bound`; how long
    /// to wait before the next look.
    pub(crate) async fn close_idle(&mut self, bound: Duration) -> Duration {
        let Some(last_used) = self.last_used.filter(|_| self.session.is_some()) else {
            return bound;
        };
        let idle = last_used.elapsed();
        if idle < bound {
            return bound.saturating_sub(idle);
        }
        if let Some(session) = self.session.take() {
            // The connection is gone whatever LOGOUT answers.
            let _ended = session.logout().await;
        }
        self.last_used = None;
        bound
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const _: () = assert!(FOLDER_FAILURE_BOUND > NARROWING_BUDGET);

    #[test]
    fn the_clocks_default_to_the_documented_values() {
        let timing = Timing::default();
        assert_eq!(timing.interval, Duration::from_secs(30));
        assert_eq!(timing.deadline, Duration::from_secs(60));
        assert_eq!(timing.idle_close, Duration::from_secs(300));
        assert_eq!(timing.retry_interval, Duration::from_secs(300));
        assert_eq!(timing.reconnect_pause, Duration::from_secs(2));
        assert_eq!(SYNC_PARALLELISM, 2);
        assert_eq!(GMAIL_CONNECTIONS, 2);
    }
}
