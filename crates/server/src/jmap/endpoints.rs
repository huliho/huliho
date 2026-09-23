// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What the proxy keeps per account between requests: the cap on the
//! requests in flight (the native path and the bridge alike) and the
//! upstream API endpoint the session object named.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use url::Url;

use super::MAX_CONCURRENT_REQUESTS;
use crate::ids::AccountId;

/// What the proxy keeps per account: the requests in flight and the
/// upstream API endpoint its session object named.
#[derive(Default)]
pub struct Endpoints {
    memory: Mutex<HashMap<AccountId, Endpoint>>,
}

struct Endpoint {
    requests: Arc<Semaphore>,
    api_url: Option<Url>,
}

impl Default for Endpoint {
    fn default() -> Self {
        Self {
            requests: Arc::new(Semaphore::new(MAX_CONCURRENT_REQUESTS)),
            api_url: None,
        }
    }
}

impl Endpoints {
    /// A permit for one request on the account; `None` past the cap.
    #[must_use]
    pub fn enter(&self, account_id: &AccountId) -> Option<OwnedSemaphorePermit> {
        let requests = {
            let mut memory = self.memory();
            Arc::clone(&memory.entry(account_id.clone()).or_default().requests)
        };
        requests.try_acquire_owned().ok()
    }

    /// Drops what the proxy remembers of an account once its row left.
    pub fn forget(&self, account_id: &AccountId) {
        self.memory().remove(account_id);
    }

    pub(super) fn api_url(&self, account_id: &AccountId) -> Option<Url> {
        self.memory()
            .get(account_id)
            .and_then(|endpoint| endpoint.api_url.clone())
    }

    pub(super) fn remember(&self, account_id: &AccountId, api_url: Url) {
        self.memory().entry(account_id.clone()).or_default().api_url = Some(api_url);
    }

    fn memory(&self) -> MutexGuard<'_, HashMap<AccountId, Endpoint>> {
        self.memory.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fifth_permit_on_one_account_is_refused_and_another_account_is_untouched() {
        let endpoints = Endpoints::default();
        let (alpha, beta) = (
            AccountId::from("alpha".to_owned()),
            AccountId::from("beta".to_owned()),
        );
        let held: Vec<_> = (0..MAX_CONCURRENT_REQUESTS)
            .map(|_| endpoints.enter(&alpha).unwrap())
            .collect();
        assert!(endpoints.enter(&alpha).is_none());
        assert!(endpoints.enter(&beta).is_some());
        drop(held);
        assert!(endpoints.enter(&alpha).is_some());
    }

    #[test]
    fn a_remembered_endpoint_leaves_with_the_account() {
        let endpoints = Endpoints::default();
        let account = AccountId::from("alpha".to_owned());
        let url: Url = "https://api.example.test/jmap/api".parse().unwrap();
        assert_eq!(endpoints.api_url(&account), None);
        endpoints.remember(&account, url.clone());
        assert_eq!(endpoints.api_url(&account), Some(url));
        endpoints.forget(&account);
        assert_eq!(endpoints.api_url(&account), None);
    }
}
