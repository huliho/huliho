// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What a rig reads back from the store after a request: the row's stop
//! cause and the account events.

use huliho_server::accounts;
use huliho_server::events;
use huliho_server::scope::Scope;
use huliho_server::store::Store;

/// The row's stop cause word; `None` while it runs.
pub fn stopped_cause(store: &Store, scope: &Scope) -> Option<String> {
    accounts::get(store, scope)
        .unwrap()
        .stopped_cause
        .map(|cause| cause.as_str().to_owned())
}

/// The account events as `(type, actor)`, oldest first.
pub fn account_events(store: &Store, scope: &Scope) -> Vec<(String, String)> {
    events::for_organization(store, scope)
        .unwrap()
        .into_iter()
        .filter(|record| record.event_type.starts_with("account."))
        .map(|record| (record.event_type, record.actor))
        .collect()
}
