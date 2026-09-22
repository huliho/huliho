// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The one IMAP conversation of an account between requests. The host
//! hands out connections; the bridge never sees a credential and never
//! resolves a host. A request takes the conversation only for a refresh
//! or a preview fetch, so one that reads the cache never waits on IMAP.

use std::future::Future;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::gmail;
use crate::mailboxes::SyncError;
use crate::session::{Session, SessionError};
use crate::store::{AccountKey, MailboxId};
use crate::sync::Cache;
use crate::sync::refresh::{Given, Refresher};

/// A `/changes` call refreshes its account at most this often; until
/// the server pushes, a client polls about twice as slowly.
pub const REFRESH_INTERVAL: Duration = Duration::from_secs(30);

/// The connections one Gmail account may hold at once, far under
/// Gmail's own cap of fifteen; the host's runtime enforces it, since
/// the bridge opens no connection itself.
pub const GMAIL_CONNECTIONS: usize = 2;

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
    ) -> impl Future<Output = Result<Self::Session, SessionError>> + Send;
}

/// What the requests of one account share: the conversation behind a
/// lock and the mailbox the client looked at last.
pub struct Link<C: Connector> {
    pub(crate) wire: tokio::sync::Mutex<Wire<C>>,
    viewed: Mutex<Option<MailboxId>>,
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
    interval: Duration,
    refreshed: Option<Instant>,
    refresher: Refresher,
}

impl<C: Connector> Link<C> {
    /// A link that refreshes at most once per `REFRESH_INTERVAL`.
    pub fn new(connector: C) -> Self {
        Self::with_interval(connector, REFRESH_INTERVAL)
    }

    /// A link with an interval of its own.
    pub fn with_interval(connector: C, interval: Duration) -> Self {
        Self {
            wire: tokio::sync::Mutex::new(Wire {
                connector,
                session: None,
                condstore: false,
                gmail: false,
                interval,
                refreshed: None,
                refresher: Refresher::default(),
            }),
            viewed: Mutex::new(None),
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

    /// Refreshes the account when one is due. A connection that cannot
    /// be opened leaves the cache as it is; the host's connector saw the
    /// failure.
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
        let Wire {
            session, refresher, ..
        } = &mut *wire;
        let Some(session) = session.as_mut() else {
            return Ok(());
        };
        if !refresher.refresh(session, cache, given).await? {
            wire.drop_session();
        }
        Ok(())
    }
}

impl<C: Connector> Wire<C> {
    /// The session of the account: the one kept from an earlier use
    /// while it still answers NOOP, a fresh one otherwise.
    ///
    /// # Errors
    ///
    /// Returns the connector's failure or the failure of the first read
    /// on a fresh session.
    pub(crate) async fn session(&mut self, cache: &Cache) -> Result<&mut C::Session, SessionError> {
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
        Ok(self.session.insert(session))
    }

    /// Drops the session after a failure, since its stream may hold
    /// unread lines; the next use connects again.
    pub(crate) fn drop_session(&mut self) {
        self.session = None;
    }

    /// Whether a refresh may run now; one that may counts as run,
    /// whatever comes of it, so a server that fails is not asked again
    /// before the interval is over.
    fn due(&mut self) -> bool {
        let now = Instant::now();
        let due = self
            .refreshed
            .is_none_or(|last| now.duration_since(last) >= self.interval);
        if due {
            self.refreshed = Some(now);
        }
        due
    }
}
