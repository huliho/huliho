// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The SMTP script: EHLO, STARTTLS, AUTH with PLAIN, LOGIN and XOAUTH2,
//! MAIL, RCPT, RSET, DATA with the refusal the script names, QUIT. A
//! message that came through DATA is recorded as one line after
//! `MESSAGE`, its dots unstuffed.

use std::fmt::Write as _;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use tokio::io::{AsyncBufRead, AsyncRead, AsyncWrite, AsyncWriteExt};

use super::{
    Fake, GOOGLE_ERROR, Greeting, Lines, Listen, Opening, PASSWORD, Phase, Protocol, Starttls,
    TOKEN, USER, decoded, open, read_line, record,
};

/// The name the certificate carries.
pub const HOST: &str = "smtp.example.test";
/// Advertised once TLS is on, unless a script says otherwise.
pub const MECHANISMS: &str = "PLAIN LOGIN XOAUTH2";

const ACCEPTED: &str = "235 2.7.0 signed in\r\n";
const REJECTED: &str = "535 5.7.8 authentication credentials invalid\r\n";
/// "Username:" and "Password:" as the LOGIN mechanism asks them.
const ASK_USERNAME: &str = "334 VXNlcm5hbWU6\r\n";
const ASK_PASSWORD: &str = "334 UGFzc3dvcmQ6\r\n";

/// What a line of the recorded message starts with.
pub const MESSAGE: &str = "MESSAGE ";

/// The SMTP submission server of the tests.
pub type FakeSmtp = Fake<Script>;

/// The one refusal a script answers with (RFC 5321 section 4.2, RFC 3463).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Every command goes through.
    None,
    /// MAIL FROM is refused for good.
    Sender,
    /// MAIL FROM is to be tried later.
    Later,
    /// RCPT TO is refused for good for an address holding the text.
    Recipient(&'static str),
    /// RCPT TO past this many accepted ones is 452 with 4.5.3.
    TooMany(usize),
    /// The message after DATA is too big for the system.
    TooLarge,
    /// The message after DATA is refused for good.
    Rejected,
}

/// How the SMTP server behaves.
#[derive(Debug, Clone, Copy)]
pub struct Script {
    /// TLS from the first byte or plaintext.
    pub listen: Listen,
    /// What the server says first.
    pub greeting: Greeting,
    /// Whether commands after the greeting get an answer.
    pub answers: bool,
    /// The AUTH mechanisms advertised once TLS is on; empty means no
    /// AUTH line at all.
    pub mechanisms: &'static str,
    /// Whether the right credential signs in; a mailbox with SMTP AUTH
    /// off says no to everything.
    pub accepts: bool,
    /// The one user the server signs in.
    pub user: &'static str,
    /// The SIZE the EHLO answer names once TLS is on; 0 names none.
    pub size: usize,
    pub refusal: Refusal,
}

impl Script {
    /// TLS from the first byte, a 220 greeting, every mechanism, every
    /// command answered, the fixture user signed in, every message taken.
    #[must_use]
    pub fn tls() -> Self {
        Self {
            listen: Listen::Tls,
            greeting: Greeting::Ok,
            answers: true,
            mechanisms: MECHANISMS,
            accepts: true,
            user: USER,
            size: 0,
            refusal: Refusal::None,
        }
    }

    /// The reply to MAIL FROM.
    fn mail(self) -> &'static str {
        match self.refusal {
            Refusal::Sender => "550 5.7.1 sender refused\r\n",
            Refusal::Later => "451 4.3.0 try again later\r\n",
            _ => "250 2.1.0 sender ok\r\n",
        }
    }

    /// The reply to RCPT TO, with how many were accepted before it.
    fn rcpt(self, argument: &str, accepted: usize) -> &'static str {
        match self.refusal {
            Refusal::Recipient(marker) if argument.contains(marker) => {
                "550 5.1.1 no such mailbox\r\n"
            }
            Refusal::TooMany(most) if accepted >= most => "452 4.5.3 too many recipients\r\n",
            _ => "250 2.1.5 recipient ok\r\n",
        }
    }

    /// The reply after the message.
    fn delivered(self) -> &'static str {
        match self.refusal {
            Refusal::TooLarge => "552 5.3.4 message too big\r\n",
            Refusal::Rejected => "550 5.7.1 message refused\r\n",
            _ => "250 2.0.0 queued\r\n",
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

    /// The EHLO answer: STARTTLS on a plaintext listener that offers it,
    /// AUTH once TLS is on (RFC 3207 section 4, RFC 4954 section 3).
    fn ehlo(self, phase: Phase) -> String {
        let mut answer = format!("250-{HOST}\r\n");
        if matches!(phase, Phase::Plain(Starttls::Offered | Starttls::Refused)) {
            answer.push_str("250-STARTTLS\r\n");
        }
        if phase.is_tls() && !self.mechanisms.is_empty() {
            answer.push_str("250-AUTH ");
            answer.push_str(self.mechanisms);
            answer.push_str("\r\n");
        }
        if phase.is_tls() && self.size > 0 {
            let _ = write!(answer, "250-SIZE {}\r\n", self.size);
        }
        answer.push_str("250 PIPELINING\r\n");
        answer
    }

    fn verdict(self, right: bool) -> String {
        if right && self.accepts {
            ACCEPTED.to_owned()
        } else {
            REJECTED.to_owned()
        }
    }

    fn opening(self) -> Opening {
        Opening {
            greeting: self.greeting,
            words: words(self.greeting),
            answers: self.answers,
        }
    }
}

