// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The bridge as the host runs it: one store, one sealer, one connector,
//! one store of uploads and, per account, the conversation a request
//! takes and the task that syncs its folders.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, Once, PoisonError};

use tokio::sync::Semaphore;
use tokio::task::JoinHandle;

use super::worker;
use super::{Connector, GMAIL_CONNECTIONS, Link, SYNC_PARALLELISM, Timing, Wire};
use crate::blob::{self, Blob, BlobError};
use crate::jmap::{self, RequestError};
use crate::seal::Sealer;
use crate::store::{AccountKey, Store};
use crate::sync::Cache;
use crate::upload::{UploadError, Uploads, Writing};

/// What the host hands the bridge beside its connector: the store it
/// opened with the schema present, its sealer and the directory the
/// uploads live under.
pub struct Host {
    pub store: Arc<Store>,
    pub sealer: Arc<dyn Sealer>,
    pub uploads: PathBuf,
}

/// What the host says about an account it hands to the bridge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registration {
    pub key: AccountKey,
    /// The host's word that the account is a Gmail account; it holds
    /// once the server advertises the extension.
    pub gmail: bool,
    /// The state of the session object, derived by the host from what
    /// the object is built from, so it never moves with the cache.
    pub session_state: String,
}

/// The bridge: what every account shares and the accounts it runs.
pub struct Bridge<C: Connector> {
    store: Arc<Store>,
    sealer: Arc<dyn Sealer>,
    uploads: Arc<Uploads>,
    connector: Arc<C>,
    timing: Timing,
    /// The accounts whose header sync runs at once, process wide.
    syncing: Arc<Semaphore>,
    accounts: Mutex<HashMap<AccountKey, Entry<C>>>,
    /// Whether the sweep of the uploads runs.
    sweeping: Once,
}

struct Entry<C: Connector> {
    account: Arc<Account<C>>,
    task: JoinHandle<()>,
}

/// One account as the runtime holds it.
pub(super) struct Account<C: Connector> {
    pub(super) cache: Cache,
    pub(super) session_state: String,
    /// The conversation a request takes for a refresh, a preview fetch
    /// or a body fetch and a blob takes for each of its windows; the
    /// header sync shares it, one batch per turn.
    pub(super) link: Arc<Link<Arc<C>>>,
    /// A conversation of the header sync's own, where the account may
    /// hold a second connection.
    pub(super) own: Option<tokio::sync::Mutex<Wire<Arc<C>>>>,
}

impl<C: Connector> Account<C> {
    /// The conversation the header sync runs on.
    pub(super) fn sync_wire(&self) -> &tokio::sync::Mutex<Wire<Arc<C>>> {
        self.own.as_ref().unwrap_or(&self.link.wire)
    }
}

impl<C: Connector + 'static> Bridge<C> {
    /// A bridge over what the host hands it, on the runtime's clocks.
    #[must_use]
    pub fn open(host: Host, connector: C) -> Self {
        Self::with_timing(host, connector, Timing::default())
    }

    /// A bridge on the given clocks.
    #[must_use]
    pub fn with_timing(host: Host, connector: C, timing: Timing) -> Self {
        let Host {
            store,
            sealer,
            uploads,
        } = host;
        Self {
            uploads: Arc::new(Uploads::new(uploads, Arc::clone(&sealer))),
            store,
            sealer,
            connector: Arc::new(connector),
            timing,
            syncing: Arc::new(Semaphore::new(SYNC_PARALLELISM)),
            accounts: Mutex::new(HashMap::new()),
            sweeping: Once::new(),
        }
    }

    /// The store the bridge answers from.
    #[must_use]
    pub fn store(&self) -> &Arc<Store> {
        &self.store
    }

    /// Starts an upload of the account into the store; the account's
    /// server is not asked.
    ///
    /// # Errors
    ///
    /// As [`Uploads::begin`].
    pub async fn upload(&self, key: &AccountKey, media_type: &str) -> Result<Writing, UploadError> {
        self.uploads.begin(key, media_type).await
    }

    /// Starts the account's runtime when it is not running: a task that
    /// syncs every store folder whose first sync is not done and keeps
    /// the conversation. An account that runs is left as it is.
    pub fn start(&self, registration: &Registration) {
        self.account(registration);
    }

    /// Starts every account the host lists at startup; the first call
    /// also starts the sweep of the uploads.
    pub fn resume(&self, registrations: impl IntoIterator<Item = Registration>) {
        self.sweeping.call_once(|| {
            tokio::spawn(Arc::clone(&self.uploads).sweep_periodically());
        });
        for registration in registrations {
            self.start(&registration);
        }
    }

    /// One Request object against the account, started when it was not.
    ///
    /// # Errors
    ///
    /// As [`jmap::handle`].
    pub async fn handle(
        &self,
        registration: &Registration,
        body: &[u8],
    ) -> Result<Vec<u8>, RequestError> {
        let account = self.account(registration);
        jmap::handle(&account.cache, &account.link, body, &account.session_state).await
    }

    /// One blob of the account, started when it was not: a whole
    /// message or one of its parts, within `limit` octets.
    ///
    /// # Errors
    ///
    /// As [`blob::open`].
    pub async fn blob(
        &self,
        registration: &Registration,
        blob_id: &str,
        limit: u64,
    ) -> Result<Blob, BlobError> {
        let account = self.account(registration);
        blob::open(&account.cache, &account.link, blob_id, limit).await
    }

    /// Stops the account's runtime: its task ends, a blob still on its
    /// way breaks off, its uploads leave and the store writes nothing
    /// more for the key, so the host can delete the rows in the
    /// transaction that removes the account.
    pub async fn forget(&self, key: &AccountKey) {
        let entry = self.accounts().remove(key);
        self.store.forget(key);
        if let Some(entry) = entry {
            entry.task.abort();
            let _ended = entry.task.await;
        }
        self.uploads.forget(key).await;
    }

    fn account(&self, registration: &Registration) -> Arc<Account<C>> {
        let mut accounts = self.accounts();
        if let Some(entry) = accounts.get(&registration.key) {
            return Arc::clone(&entry.account);
        }
        let connector = Arc::clone(&self.connector);
        let own = (registration.gmail && GMAIL_CONNECTIONS > 1)
            .then(|| tokio::sync::Mutex::new(Wire::new(Arc::clone(&connector), self.timing)));
        let account = Arc::new(Account {
            cache: Cache {
                store: Arc::clone(&self.store),
                sealer: Arc::clone(&self.sealer),
                key: registration.key.clone(),
                gmail: registration.gmail,
                uploads: Arc::clone(&self.uploads),
            },
            session_state: registration.session_state.clone(),
            link: Arc::new(Link::with_timing(connector, self.timing)),
            own,
        });
        let task = tokio::spawn(worker::run(
            Arc::clone(&account),
            Arc::clone(&self.syncing),
            self.timing,
        ));
        accounts.insert(
            registration.key.clone(),
            Entry {
                account: Arc::clone(&account),
                task,
            },
        );
        account
    }

    fn accounts(&self) -> MutexGuard<'_, HashMap<AccountKey, Entry<C>>> {
        self.accounts.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
