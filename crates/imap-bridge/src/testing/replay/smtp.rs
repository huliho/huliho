// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The SMTP replayer: the scripted submission server driven by the sends
//! of a transcript. Every command gets what the live server answered; an
//! EHLO is read by its verb alone, since every machine says its own
//! name, and the message after a DATA the recording took is read up to
//! the lone dot and compared with nothing. A command the recording does
//! not hold at that point is answered 500 and noted for the test to
//! read.

use tokio::io::{AsyncRead, AsyncWrite, BufReader};

use super::{Playback, write_lines};
use crate::testing::record::fixed::smtp::{is_ehlo, takes_message, verb};
use crate::testing::smtp::{self, HOST};
use crate::testing::transcript::{Conversation, Line};
use crate::testing::{Fake, Lines, Listen, Phase, Protocol, read_line, record};

/// What a command off the script is answered with (RFC 5321 section
/// 4.2.3).
const OFF_SCRIPT: &str = "500 5.5.1 not in the transcript";

/// The script of the SMTP replayer: the sends still to play and what
/// did not go as recorded.
#[derive(Clone)]
pub struct Script {
    playback: Playback,
}

/// The SMTP replayer as a server.
pub type SmtpReplayer = Fake<Script>;

impl Script {
    /// A replayer over the sends of a transcript, in order.
    #[must_use]
    pub fn new(sends: Vec<Conversation>) -> Self {
        Self {
            playback: Playback::new(sends),
        }
    }

    /// What did not go as recorded; empty after a faithful replay.
    #[must_use]
    pub fn mismatches(&self) -> Vec<String> {
        self.playback.mismatches()
    }

    /// The recorded sends nobody opened yet.
    #[must_use]
    pub fn unplayed(&self) -> usize {
        self.playback.unplayed()
    }
}

/// Whether the command is the recorded one; an EHLO by its verb alone.
fn matches(recorded: &str, command: &str) -> bool {
    (is_ehlo(recorded) && is_ehlo(command)) || recorded == command
}

/// A command as a note shows it: an AUTH by its verb, so no credential
/// lands in a note.
fn shown(command: &str) -> &str {
    if verb(command) == "AUTH" {
        "AUTH"
    } else {
        command
    }
}

// The same trait as the IMAP replayer, so the listener and the signature
// read alike.
// jscpd:ignore-start
impl Protocol for Script {
    const HOST: &'static str = HOST;

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
        let (reader, mut writer) = tokio::io::split(stream);
        let mut reader = BufReader::new(reader);
        let exchanges = self.playback.opened(&mut writer).await?;
        let mut left = exchanges.len();
        let mut message_next = false;
        for expected in exchanges {
            let Some(recorded) = expected.client.as_deref() else {
                self.playback
                    .note("an exchange without a command".to_owned());
                return None;
            };
            // The message is read up to the dot and compared with nothing.
            let read = if message_next {
                smtp::message(&mut reader).await.map(|_message| None)
            } else {
                read_line(&mut reader).await.map(Some)
            };
            let Some(command) = read else {
                self.playback
                    .note(format!("the client left with exchanges unplayed: {left}"));
                return None;
            };
            if let Some(command) = command {
                record(lines, phase, &command);
                if !matches(recorded, &command) {
                    self.playback.note(format!(
                        "expected {}, got {}",
                        shown(recorded),
                        shown(&command)
                    ));
                    write_lines(&mut writer, &[Line::text(OFF_SCRIPT)]).await;
                    return None;
                }
            }
            message_next = expected
                .server
                .last()
                .is_some_and(|last| takes_message(recorded, last.opening()));
            write_lines(&mut writer, &expected.server).await?;
            left -= 1;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, DuplexStream};

    use super::*;
    use crate::testing::transcript::Exchange;

    /// Room for one conversation in the pipe between the two ends.
    const PIPE_BYTES: usize = 4096;

    /// How long a test waits for the replayer to finish a conversation.
    const DONE: Duration = Duration::from_secs(2);

    type Client = BufReader<DuplexStream>;

    fn exchange(client: Option<&str>, server: &[&str]) -> Exchange {
        Exchange {
            client: client.map(str::to_owned),
            server: server.iter().map(|line| Line::text(line)).collect(),
        }
    }

