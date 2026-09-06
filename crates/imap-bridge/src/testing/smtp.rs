// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The SMTP script: EHLO, STARTTLS, AUTH with PLAIN, LOGIN and XOAUTH2,
//! QUIT.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use tokio::io::{AsyncBufRead, AsyncRead, AsyncWrite, AsyncWriteExt};

use super::{
    Fake, GOOGLE_ERROR, Greeting, Lines, Listen, PASSWORD, Phase, Protocol, Starttls, TOKEN, USER,
    decoded, open, read_line, record,
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

/// The SMTP submission server of the tests.
pub type FakeSmtp = Fake<Script>;

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
}

impl Script {
    /// TLS from the first byte, a 220 greeting, every mechanism, every
    /// command answered.
    #[must_use]
    pub fn tls() -> Self {
        Self {
            listen: Listen::Tls,
            greeting: Greeting::Ok,
            answers: true,
            mechanisms: MECHANISMS,
            accepts: true,
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
}

// The same trait as the IMAP script, so the accessors and the loop's opening
// read alike.
// jscpd:ignore-start
impl Protocol for Script {
    const HOST: &'static str = HOST;

    fn listen(self) -> Listen {
        self.listen
    }

    fn greeting(self) -> Greeting {
        self.greeting
    }

    fn answers(self) -> bool {
        self.answers
    }

    fn words(greeting: Greeting) -> &'static str {
        match greeting {
            Greeting::Ok => "220 smtp.example.test ESMTP ready\r\n",
            Greeting::Bye => "421 4.3.2 not now\r\n",
            Greeting::Garbage => "* OK IMAP4rev2 ready\r\n",
            Greeting::Silence => "",
        }
    }

    async fn converse<S: AsyncRead + AsyncWrite + Unpin + Send>(
        self,
        phase: Phase,
        lines: &Lines,
        stream: S,
    ) -> Option<S> {
        let (mut reader, mut writer) = open(self, phase, stream).await?;
        loop {
            let line = read_line(&mut reader).await?;
            record(lines, phase, &line);
            let (verb, argument) = line.split_once(' ').unwrap_or((line.as_str(), ""));
            let reply = match verb.to_ascii_uppercase().as_str() {
                "EHLO" => self.ehlo(phase),
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
                    exchange.auth(self, argument).await?
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
                Some(script.verdict(identity == format!("\0{USER}\0{PASSWORD}")))
            }
            "LOGIN" => {
                let username = self.ask(ASK_USERNAME).await?;
                let password = self.ask(ASK_PASSWORD).await?;
                Some(script.verdict(username == USER && password == PASSWORD))
            }
            "XOAUTH2" => self.xoauth2(script, initial).await,
            _ => Some("504 5.5.4 mechanism not supported\r\n".to_owned()),
        }
    }

    /// The XOAUTH2 exchange as Google runs it: on a wrong token a
    /// challenge carrying the error, one more line and then the refusal.
    async fn xoauth2(&mut self, script: Script, initial: &str) -> Option<String> {
        let identity = decoded(self.lines, Phase::Tls, initial);
        if identity == format!("user={USER}\x01auth=Bearer {TOKEN}\x01\x01") && script.accepts {
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
