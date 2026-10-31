// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The IMAP script: CAPABILITY, STARTTLS, LOGIN, AUTHENTICATE with
//! XOAUTH2, LOGOUT, then the mailbox and message models behind them.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use tokio::io::{AsyncBufRead, AsyncRead, AsyncWrite, AsyncWriteExt};

use super::command::{Command, Reading, read as read_command};
use super::messages::{Conversation, search};
use super::{
    Fake, GOOGLE_ERROR, Greeting, Lines, Listen, Mailboxes, Opening, PASSWORD, Phase, Protocol,
    Starttls, Storing, TOKEN, USER, decoded, open, read_line, record,
};

/// The name the certificate carries.
pub const HOST: &str = "imap.example.test";
/// Advertised once TLS is on, unless a script says otherwise.
pub const CAPABILITIES: &str = "IMAP4rev2 IMAP4rev1 AUTH=PLAIN AUTH=XOAUTH2 IDLE";

/// The IMAP server of the tests.
pub type FakeImap = Fake<Script>;

/// How the IMAP server behaves.
#[derive(Debug, Clone)]
pub struct Script {
    /// TLS from the first byte or plaintext.
    pub listen: Listen,
    /// What the server says first.
    pub greeting: Greeting,
    /// Whether commands after the greeting get an answer.
    pub answers: bool,
    /// Advertised once TLS is on, the mailbox model's extensions after
    /// it. With no name at all the CAPABILITY line stays out.
    pub capabilities: &'static str,
    /// The one user the server signs in.
    pub user: &'static str,
    /// The sign-in backend is down: every sign-in over TLS answers NO
    /// with the RFC 5530 `UNAVAILABLE` code.
    pub unavailable: bool,
    /// The folders the server lists and the extensions it honors.
    pub mailboxes: Mailboxes,
    /// Lines nobody asked for, sent ahead of every CAPABILITY answer.
    pub ahead: String,
    /// Every CAPABILITY line filled with one long atom to this many
    /// bytes, its line break included.
    pub capability_bytes: Option<usize>,
}

impl Script {
    /// TLS from the first byte, an OK greeting, every command answered,
    /// the fixture user signed in.
    #[must_use]
    pub fn tls() -> Self {
        Self {
            listen: Listen::Tls,
            greeting: Greeting::Ok,
            answers: true,
            capabilities: CAPABILITIES,
            user: USER,
            unavailable: false,
            mailboxes: Mailboxes::default(),
            ahead: String::new(),
            capability_bytes: None,
        }
    }

    /// Plaintext with the given STARTTLS behavior.
    #[must_use]
    pub fn plain(starttls: Starttls) -> Self {
        Self {
            listen: Listen::Plain(starttls),
            ..Self::tls()
        }
    }

    /// The answer to a UID SEARCH, counted as it arrives. One that reads
    /// the messages is sent after the delay the behavior names; the UID
    /// list of a sync answers at once.
    async fn search(
        &self,
        conversation: &mut Conversation,
        command: &Command,
        tag: &str,
    ) -> String {
        self.mailboxes.searched();
        let reads = search::parse(command).is_some_and(|search| search.reads_messages());
        if let Some(delay) = self.mailboxes.behavior.search_delay
            && reads
        {
            tokio::time::sleep(delay).await;
        }
        conversation.search(&self.mailboxes, command, tag)
    }

    /// The answer to EXAMINE, SELECT, NOOP or a UID command; one the
    /// behavior never answers holds the connection.
    async fn selected_answer(
        &self,
        conversation: &mut Conversation,
        command: &str,
        tag: &str,
    ) -> Option<String> {
        if conversation.stalls(command, self.mailboxes.behavior) {
            std::future::pending::<()>().await;
        }
        conversation.answer(&self.mailboxes, command, tag)
    }

    fn opening(&self) -> Opening {
        Opening {
            greeting: self.greeting,
            words: words(self.greeting),
            answers: self.answers,
        }
    }