    /// One send: the greeting, EHLO, DATA with a message, QUIT.
    fn one_send() -> Conversation {
        Conversation {
            exchanges: vec![
                exchange(None, &["220 ready"]),
                exchange(
                    Some("EHLO client.example.test"),
                    &["250-smtp.example.test ready", "250 PIPELINING"],
                ),
                exchange(Some("DATA"), &["354 ready"]),
                exchange(
                    Some("Message-ID: <d1@example.test>\r\n\r\nx"),
                    &["250 2.0.0 ready"],
                ),
                exchange(Some("QUIT"), &["221 2.0.0 ready"]),
            ],
        }
    }

    /// The script conversing over a pipe; the client's end and the task
    /// that ends when the conversation does.
    fn served(script: &Script) -> (Client, tokio::task::JoinHandle<()>) {
        let (client, server) = tokio::io::duplex(PIPE_BYTES);
        let script = script.clone();
        let task = tokio::spawn(async move {
            script.converse(Phase::Tls, &Lines::default(), server).await;
        });
        (BufReader::new(client), task)
    }

    async fn send(client: &mut Client, line: &str) {
        client
            .write_all(format!("{line}\r\n").as_bytes())
            .await
            .unwrap();
    }

    /// The next line, or `None` once the replayer closed the connection.
    async fn next_line(client: &mut Client) -> Option<String> {
        let mut text = String::new();
        match client.read_line(&mut text).await {
            Ok(0) => None,
            Ok(_) => Some(text),
            Err(error) => panic!("{error}"),
        }
    }

    #[tokio::test]
    async fn an_ehlo_of_any_name_and_a_message_up_to_the_dot_play_as_recorded() {
        let script = Script::new(vec![one_send()]);
        let (mut client, task) = served(&script);
        assert_eq!(
            next_line(&mut client).await.as_deref(),
            Some("220 ready\r\n")
        );
        send(&mut client, "EHLO some.other.host").await;
        assert_eq!(
            next_line(&mut client).await.as_deref(),
            Some("250-smtp.example.test ready\r\n")
        );
        assert_eq!(
            next_line(&mut client).await.as_deref(),
            Some("250 PIPELINING\r\n")
        );
        send(&mut client, "DATA").await;
        assert_eq!(
            next_line(&mut client).await.as_deref(),
            Some("354 ready\r\n")
        );
        send(&mut client, "Subject: anything at all\r\n\r\n..dots\r\n.").await;
        assert_eq!(
            next_line(&mut client).await.as_deref(),
            Some("250 2.0.0 ready\r\n")
        );
        send(&mut client, "QUIT").await;
        assert_eq!(
            next_line(&mut client).await.as_deref(),
            Some("221 2.0.0 ready\r\n")
        );
        assert_eq!(next_line(&mut client).await, None);
        tokio::time::timeout(DONE, task).await.unwrap().unwrap();
        assert_eq!(script.mismatches(), Vec::<String>::new());
        assert_eq!(script.unplayed(), 0);
    }

    #[tokio::test]
    async fn a_command_off_the_script_is_answered_500_and_noted_without_its_credential() {
        let script = Script::new(vec![one_send()]);
        let (mut client, task) = served(&script);
        assert_eq!(
            next_line(&mut client).await.as_deref(),
            Some("220 ready\r\n")
        );
        send(&mut client, "AUTH PLAIN c2VjcmV0").await;
        assert_eq!(
            next_line(&mut client).await.as_deref(),
            Some("500 5.5.1 not in the transcript\r\n")
        );
        assert_eq!(next_line(&mut client).await, None);
        tokio::time::timeout(DONE, task).await.unwrap().unwrap();
        assert_eq!(
            script.mismatches(),
            ["expected EHLO client.example.test, got AUTH"]
        );
        let (mut second, task) = served(&script);
        assert_eq!(next_line(&mut second).await, None);
        tokio::time::timeout(DONE, task).await.unwrap().unwrap();
        assert_eq!(script.mismatches().len(), 2);
    }

    #[tokio::test]
    async fn a_client_that_leaves_early_is_noted_with_what_was_left() {
        let script = Script::new(vec![one_send()]);
        let (mut client, task) = served(&script);
        assert_eq!(
            next_line(&mut client).await.as_deref(),
            Some("220 ready\r\n")
        );
        drop(client);
        tokio::time::timeout(DONE, task).await.unwrap().unwrap();
        assert_eq!(
            script.mismatches(),
            ["the client left with exchanges unplayed: 4"]
        );
    }
}
