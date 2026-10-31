// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The runtime of the bridge: where connections come from, the one
//! conversation an account keeps between requests, the connection its
//! searches run on and the task that syncs its folders in the
//! background. The host hands out connections; the bridge never sees a
//! credential and never resolves a host. A request takes the
//! conversation only for a refresh, a preview fetch, a body fetch or an
//! `Email/set`, so one that reads the cache never waits on IMAP; a blob
//! takes it one window at a time.

mod bridge;
pub(crate) mod folders;
pub(crate) mod search;
mod timing;
mod worker;

use std::future::Future;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use thiserror::Error;
use tokio::sync::Notify;
use tokio::time::timeout;

pub use bridge::{Bridge, Registration};
pub use search::{SEARCH_CACHE_ENTRIES, SEARCH_CACHE_TTL};
pub use timing::{
    CONVERSATION_DEADLINE, GMAIL_LIST_AGE, RECONNECT_PAUSE, REFRESH_INTERVAL, RETRY_INTERVAL,
    SEARCH_DEADLINE, SESSION_IDLE_CLOSE, Timing,
};

use crate::gmail;
use crate::mailboxes::SyncError;
use crate::session::{Moving, Searching, Session, SessionError};
use crate::store::{AccountKey, MailboxId};
use crate::sync::refresh::{Given, Refreshed, Refresher};
use crate::sync::{Cache, NARROWING_BUDGET};

/// The connections one Gmail account may hold signed in at once, far
/// under Gmail's own cap of fifteen. The requests take one
/// conversation; the header sync keeps one of its own, so a large store
/// never holds up the reading pane; the searches run on the third.
pub const GMAIL_CONNECTIONS: usize = 3;

/// The accounts whose header sync runs at once, process wide.
pub const SYNC_PARALLELISM: usize = 2;

/// The failures one run of a folder may cost before the folder waits
/// for the next round: above the narrowing budget, so a folder full of
/// hostile messages narrows to its end, with room for a flaky link.
pub const FOLDER_FAILURE_BOUND: usize = NARROWING_BUDGET + 16;

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
    /// The session the host opens. It owns its connection, so its
    /// LOGOUT can run on a task of its own.
    type Session: Session + 'static;

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
/// lock, the searches with their own connection and hits and the mailbox
/// the client looked at last.
pub struct Link<C: Connector> {
    pub(crate) wire: tokio::sync::Mutex<Wire<C>>,
    /// On the heap, since a connection more would double the size of
    /// every future that holds a link.
    pub(crate) search: Box<search::Searches<C>>,
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
    /// The commands the server offers for moving mail, read with it.
    moving: Moving,
    /// How the server takes the literals of a search, read with it.
    searching: Searching,
    /// Whether the host's word that the account is a Gmail account holds
    /// on this server, read with it.
    gmail: bool,
    timing: Timing,
    refreshed: Option<Instant>,
    last_used: Option<Instant>,
    /// When the session signed in.
    signed_in: Option<Instant>,
    /// Whether no mailbox pass ran on the session yet.
    unpassed: bool,
    refresher: Refresher,
}

impl<C: Connector + Clone> Link<C> {
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

    /// A link on the given clocks; the searches connect through a copy
    /// of the connector.
    pub fn with_timing(connector: C, timing: Timing) -> Self {
        Self {
            wire: tokio::sync::Mutex::new(Wire::new(connector.clone(), timing)),
            search: Box::new(search::Searches::new(connector, timing)),
            viewed: Mutex::new(None),
            wake: Notify::new(),
        }
    }
}

impl<C: Connector> Link<C> {
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
        wire.close_stale_list();
        if wire.session(cache).await.is_err() {
            return Ok(());
        }
        let given = Given {
            condstore: wire.condstore,
            gmail: wire.gmail,
            viewed: viewed.as_ref(),
            first_pass: std::mem::take(&mut wire.unpassed),
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

    /// Logs the conversation and the search connection out once nothing
    /// used either for `bound`; how long to wait before the next look.
    pub(crate) async fn close_idle(&self, bound: Duration) -> Duration {
        let conversation = self.wire.lock().await.close_idle(bound);
        conversation.min(self.search.close_idle(bound))
    }
}

impl<C: Connector> Wire<C> {
    pub(crate) fn new(connector: C, timing: Timing) -> Self {
        Self {
            connector,
            session: None,
            condstore: false,
            moving: Moving::default(),
            searching: Searching::default(),
            gmail: false,
            timing,
            refreshed: None,
            last_used: None,
            signed_in: None,
            unpassed: true,
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
            self.moving = capabilities.moving();
            self.searching = capabilities.searching();
            self.gmail = gmail::confirmed(cache.gmail, &capabilities);
            self.signed_in = Some(Instant::now());
            self.unpassed = true;
            fresh
        };
        self.last_used = Some(Instant::now());
        Ok(self.session.insert(session))
    }

    /// The session as [`Wire::session`] answers it, with the commands
    /// its server offers for moving mail.
    ///
    /// # Errors
    ///
    /// As [`Wire::session`].
    pub(crate) async fn session_with(
        &mut self,
        cache: &Cache,
    ) -> Result<(&mut C::Session, Moving), ConnectError> {
        self.session(cache).await?;
        let moving = self.moving;
        let session = self
            .session
            .as_mut()
            .ok_or(ConnectError::Failed(SessionError::Closed))?;
        Ok((session, moving))
    }

    /// Drops the session after a failure, since its stream may hold
    /// unread lines; the next use connects again.
    pub(crate) fn drop_session(&mut self) {
        self.session = None;
    }

    /// Logs a Gmail session out once it is older than the list age,
    /// since Gmail shows a connection the folders as they stood when it
    /// signed in; the next use signs in afresh.
    fn close_stale_list(&mut self) {
        let age = self.timing.list_age;
        if self.gmail && self.signed_in.is_some_and(|at| at.elapsed() >= age) {
            self.log_out();
        }
    }

    /// Ends a healthy session with LOGOUT (RFC 9051 section 6.1.3) on a
    /// task of its own, so a server that holds the answer holds up no
    /// request. The connection is gone whatever LOGOUT answers.
    fn log_out(&mut self) {
        if let Some(session) = self.session.take() {
            tokio::spawn(async move {
                let _ended = session.logout().await;
            });
        }
        self.last_used = None;
    }

    /// The session as [`Wire::session`] answers it, taken out for a turn
    /// that hands it back only once every answer was read; a turn that
    /// ends early drops it with its future.
    ///
    /// # Errors
    ///
    /// As [`Wire::session`].
    pub(crate) async fn take(&mut self, cache: &Cache) -> Result<C::Session, ConnectError> {
        self.session(cache).await?;
        self.session
            .take()
            .ok_or(ConnectError::Failed(SessionError::Closed))
    }

    /// Hands back a session [`Wire::take`] took out.
    pub(crate) fn put(&mut self, session: C::Session) {
        self.last_used = Some(Instant::now());
        self.session = Some(session);
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
    pub(crate) fn close_idle(&mut self, bound: Duration) -> Duration {
        let Some(last_used) = self.last_used.filter(|_| self.session.is_some()) else {
            return bound;
        };
        let idle = last_used.elapsed();
        if idle < bound {
            return bound.saturating_sub(idle);
        }
        self.log_out();
        bound
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const _: () = assert!(FOLDER_FAILURE_BOUND > NARROWING_BUDGET);

    #[test]
    fn the_counts_default_to_the_documented_values() {
        assert_eq!(SYNC_PARALLELISM, 2);
        assert_eq!(GMAIL_CONNECTIONS, 3);
    }
}
