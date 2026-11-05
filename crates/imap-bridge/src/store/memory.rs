// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What the store keeps in memory per account beside its tables: no
//! column holds it. A counter names each state behind a mark drawn when
//! the store opens, so a state from before a restart is one this store
//! never issued.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use uuid::Uuid;

use super::AccountKey;

/// What every account holds and the mark of this store. Its lock is
/// never held together with another lock of the store.
pub(super) struct InMemory<T> {
    mark: String,
    accounts: Mutex<HashMap<AccountKey, T>>,
}

impl<T> InMemory<T> {
    pub(super) fn new() -> Self {
        Self {
            mark: Uuid::new_v4().simple().to_string(),
            accounts: Mutex::new(HashMap::new()),
        }
    }

    pub(super) fn lock(&self) -> MutexGuard<'_, HashMap<AccountKey, T>> {
        self.accounts.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(super) fn forget(&self, key: &AccountKey) {
        self.lock().remove(key);
    }

    /// The state the counter names.
    pub(super) fn state(&self, counter: u64) -> String {
        format!("{}-{counter}", self.mark)
    }
}