/// The greeting line for a kind that says something.
fn words(greeting: Greeting) -> &'static str {
    match greeting {
        Greeting::Ok => "220 smtp.example.test ESMTP ready\r\n",
        Greeting::Bye => "421 4.3.2 not now\r\n",
        Greeting::Garbage => "* OK IMAP4rev2 ready\r\n",
        Greeting::Silence => "",
    }
}

// The same trait as the IMAP script, so the accessors and the loop's opening
// read alike.
// jscpd:ignore-start
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
        let mut accepted = 0;
        loop {
            let line = read_line(&mut reader).await?;
            record(lines, phase, &line);
            let (verb, argument) = line.split_once(' ').unwrap_or((line.as_str(), ""));
            let reply = match verb.to_ascii_uppercase().as_str() {
                "EHLO" => self.ehlo(phase),
                "MAIL" | "RCPT" | "DATA" if !phase.is_tls() => {
                    "530 5.7.0 STARTTLS first\r\n".to_owned()
                }
                "MAIL" => self.mail().to_owned(),
                "RCPT" => {
                    let reply = self.rcpt(argument, accepted);
                    accepted += usize::from(reply.starts_with('2'));
                    reply.to_owned()
                }
                "RSET" => {
                    accepted = 0;
                    "250 2.0.0 reset\r\n".to_owned()
                }
                "DATA" => {
                    writer.write_all(b"354 end with a dot\r\n").await.ok()?;
                    let message = message(&mut reader).await?;
                    record(lines, phase, &format!("{MESSAGE}{message}"));
                    self.delivered().to_owned()
                }
                "STARTTLS" if phase == Phase::Plain(Starttls::Offered) => {
                    writer.write_all(b"220 2.0.0 begin TLS\r\n").await.ok()?;
                    return Some(reader.into_inner().unsplit(writer));
                }
                "STARTTLS" => "454 4.7.0 not now\r\n".to_owned(),
                "AUTH" if phase.is_tls() => {
                    let mut exchange = Exchange {
                        reader: &mut reader,
                        writer: &mut writer,
                        lines,
                    };
                    exchange.auth(*self, argument).await?
                }
                "AUTH" => "530 5.7.0 STARTTLS first\r\n".to_owned(),
                "QUIT" => {
                    writer.write_all(b"221 2.0.0 bye\r\n").await.ok();
                    return None;
                }
                _ => "500 5.5.1 unknown command\r\n".to_owned(),
            };
            writer.write_all(reply.as_bytes()).await.ok()?;
        }
    }
}
// jscpd:ignore-end

/// The lines up to the one dot, each leading dot taken off (RFC 5321
/// section 4.5.2), joined by CRLF; `None` when the client left first.
pub(super) async fn message<R: AsyncBufRead + Unpin>(reader: &mut R) -> Option<String> {
    let mut lines = Vec::new();
    loop {
        let line = read_line(reader).await?;
        if line == "." {
            return Some(lines.join("\r\n"));
        }
        lines.push(line.strip_prefix('.').map_or(line.clone(), str::to_owned));
    }
}

/// One AUTH exchange on an encrypted connection.
struct Exchange<'a, R, W> {
    reader: &'a mut R,
    writer: &'a mut W,
    lines: &'a Lines,
}

impl<R: AsyncBufRead + Unpin, W: AsyncWrite + Unpin> Exchange<'_, R, W> {
    /// The initial response rides the command for PLAIN and XOAUTH2;
    /// LOGIN asks in two challenges (RFC 4954 section 4).
    async fn auth(&mut self, script: Script, argument: &str) -> Option<String> {
        let (mechanism, initial) = argument.split_once(' ').unwrap_or((argument, ""));
        match mechanism.to_ascii_uppercase().as_str() {
            "PLAIN" => {
                let identity = decoded(self.lines, Phase::Tls, initial);
                Some(script.verdict(identity == format!("\0{}\0{PASSWORD}", script.user)))
            }
            "LOGIN" => {
                let username = self.ask(ASK_USERNAME).await?;
                let password = self.ask(ASK_PASSWORD).await?;
                Some(script.verdict(username == script.user && password == PASSWORD))
            }
            "XOAUTH2" => self.xoauth2(script, initial).await,
            _ => Some("504 5.5.4 mechanism not supported\r\n".to_owned()),
        }
    }

    /// The XOAUTH2 exchange as Google runs it: on a wrong token a
    /// challenge carrying the error, one more line and then the refusal.
    async fn xoauth2(&mut self, script: Script, initial: &str) -> Option<String> {
        let identity = decoded(self.lines, Phase::Tls, initial);
        let expected = format!("user={}\x01auth=Bearer {TOKEN}\x01\x01", script.user);
        if identity == expected && script.accepts {
            return Some(ACCEPTED.to_owned());
        }
        let error = BASE64.encode(GOOGLE_ERROR);
        self.writer
            .write_all(format!("334 {error}\r\n").as_bytes())
            .await
            .ok()?;
        let _answer = decoded(self.lines, Phase::Tls, &read_line(self.reader).await?);
        Some(REJECTED.to_owned())
    }

    /// Sends one challenge and reads the decoded answer.
    async fn ask(&mut self, challenge: &str) -> Option<String> {
        self.writer.write_all(challenge.as_bytes()).await.ok()?;
        Some(decoded(
            self.lines,
            Phase::Tls,
            &read_line(self.reader).await?,
        ))
    }
}
