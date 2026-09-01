// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Shared fixtures for the HTTP integration tests.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;

use huliho_server::api::ApiState;
use huliho_server::config::AuthConfig;
use huliho_server::rate::RateLimiter;
use huliho_server::secrets::{InstanceSecret, SessionKeys};
use huliho_server::session::SessionTimeouts;
use huliho_server::store::Store;

fn session_keys() -> SessionKeys {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secret");
    let mut file = std::fs::File::create(&path).unwrap();
    file.write_all(b"0123456789abcdef0123456789abcdef").unwrap();
    set_owner_only(&path);
    SessionKeys::derive(&InstanceSecret::load(Some(&path)).unwrap())
}

fn set_owner_only(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
}

/// The state the router runs on, over the given store with default
/// timeouts; tests reach the verification gate through it.
pub fn api_state(store: Arc<Store>) -> ApiState {
    ApiState {
        store,
        keys: Arc::new(session_keys()),
        timeouts: SessionTimeouts::from(&AuthConfig::default()),
        limiter: Arc::new(RateLimiter::default()),
        verify_gate: Arc::new(tokio::sync::Semaphore::new(
            huliho_server::api::MAX_CONCURRENT_VERIFICATIONS,
        )),
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
