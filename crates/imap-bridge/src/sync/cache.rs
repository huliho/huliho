// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Where a sync writes and how a store call leaves the runtime.

use std::sync::Arc;

use crate::mailboxes::SyncError;
use crate::seal::Sealer;
use crate::store::{AccountKey, Store, StoreError};

/// Where a sync writes: the store, the host's sealer and the account.
#[derive(Clone)]
pub struct Cache {
    pub store: Arc<Store>,
    pub sealer: Arc<dyn Sealer>,
    pub key: AccountKey,
    /// The host's word that the account is a Gmail account; it holds
    /// once the server advertises `X-GM-EXT-1`.
    pub gmail: bool,
}

/// Runs a store call off the runtime.
pub(crate) async fn blocking<T: Send + 'static>(
    call: impl FnOnce() -> Result<T, StoreError> + Send + 'static,
) -> Result<T, SyncError> {
    tokio::task::spawn_blocking(call)
        .await
        .map_err(|_join| SyncError::Task)?
        .map_err(SyncError::from)
}