    /// The untagged CAPABILITY line of a phase.
    fn capability_line(&self, phase: Phase) -> String {
        let names = self.capabilities(phase);
        if names.is_empty() {
            return String::new();
        }
        let mut line = format!("* CAPABILITY {names}");
        let line_break = "\r\n";
        if let Some(bytes) = self.capability_bytes {
            let atom = bytes.saturating_sub(line.len() + " ".len() + line_break.len());
            line.push(' ');
            line.push_str(&"A".repeat(atom));
        }
        line + line_break
    }

    fn capabilities(&self, phase: Phase) -> String {
        match phase {
            Phase::Plain(Starttls::Absent) => "IMAP4rev1 LOGINDISABLED".to_owned(),
            Phase::Plain(_) => "IMAP4rev1 STARTTLS LOGINDISABLED".to_owned(),
            Phase::Tls | Phase::Upgraded => {
                let extensions = self.mailboxes.capabilities();
                [self.capabilities, extensions.as_str()]
                    .into_iter()
                    .filter(|words| !words.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ")
            }
        }
    }

    /// The answer to a mailbox command. `None` once the connection ends
    /// with it, as Dovecot ends one whose open mailbox a DELETE took.
    async fn folder_answer<W: AsyncWrite + Unpin>(
        &self,
        writer: &mut W,
        conversation: &Conversation,
        (verb, command, tag): (&str, &str, &str),
    ) -> Option<String> {
        let reply = self.mailboxes.answer(verb, command, tag);
        if reply.starts_with(&format!("{tag} OK")) && conversation.deletes_selected(command) {
            let bye = "* BYE Selected mailbox was deleted, have to disconnect\r\n";
            writer
                .write_all(format!("{reply}{bye}").as_bytes())
                .await
                .ok()?;
            return None;
        }
        Some(reply)
    }

    /// The answer to LOGOUT as the behavior has it. `None` once the
    /// connection ends with it, after the BYE and the tagged OK or
    /// without a word.
    async fn logout_answer<W: AsyncWrite + Unpin>(
        &self,
        writer: &mut W,
        tag: &str,
    ) -> Option<String> {
        match self.mailboxes.behavior.logging_out {
            Storing::Stores => {
                writer
                    .write_all(format!("* BYE bye\r\n{tag} OK done\r\n").as_bytes())
                    .await
                    .ok();
                None
            }
            Storing::Drops => None,
            Storing::Refuses => Some(format!("{tag} NO refused\r\n")),
            Storing::Bad => Some(format!("{tag} BAD refused\r\n")),
            Storing::Stalls => std::future::pending().await,
        }
    }
}

/// The greeting line for a kind that says something.
fn words(greeting: Greeting) -> &'static str {
    match greeting {
        Greeting::Ok => "* OK ready\r\n",
        Greeting::Bye => "* BYE not now\r\n",
        Greeting::Garbage => "220 mail.example.test ESMTP\r\n",
        Greeting::Silence => "",
    }
}

impl Protocol for Script {
    const HOST: &'static str = HOST;

    fn listen(&self) -> Listen {
        self.listen
    }

