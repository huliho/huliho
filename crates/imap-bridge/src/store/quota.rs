// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The quota of each account as the last mailbox pass read it, kept in
//! memory: no table holds it and the next pass reads it again. A
//! counter names every reading that differs from the one before it,
//! behind a mark drawn when the store opens, so a state from before a
//! restart is one this store never issued.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use uuid::Uuid;

use super::{AccountKey, Store};
use crate::session::{QuotaRoot, Usage};

/// What a quota measures (RFC 9208 sections 5.1 and 5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resource {
    /// STORAGE, in octets.
    Storage,
    /// MESSAGE, a count of messages.
    Messages,
}

/// One resource of one quota root, under the id the store gave it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotaRow {
    pub id: String,
    pub root: String,
    pub resource: Resource,
    pub usage: Usage,
}

/// The quota of an account as the store holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotaSnapshot {
    /// Whether the server advertises QUOTA, as the last pass read it;
    /// false before the first.
    pub advertised: bool,
    /// The state the rows belong to.
    pub state: String,
    pub rows: Vec<QuotaRow>,
}

/// The readings of every account and the mark of this store.
pub(super) struct Quotas {
    mark: String,
    accounts: Mutex<HashMap<AccountKey, Kept>>,
}

#[derive(Debug, Default)]
struct Kept {
    advertised: bool,
    counter: u64,
    rows: Vec<QuotaRow>,
}

impl Quotas {
    pub(super) fn new() -> Self {
        Self {
            mark: Uuid::new_v4().simple().to_string(),
            accounts: Mutex::new(HashMap::new()),
        }
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<AccountKey, Kept>> {
        self.accounts.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(super) fn forget(&self, key: &AccountKey) {
        self.lock().remove(key);
    }

    fn state(&self, counter: u64) -> String {
        format!("{}-{counter}", self.mark)
    }
}

impl Store {
    /// Keeps what a pass read of the account's quota: the roots of the
    /// inbox where the server advertises QUOTA, `None` where it does
    /// not. A reading that differs from the one kept moves the state; a
    /// resource keeps its id while its root names it. A forgotten
    /// account keeps nothing.
    pub fn record_quota(&self, key: &AccountKey, roots: Option<&[QuotaRoot]>) {
        if self.is_forgotten(key) {
            return;
        }
        let mut accounts = self.quotas.lock();
        let kept = accounts.entry(key.clone()).or_default();
        let rows = roots.map_or_else(Vec::new, |roots| rows_of(roots, &kept.rows));
        let advertised = roots.is_some();
        if advertised != kept.advertised || rows != kept.rows {
            kept.advertised = advertised;
            kept.rows = rows;
            kept.counter += 1;
        }
    }

    /// The account's quota as the last pass left it.
    #[must_use]
    pub fn quota(&self, key: &AccountKey) -> QuotaSnapshot {
        let accounts = self.quotas.lock();
        let kept = accounts.get(key);
        QuotaSnapshot {
            advertised: kept.is_some_and(|kept| kept.advertised),
            state: self.quotas.state(kept.map_or(0, |kept| kept.counter)),
            rows: kept.map(|kept| kept.rows.clone()).unwrap_or_default(),
        }
    }
}

/// One row per resource a root names, storage before messages, under
/// the id an earlier reading gave that root and resource.
fn rows_of(roots: &[QuotaRoot], kept: &[QuotaRow]) -> Vec<QuotaRow> {
    let mut rows = Vec::new();
    for root in roots {
        let resources = [
            (Resource::Storage, root.storage),
            (Resource::Messages, root.messages),
        ];
        for (resource, usage) in resources {
            let Some(usage) = usage else {
                continue;
            };
            let id = kept
                .iter()
                .find(|row| row.root == root.name && row.resource == resource)
                .map_or_else(|| format!("q{}", Uuid::new_v4()), |row| row.id.clone());
            rows.push(QuotaRow {
                id,
                root: root.name.clone(),
                resource,
                usage,
            });
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> AccountKey {
        AccountKey::new("a1")
    }

    fn root(name: &str, used: u64, messages: Option<u64>) -> QuotaRoot {
        QuotaRoot {
            name: name.to_owned(),
            storage: Some(Usage {
                used,
                limit: 1024 * 1024,
            }),
            messages: messages.map(|used| Usage { used, limit: 100 }),
        }
    }

    #[test]
    fn an_account_no_pass_has_read_shows_no_quota() {
        let store = Store::in_memory().unwrap();
        let snapshot = store.quota(&key());
        assert!(!snapshot.advertised);
        assert!(snapshot.rows.is_empty(), "{:?}", snapshot.rows);
        assert!(snapshot.state.ends_with("-0"), "{}", snapshot.state);
    }

    #[test]
    fn the_state_moves_with_a_reading_that_differs_and_an_id_stays_with_its_root() {
        let store = Store::in_memory().unwrap();
        store.record_quota(&key(), Some(&[root("User quota", 10, Some(1))]));
        let first = store.quota(&key());
        assert!(first.advertised);
        let resources: Vec<Resource> = first.rows.iter().map(|row| row.resource).collect();
        assert_eq!(resources, [Resource::Storage, Resource::Messages]);
        assert_ne!(first.rows[0].id, first.rows[1].id);
        store.record_quota(&key(), Some(&[root("User quota", 10, Some(1))]));
        assert_eq!(store.quota(&key()), first);
        store.record_quota(&key(), Some(&[root("User quota", 20, Some(1))]));
        let second = store.quota(&key());
        assert_ne!(second.state, first.state);
        assert_eq!(second.rows[0].id, first.rows[0].id);
        assert_eq!(second.rows[0].usage.used, 20);
        store.record_quota(&key(), Some(&[root("Other", 20, None)]));
        let renamed = store.quota(&key());
        assert_eq!(renamed.rows.len(), 1);
        assert_ne!(renamed.rows[0].id, first.rows[0].id);
    }

    #[test]
    fn a_server_that_stops_advertising_quota_leaves_none_and_moves_the_state() {
        let store = Store::in_memory().unwrap();
        store.record_quota(&key(), Some(&[]));
        let empty = store.quota(&key());
        assert!(empty.advertised && empty.rows.is_empty());
        store.record_quota(&key(), None);
        let gone = store.quota(&key());
        assert!(!gone.advertised && gone.rows.is_empty());
        assert_ne!(gone.state, empty.state);
    }

    #[test]
    fn two_stores_never_issue_the_same_state_and_a_forgotten_account_keeps_nothing() {
        let (one, two) = (Store::in_memory().unwrap(), Store::in_memory().unwrap());
        assert_ne!(one.quota(&key()).state, two.quota(&key()).state);
        one.record_quota(&key(), Some(&[root("", 1, None)]));
        one.forget(&key());
        one.record_quota(&key(), Some(&[root("", 1, None)]));
        assert!(!one.quota(&key()).advertised);
    }
}
