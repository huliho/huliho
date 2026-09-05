// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Shared fixtures for the HTTP integration tests.

use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;

use huliho_server::api::ApiState;
use huliho_server::config::{AuthConfig, UpstreamConfig};
use huliho_server::rate::RateLimiter;
use huliho_server::secrets::{InstanceSecret, Keys};
use huliho_server::session::SessionTimeouts;
use huliho_server::store::Store;
use huliho_server::upstream::Upstream;

/// The instance secret every test router derives its keys from; a row
/// sealed under the same bytes elsewhere opens inside the router.
const SECRET: &[u8] = b"0123456789abcdef0123456789abcdef";

fn keys() -> Keys {
    Keys::derive(&InstanceSecret::from_bytes(SECRET.to_vec()).unwrap())
}

/// The state the router runs on, over the given store with default
/// timeouts; tests reach the verification gate through it.
pub fn api_state(store: Arc<Store>) -> ApiState {
    ApiState {
        store,
        keys: Arc::new(keys()),
        timeouts: SessionTimeouts::from(&AuthConfig::default()),
        limiter: Arc::new(RateLimiter::default()),
        verify_gate: Arc::new(tokio::sync::Semaphore::new(
            huliho_server::api::MAX_CONCURRENT_VERIFICATIONS,
        )),
        probe_interval_minutes: UpstreamConfig::default().probe_interval_minutes,
        public_url: None,
        upstream: Arc::new(Upstream::new(&UpstreamConfig::default()).unwrap()),
    }
}

/// The full application router over the given store, on default timeouts.
pub fn router_on(store: Arc<Store>) -> Router {
    router_with(api_state(store))
}

/// The full application router on prepared state.
pub fn router_with(api: ApiState) -> Router {
    let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/spa");
    huliho_server::app::router(&assets, api)
}