    async fn converse<S: AsyncRead + AsyncWrite + Unpin + Send>(
        &self,
        phase: Phase,
        lines: &Lines,
        stream: S,
    ) -> Option<S> {
        let (mut reader, mut writer) = open(self.opening(), phase, stream).await?;
        let mut conversation = Conversation::default();
        let reading = Reading {
            literals: self.mailboxes.literals(),
            continuation: self.mailboxes.behavior.continuation,
        };
        loop {
            let seen = |line: &str| record(lines, phase, line);
            let received = read_command(&mut reader, &mut writer, reading, seen).await?;
            let line = received.line();
            let (tag, command) = line.split_once(' ').unwrap_or((line.as_str(), ""));
            let verb = command
                .split(' ')
                .next()
                .unwrap_or_default()
                .to_ascii_uppercase();
            let reply = match verb.as_str() {
                "CAPABILITY" => format!(
                    "{}{}{tag} OK done\r\n",
                    self.ahead,
                    self.capability_line(phase)
                ),
                "STARTTLS" if phase == Phase::Plain(Starttls::Offered) => {
                    writer
                        .write_all(format!("{tag} OK begin TLS\r\n").as_bytes())
                        .await
                        .ok()?;
                    return Some(reader.into_inner().unsplit(writer));
                }
                "STARTTLS" if phase == Phase::Plain(Starttls::Refused) => {
                    format!("{tag} NO not now\r\n")
                }
                "LOGIN" | "AUTHENTICATE" if phase.is_tls() && self.unavailable => {
                    format!("{tag} NO [UNAVAILABLE] Temporary authentication failure.\r\n")
                }
                "LOGIN"
                    if phase.is_tls()
                        && command == format!("LOGIN \"{}\" \"{PASSWORD}\"", self.user) =>
                {
                    format!("{tag} OK signed in\r\n")
                }
                "LOGIN" => format!("{tag} NO [AUTHENTICATIONFAILED] Authentication failed.\r\n"),
                "AUTHENTICATE" if phase.is_tls() => {
                    let signed_in = xoauth2(&mut reader, &mut writer, lines, self.user).await?;
                    authenticated(tag, signed_in)
                }
                "LOGOUT" => self.logout_answer(&mut writer, tag).await?,
                "LIST" | "LSUB" | "STATUS" | "CREATE" | "RENAME" | "DELETE" | "SUBSCRIBE"
                | "UNSUBSCRIBE" | "MYRIGHTS"
                    if phase.is_tls() =>
                {
                    self.folder_answer(&mut writer, &conversation, (&verb, command, tag))
                        .await?
                }
                "GETQUOTAROOT" if phase.is_tls() => self.mailboxes.quota_root(tag),
                "UID" if phase.is_tls() && command.starts_with("UID SEARCH ") => {
                    self.search(&mut conversation, &received, tag).await
                }
                "EXAMINE" | "SELECT" | "UID" | "NOOP" if phase.is_tls() => {
                    self.selected_answer(&mut conversation, command, tag)
                        .await?
                }
                _ => format!("{tag} BAD unknown command\r\n"),
            };
            writer.write_all(reply.as_bytes()).await.ok()?;
            // A TLS write may return with ciphertext still buffered.
            writer.flush().await.ok()?;
        }
    }
}

/// The tagged answer to AUTHENTICATE, as Google words it.
fn authenticated(tag: &str, signed_in: bool) -> String {
    if signed_in {
        format!("{tag} OK signed in\r\n")
    } else {
        format!("{tag} NO [AUTHENTICATIONFAILED] Invalid credentials (Failure)\r\n")
    }
}

/// `* 1 FETCH` with a BODYSTRUCTURE of multiparts around one leaf, its
/// deepest parenthesis `depth` levels in; two levels at least.
#[must_use]
pub fn nested_fetch(depth: usize) -> String {
    let mut body = "(\"TEXT\" \"PLAIN\" NIL NIL NIL \"7BIT\" 1 1)".to_owned();
    for _ in 2..depth {
        body = format!("({body} \"MIXED\")");
    }
    format!("* 1 FETCH (UID 1 BODYSTRUCTURE {body})\r\n")
}

/// The XOAUTH2 exchange as Google runs it: an empty challenge, the
/// identity; on a wrong token a challenge carrying the error and an
/// empty answer before the refusal. Whether `user` signed in with the
/// fixture token; the caller sends the tagged answer.
async fn xoauth2<R: AsyncBufRead + Unpin, W: AsyncWrite + Unpin>(
    reader: &mut R,
    writer: &mut W,
    lines: &Lines,
    user: &str,
) -> Option<bool> {
    writer.write_all(b"+ \r\n").await.ok()?;
    let identity = decoded(lines, Phase::Tls, &read_line(reader).await?);
    if identity == format!("user={user}\x01auth=Bearer {TOKEN}\x01\x01") {
        return Some(true);
    }
    let error = BASE64.encode(GOOGLE_ERROR);
    writer
        .write_all(format!("+ {error}\r\n").as_bytes())
        .await
        .ok()?;
    let empty = read_line(reader).await?;
    record(lines, Phase::Tls, &format!("SASL {empty:?}"));
    Some(false)
}
