// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The recorder: a server on a loopback port that hands one
//! conversation on to a live server and writes both sides down. When the
//! recording is done it comes back with every personal value replaced
//! and checked, or not at all.

pub mod fixed;
pub mod redact;
pub mod scan;

use std::sync::{Arc, MutexGuard, PoisonError};

use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;
use tokio_rustls::rustls::ClientConfig;
use tokio_rustls::rustls::pki_types::ServerName;

use super::transcript::{self, Shared, Transcript};
use super::{Fake, Lines, Listen, Phase, Protocol, read_line, record};
use crate::session::{STEP_TIMEOUT, Target, TlsMode};

/// The live server a recording talks to: where it is and what trust it
/// is checked against. TLS from the first byte; a STARTTLS target is not
/// recorded.
pub struct Upstream {
    pub target: Target,
    pub tls: Arc<ClientConfig>,
}

/// The script of the recorder: the live server and the recording so
/// far.
#[derive(Clone)]
pub struct Script {
    upstream: Arc<Upstream>,
    transcript: Shared,
}

/// The recorder as a server: what connects to it reaches the live
/// server, both sides written down.
pub type Recorder = Fake<Script>;

impl Script {
    /// A recorder in front of the live server.
    #[must_use]
    pub fn new(upstream: Upstream) -> Self {
        Self {
            upstream: Arc::new(upstream),
            transcript: Shared::default(),
        }
    }

    /// The recording with every personal value replaced, once the scan
    /// finds nothing left; the findings otherwise, so a leak never
    /// reaches disk.
    ///
    /// # Errors
    ///
    /// Returns every finding of the scan.
    pub fn finish(&self) -> Result<Transcript, Vec<String>> {
        let redacted = redact::redact(&Transcript::snapshot(&self.transcript));
        let findings = scan::scan(&redacted);
        if findings.is_empty() {
            Ok(redacted)
        } else {
            Err(findings)
        }
    }

    fn transcript(&self) -> MutexGuard<'_, Transcript> {
        self.transcript
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

impl Protocol for Script {
    const HOST: &'static str = super::imap::HOST;

    fn listen(&self) -> Listen {
        Listen::Tls
    }

    async fn converse<S: AsyncRead + AsyncWrite + Unpin + Send>(
        &self,
        phase: Phase,
        lines: &Lines,
        stream: S,
    ) -> Option<S> {
        relay(self, phase, lines, stream).await;
        None
    }
}

/// Both directions of one connection until either side leaves: every
/// command noted and passed on, every answer noted and passed back.
async fn relay<S: AsyncRead + AsyncWrite + Unpin + Send>(
    script: &Script,
    phase: Phase,
    lines: &Lines,
    stream: S,
) -> Option<()> {
    let upstream = connect(&script.upstream).await?;
    let session = script.transcript().start_session();
    let (client_read, mut client_write) = tokio::io::split(stream);
    let (server_read, mut server_write) = tokio::io::split(upstream);
    let mut client_read = BufReader::new(client_read);
    let mut server_read = BufReader::new(server_read);
    let commands = async {
        while let Some(line) = read_line(&mut client_read).await {
            record(lines, phase, &line);
            script.transcript().client(session, line.clone());
            server_write
                .write_all(format!("{line}\r\n").as_bytes())
                .await
                .ok()?;
            server_write.flush().await.ok()?;
        }
        Some(())
    };
    let answers = async {
        while let Some(line) = transcript::read_line(&mut server_read).await {
            script.transcript().server(session, line.clone());
            client_write.write_all(&line.bytes()).await.ok()?;
            client_write.flush().await.ok()?;
        }
        Some(())
    };
    tokio::select! {
        _ = commands => {}
        _ = answers => {}
    }
    Some(())
}

/// The live server over TLS, its addresses tried in order.
async fn connect(upstream: &Upstream) -> Option<TlsStream<TcpStream>> {
    if upstream.target.tls != TlsMode::Implicit {
        return None;
    }
    let name = ServerName::try_from(upstream.target.host.clone()).ok()?;
    let connector = TlsConnector::from(Arc::clone(&upstream.tls));
    for address in &upstream.target.addresses {
        let Ok(Ok(tcp)) = timeout(STEP_TIMEOUT, TcpStream::connect(address)).await else {
            continue;
        };
        return timeout(STEP_TIMEOUT, connector.connect(name.clone(), tcp))
            .await
            .ok()?
            .ok();
    }
    None
}
