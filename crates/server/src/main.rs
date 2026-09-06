// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Binary entry: load config, set up tracing, serve until shutdown.

use std::net::SocketAddr;
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

use tokio::signal::unix::{SignalKind, signal};
use tracing_subscriber::filter::{LevelFilter, Targets};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;
use tracing_subscriber::{EnvFilter, Layer as _};

use huliho_server::api::{ApiState, MAX_CONCURRENT_VERIFICATIONS};
use huliho_server::config::{CONFIG_PATH_VAR, Config, DEFAULT_CONFIG_PATH};
use huliho_server::rate::RateLimiter;
use huliho_server::secrets::{InstanceSecret, Keys};
use huliho_server::session::SessionTimeouts;
use huliho_server::store::Store;
use huliho_server::upstream::Upstream;
use huliho_server::{events, session};

/// Request logs without debug noise; override via `RUST_LOG`.
const DEFAULT_LOG_FILTER: &str = "info";

/// The IMAP client library prints every command it sends at trace
/// level, LOGIN included; nothing in `RUST_LOG` lifts this ceiling.
const CLIENT_LIBRARY_TARGET: &str = "async_imap";
const CLIENT_LIBRARY_CEILING: LevelFilter = LevelFilter::DEBUG;

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
    let requested =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG_FILTER));
    subscriber(requested, std::io::stdout).init();

    let config = match std::env::var_os(CONFIG_PATH_VAR) {
        Some(path) => Config::load(Path::new(&path))?,
        None => Config::load_or_default(Path::new(DEFAULT_CONFIG_PATH))?,
    };

    let secret = InstanceSecret::load(config.auth.secret_file.as_deref())?;
    let store = Arc::new(Store::open(&config.storage.path)?);
    let timeouts = SessionTimeouts::from(&config.auth);
    let upstream = Arc::new(Upstream::new(&config.upstream)?);
    tokio::spawn(events::prune_periodically(
        Arc::clone(&store),
        config.events.retention_days,
    ));
    tokio::spawn(session::prune_periodically(Arc::clone(&store), timeouts));

    let api = ApiState {
        store,
        keys: Arc::new(Keys::derive(&secret)),
        timeouts,
        limiter: Arc::new(RateLimiter::default()),
        verify_gate: Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_VERIFICATIONS)),
        probe_interval_minutes: config.upstream.probe_interval_minutes,
        public_url: config.public_url.clone(),
        upstream,
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

/// The log output: what the filter asks for, under the ceiling the
/// client library needs.
fn subscriber<W>(
    requested: EnvFilter,
    writer: W,
) -> impl tracing::Subscriber + Send + Sync + 'static
where
    W: for<'a> MakeWriter<'a> + Send + Sync + 'static,
{
    let ceiling = Targets::new()
        .with_target(CLIENT_LIBRARY_TARGET, CLIENT_LIBRARY_CEILING)
        .with_default(LevelFilter::TRACE);
    tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .with_writer(writer)
            .with_filter(requested)
            .with_filter(ceiling),
    )
}

async fn shutdown_signal() {
    let mut terminate = signal(SignalKind::terminate()).expect("SIGTERM handler installs");
    tokio::select! {
        result = tokio::signal::ctrl_c() => result.expect("ctrl-c handler installs"),
        _ = terminate.recv() => {}
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tracing_subscriber::fmt::writer::MutexGuardWriter;

    use super::*;

    /// A writer the test can read back once the subscriber is gone.
    struct Capture(Arc<Mutex<Vec<u8>>>);

    impl<'a> MakeWriter<'a> for Capture {
        type Writer = MutexGuardWriter<'a, Vec<u8>>;

        fn make_writer(&'a self) -> Self::Writer {
            self.0.make_writer()
        }
    }

    /// Everything the subscriber writes while `emit` runs under it.
    fn logged(filter: &str, emit: impl FnOnce()) -> String {
        let written = Arc::new(Mutex::new(Vec::new()));
        let subscriber = subscriber(EnvFilter::new(filter), Capture(Arc::clone(&written)));
        tracing::subscriber::with_default(subscriber, emit);
        String::from_utf8(written.lock().unwrap().clone()).unwrap()
    }

    #[test]
    fn the_client_library_trace_stays_off_whatever_the_filter_asks() {
        let text = logged("async_imap::imap_stream=trace", || {
            tracing::trace!(target: "async_imap::imap_stream", "C: a1 LOGIN sanne secret");
            tracing::debug!(target: "async_imap::imap_stream", "stream opened");
            tracing::trace!(target: "huliho_server::probe", "the server's own trace");
        });
        assert!(!text.contains("LOGIN"), "{text}");
        assert!(text.contains("stream opened"), "{text}");
        assert!(!text.contains("own trace"), "{text}");
    }

    #[test]
    fn the_default_filter_shows_info_and_hides_debug() {
        let text = logged(DEFAULT_LOG_FILTER, || {
            tracing::debug!("quiet");
            tracing::info!("loud");
        });
        assert!(!text.contains("quiet"), "{text}");
        assert!(text.contains("loud"), "{text}");
    }
}
