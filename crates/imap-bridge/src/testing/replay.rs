// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The replayer: the scripted server driven by a transcript. Every
//! command gets what the live server answered, under the client's own
//! tag, and the connection closes where the live one closed. A command
//! the transcript does not hold at that point is a mismatch: answered
//! BAD and noted for the test to read.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};

use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

use super::transcript::{Conversation, Line, Transcript, split_tag};
use super::{Fake, Lines, Listen, Phase, Protocol, read_line, record};

/// The script of the replayer: the connections still to play and what
/// did not go as recorded.
#[derive(Clone)]
pub struct Script {
    sessions: Arc<Mutex<VecDeque<Conversation>>>,
    mismatches: Arc<Mutex<Vec<String>>>,
}

/// The replayer as a server.
pub type Replayer = Fake<Script>;

impl Script {
    /// A replayer over the transcript, its connections in order.
    #[must_use]
    pub fn new(transcript: Transcript) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(transcript.sessions.into())),
            mismatches: Arc::default(),
        }
    }

    /// What did not go as recorded: a command off the script, a
    /// connection past the last recorded one or a connection the
    /// client closed with exchanges left. Empty after a faithful
    /// replay.
    #[must_use]
    pub fn mismatches(&self) -> Vec<String> {
        self.mismatches
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The recorded connections nobody opened yet.
    #[must_use]
    pub fn unplayed(&self) -> usize {
        self.sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    fn note(&self, what: String) {
        self.mismatches
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(what);
    }

    fn next_session(&self) -> Option<Conversation> {
        self.sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front()
    }
}

// The same trait as the recorder, so the listener and the signature read
// alike.
// jscpd:ignore-start
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
        // jscpd:ignore-end
        let Some(conversation) = self.next_session() else {
            self.note("a connection past the last recorded one".to_owned());
            return None;
        };
        let (reader, mut writer) = tokio::io::split(stream);
        let mut reader = BufReader::new(reader);
        let mut exchanges = conversation.exchanges.into_iter();
        let greeting = exchanges.next()?;
        if greeting.client.is_some() {
            self.note("the recording opens with a command instead of the greeting".to_owned());
            return None;
        }
        write_lines(&mut writer, &greeting.server).await?;
        let mut left = exchanges.len();
        for expected in exchanges {
            let Some(line) = read_line(&mut reader).await else {
                self.note(format!("the client left with exchanges unplayed: {left}"));
                return None;
            };
            record(lines, phase, &line);
            let (tag, command) = split_tag(&line);
            let Some(recorded) = expected.client.as_deref() else {
                self.note("an exchange without a command".to_owned());
                return None;
            };
            let (recorded_tag, recorded_command) = split_tag(recorded);
            if command != recorded_command {
                self.note(format!("expected {recorded_command}, got {command}"));
                let refusal = Line::text(&format!("{tag} BAD not in the transcript"));
                write_lines(&mut writer, &[refusal]).await;
                return None;
            }
            let answers: Vec<Line> = expected
                .server
                .iter()
                .map(|line| line.retagged(recorded_tag, tag))
                .collect();
            write_lines(&mut writer, &answers).await?;
            left -= 1;
        }
        None
    }
}

