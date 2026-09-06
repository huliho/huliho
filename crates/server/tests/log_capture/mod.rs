// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Captures every log line of the test binary, so a test can prove a
//! secret never reached one.

use std::io::{self, Write};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;

/// The instance's own crates at every level, everything else at the
/// level the binary runs on by default.
const FILTER: &str = "info,huliho_server=trace,huliho_imap_bridge=trace";

/// The lines every subscriber in this binary writes.
#[derive(Clone, Default)]
pub struct Capture(Arc<Mutex<Vec<u8>>>);

impl Capture {
    /// Installs the capture once per process and hands it back.
    pub fn install() -> Self {
        static CAPTURE: OnceLock<Capture> = OnceLock::new();
        CAPTURE
            .get_or_init(|| {
                let capture = Self::default();
                tracing_subscriber::fmt()
                    .with_env_filter(EnvFilter::new(FILTER))
                    .with_ansi(false)
                    .with_writer(capture.clone())
                    .init();
                capture
            })
            .clone()
    }

    /// Everything logged so far.
    pub fn text(&self) -> String {
        let bytes = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

impl Write for Capture {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Capture {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}
