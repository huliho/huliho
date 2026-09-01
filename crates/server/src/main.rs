// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Binary entry: load config, set up tracing, serve until shutdown.

use std::net::SocketAddr;
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

use tokio::signal::unix::{SignalKind, signal};
use tracing_subscriber::EnvFilter;

use huliho_server::api::{ApiState, MAX_CONCURRENT_VERIFICATIONS};
use huliho_server::config::{CONFIG_PATH_VAR, Config, DEFAULT_CONFIG_PATH};
use huliho_server::rate::RateLimiter;
use huliho_server::secrets::{InstanceSecret, SessionKeys};
use huliho_server::session::SessionTimeouts;
use huliho_server::store::Store;
use huliho_server::{events, session};

/// Request logs without debug noise; override via `RUST_LOG`.
const DEFAULT_LOG_FILTER: &str = "info";

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG_FILTER)),
        )
        .init();

    let config = match std::env::var_os(CONFIG_PATH_VAR) {
        Some(path) => Config::load(Path::new(&path))?,
        None => Config::load_or_default(Path::new(DEFAULT_CONFIG_PATH))?,
    };

    let secret = InstanceSecret::load(config.auth.secret_file.as_deref())?;
    let store = Arc::new(Store::open(&config.storage.path)?);
    let timeouts = SessionTimeouts::from(&config.auth);
    tokio::spawn(events::prune_periodically(
        Arc::clone(&store),
        config.events.retention_days,
    ));
    tokio::spawn(session::prune_periodically(Arc::clone(&store), timeouts));

    let api = ApiState {
        store,
        keys: Arc::new(SessionKeys::derive(&secret)),
        timeouts,
        limiter: Arc::new(RateLimiter::default()),
        verify_gate: Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_VERIFICATIONS)),
    };
    let listener = tokio::net::TcpListener::bind(config.listen).await?;
    tracing::info!(listen = %config.listen, "listening");
    axum::serve(
        listener,
        huliho_server::app::router(&config.assets, api)
            .into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    Ok(())
}

async fn shutdown_signal() {
    let mut terminate = signal(SignalKind::terminate()).expect("SIGTERM handler installs");
    tokio::select! {
        result = tokio::signal::ctrl_c() => result.expect("ctrl-c handler installs"),
        _ = terminate.recv() => {}
    }
}