async fn write_lines<W: AsyncWrite + Unpin>(writer: &mut W, lines: &[Line]) -> Option<()> {
    for line in lines {
        writer.write_all(&line.bytes()).await.ok()?;
    }
    // A TLS write may return with ciphertext still buffered.
    writer.flush().await.ok()
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::time::Duration;

    use tokio::io::AsyncBufReadExt;
    use tokio::net::TcpStream;
    use tokio_rustls::TlsConnector;
    use tokio_rustls::client::TlsStream;
    use tokio_rustls::rustls::pki_types::ServerName;

    use super::*;
    use crate::testing::imap::HOST;
    use crate::testing::transcript::Exchange;

    /// How long a test waits for the replayer to note a connection that
    /// went wrong, looking every `LOOK`.
    const NOTICE: Duration = Duration::from_secs(2);
    const LOOK: Duration = Duration::from_millis(10);

    type Client = BufReader<TlsStream<TcpStream>>;

    /// A transcript of one connection: the greeting and one NOOP.
    fn one_noop() -> Transcript {
        Transcript {
            sessions: vec![Conversation {
                exchanges: vec![
                    Exchange {
                        client: None,
                        server: vec![Line::text("* OK ready")],
                    },
                    Exchange {
                        client: Some("A1 NOOP".to_owned()),
                        server: vec![Line::text("A1 OK done")],
                    },
                ],
            }],
        }
    }

    async fn connect(replayer: &Replayer) -> Client {
        let tcp = TcpStream::connect(replayer.address).await.unwrap();
        let name = ServerName::try_from(HOST).unwrap();
        let stream = TlsConnector::from(replayer.trusting())
            .connect(name, tcp)
            .await
            .unwrap();
        BufReader::new(stream)
    }

    async fn send(client: &mut Client, line: &str) {
        client
            .write_all(format!("{line}\r\n").as_bytes())
            .await
            .unwrap();
        client.flush().await.unwrap();
    }

    /// The next line, or `None` once the replayer closed the connection.
    async fn next_line(client: &mut Client) -> Option<String> {
        let mut text = String::new();
        match client.read_line(&mut text).await {
            Ok(0) => None,
            Ok(_) => Some(text),
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => None,
            Err(error) => panic!("{error}"),
        }
    }

    /// The replayer's notes, once there is one.
    async fn noted(replayer: &Replayer) -> Vec<String> {
        tokio::time::timeout(NOTICE, async {
            loop {
                let found = replayer.script().mismatches();
                if !found.is_empty() {
                    return found;
                }
                tokio::time::sleep(LOOK).await;
            }
        })
        .await
        .expect("the replayer notes what went wrong")
    }

    #[tokio::test]
    async fn a_command_off_the_script_is_refused_under_its_own_tag_and_noted() {
        let replayer = Replayer::start(Script::new(one_noop())).await;
        let mut client = connect(&replayer).await;
        assert_eq!(
            next_line(&mut client).await.as_deref(),
            Some("* OK ready\r\n")
        );
        send(&mut client, "Z9 CAPABILITY").await;
        assert_eq!(
            next_line(&mut client).await.as_deref(),
            Some("Z9 BAD not in the transcript\r\n")
        );
        assert_eq!(next_line(&mut client).await, None);
        assert_eq!(noted(&replayer).await, ["expected NOOP, got CAPABILITY"]);
        assert_eq!(replayer.script().unplayed(), 0);
    }

    #[tokio::test]
    async fn a_connection_past_the_last_recorded_one_is_closed_and_noted() {
        let replayer = Replayer::start(Script::new(one_noop())).await;
        let mut first = connect(&replayer).await;
        assert_eq!(
            next_line(&mut first).await.as_deref(),
            Some("* OK ready\r\n")
        );
        send(&mut first, "A7 NOOP").await;
        assert_eq!(
            next_line(&mut first).await.as_deref(),
            Some("A7 OK done\r\n")
        );
        assert_eq!(next_line(&mut first).await, None);
        assert_eq!(replayer.script().mismatches(), Vec::<String>::new());
        let mut second = connect(&replayer).await;
        assert_eq!(next_line(&mut second).await, None);
        assert_eq!(
            noted(&replayer).await,
            ["a connection past the last recorded one"]
        );
    }

    #[tokio::test]
    async fn a_client_that_leaves_early_is_noted_with_what_was_left() {
        let replayer = Replayer::start(Script::new(one_noop())).await;
        let mut client = connect(&replayer).await;
        assert_eq!(
            next_line(&mut client).await.as_deref(),
            Some("* OK ready\r\n")
        );
        drop(client);
        assert_eq!(
            noted(&replayer).await,
            ["the client left with exchanges unplayed: 1"]
        );
    }
}
