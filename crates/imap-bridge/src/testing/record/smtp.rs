// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The SMTP recorder: a server on a loopback port that hands one send on
//! to a live submission server and writes both sides down. Without
//! pipelining SMTP runs in lock step (RFC 5321 section 4.1.1), so the
//! relay reads one command, passes it on, reads the reply whole and
//! passes it back; the message after a DATA the server took is noted as
//! a piece of its own, its dots unstuffed.

use std::sync::{Arc, MutexGuard, PoisonError};

use tokio::io::{AsyncRead, AsyncWrite, BufReader, ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;

use super::fixed::smtp::{reply_continues, takes_message};
use super::{Upstream, connect, passed};
use crate::testing::transcript::{Line, Shared, Transcript, Wire};
use crate::testing::{Fake, Lines, Listen, Phase, Protocol, read_line, record, smtp};

/// The script of the SMTP recorder: the live server and the recording it
/// shares with the IMAP recorder.
#[derive(Clone)]
pub struct Script {
    upstream: Arc<Upstream>,
    transcript: Shared,
}

/// The SMTP recorder as a server: what connects to it reaches the live
/// server, both sides written down.
pub type SmtpRecorder = Fake<Script>;

/// The live server's stream once connected.
type Live = TlsStream<TcpStream>;

impl Script {
    /// A recorder in front of the live server, writing into the
    /// recording given.
    #[must_use]
    pub fn new(upstream: Upstream, transcript: Shared) -> Self {
        Self {
            upstream: Arc::new(upstream),
            transcript,
        }
    }

    fn transcript(&self) -> MutexGuard<'_, Transcript> {
        self.transcript
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

// The same trait as the IMAP recorder, so the listener and the signature
// read alike.
// jscpd:ignore-start
impl Protocol for Script {
    const HOST: &'static str = smtp::HOST;

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
// jscpd:ignore-end

/// Both ends of one connection and where its record goes.
struct Relay<'a, S> {
    script: &'a Script,
    session: usize,
    client: (BufReader<ReadHalf<S>>, WriteHalf<S>),
    server: (BufReader<ReadHalf<Live>>, WriteHalf<Live>),
}

/// One connection in lock step: the greeting, then each command with its
/// reply until either side leaves.
async fn relay<S: AsyncRead + AsyncWrite + Unpin + Send>(
    script: &Script,
    phase: Phase,
    lines: &Lines,
    stream: S,
) -> Option<()> {
    let upstream = connect(&script.upstream).await?;
    let session = script.transcript().start_session(Wire::Smtp);
    let (client_read, client_write) = tokio::io::split(stream);
    let (server_read, server_write) = tokio::io::split(upstream);
    let mut relay = Relay {
        script,
        session,
        client: (BufReader::new(client_read), client_write),
        server: (BufReader::new(server_read), server_write),
    };
    relay.answered().await?;
    loop {
        let command = relay.commanded(phase, lines).await?;
        let reply = relay.answered().await?;
        if takes_message(&command, reply.last()?) {
            relay.message().await?;
            relay.answered().await?;
        }
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> Relay<'_, S> {
    /// The server's reply whole, each line noted and passed back; `None`
    /// when the server left.
    async fn answered(&mut self) -> Option<Vec<String>> {
        let mut reply = Vec::new();
        loop {
            let line = read_line(&mut self.server.0).await?;
            self.script
                .transcript()
                .server(Wire::Smtp, self.session, Line::text(&line));
            passed(&mut self.client.1, &line).await?;
            let more = reply_continues(&line);
            reply.push(line);
            if !more {
                return Some(reply);
            }
        }
    }

    /// One command noted and passed on; `None` when the client left.
    async fn commanded(&mut self, phase: Phase, lines: &Lines) -> Option<String> {
        let line = read_line(&mut self.client.0).await?;
        record(lines, phase, &line);
        self.script
            .transcript()
            .client(Wire::Smtp, self.session, line.clone());
        passed(&mut self.server.1, &line).await?;
        Some(line)
    }

    /// The message up to the lone dot, every line passed on as sent and
    /// noted with its leading dot taken off (RFC 5321 section 4.5.2).
    async fn message(&mut self) -> Option<()> {
        let mut message = Vec::new();
        loop {
            let line = read_line(&mut self.client.0).await?;
            passed(&mut self.server.1, &line).await?;
            if line == "." {
                break;
            }
            message.push(line.strip_prefix('.').map_or(line.clone(), str::to_owned));
        }
        self.script
            .transcript()
            .client(Wire::Smtp, self.session, message.join("\r\n"));
        Some(())
    }
}
