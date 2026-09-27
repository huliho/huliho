// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What the proxy keeps per account between requests: the cap on the
//! requests in flight (the native path and the bridge alike), the cap
//! on the downloads in flight and what the upstream session object
//! named.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use url::Url;

use super::MAX_CONCURRENT_REQUESTS;
use crate::ids::AccountId;
use crate::mail::download::MAX_CONCURRENT_DOWNLOADS;

/// What the proxy keeps per account.
#[derive(Default)]
pub struct Endpoints {
    memory: Mutex<HashMap<AccountId, Endpoint>>,
}

/// What the upstream session object named, checked and remembered: the
/// API endpoint and the download template as the upstream wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct UpstreamUrls {
    pub api_url: Url,
    pub download_url: Option<String>,
}

struct Endpoint {
    requests: Arc<Semaphore>,
    /// Beside the request cap, so a long download starves no request.
    downloads: Arc<Semaphore>,
    urls: Option<UpstreamUrls>,
}

impl Default for Endpoint {
    fn default() -> Self {
        Self {
            requests: Arc::new(Semaphore::new(MAX_CONCURRENT_REQUESTS)),
            downloads: Arc::new(Semaphore::new(MAX_CONCURRENT_DOWNLOADS)),
            urls: None,
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

    /// The account's download lane, which a caller waits on for a
    /// permit.
    #[must_use]
    pub fn downloads(&self, account_id: &AccountId) -> Arc<Semaphore> {
        let mut memory = self.memory();
        Arc::clone(&memory.entry(account_id.clone()).or_default().downloads)
    }

    /// Drops what the proxy remembers of an account once its row left.
    pub fn forget(&self, account_id: &AccountId) {
        self.memory().remove(account_id);
    }

    pub(super) fn upstream_urls(&self, account_id: &AccountId) -> Option<UpstreamUrls> {
        self.memory()
            .get(account_id)
            .and_then(|endpoint| endpoint.urls.clone())
    }

    pub(super) fn remember(&self, account_id: &AccountId, urls: UpstreamUrls) {
        self.memory().entry(account_id.clone()).or_default().urls = Some(urls);
    }

    fn memory(&self) -> MutexGuard<'_, HashMap<AccountId, Endpoint>> {
        self.memory.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(api_url: &str) -> UpstreamUrls {
        UpstreamUrls {
            api_url: api_url.parse().unwrap(),
            download_url: Some("https://api.example.test/jmap/download/{blobId}".to_owned()),
        }
    }

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
    fn the_download_lane_holds_two_beside_the_request_cap() {
        let endpoints = Endpoints::default();
        let alpha = AccountId::from("alpha".to_owned());
        let lane = endpoints.downloads(&alpha);
        assert_eq!(lane.available_permits(), MAX_CONCURRENT_DOWNLOADS);
        let held: Vec<_> = (0..MAX_CONCURRENT_DOWNLOADS)
            .map(|_| lane.clone().try_acquire_owned().unwrap())
            .collect();
        assert!(lane.clone().try_acquire_owned().is_err());
        assert!(endpoints.enter(&alpha).is_some());
        drop(held);
        assert!(Arc::ptr_eq(&lane, &endpoints.downloads(&alpha)));
    }

    #[test]
    fn what_the_upstream_named_leaves_with_the_account() {
        let endpoints = Endpoints::default();
        let account = AccountId::from("alpha".to_owned());
        assert_eq!(endpoints.upstream_urls(&account), None);
        let urls = named("https://api.example.test/jmap/api");
        endpoints.remember(&account, urls.clone());
        assert_eq!(endpoints.upstream_urls(&account), Some(urls));
        endpoints.forget(&account);
        assert_eq!(endpoints.upstream_urls(&account), None);
    }
}
