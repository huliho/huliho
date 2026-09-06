// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The IMAP script: CAPABILITY, STARTTLS, LOGIN, AUTHENTICATE with
//! XOAUTH2, LOGOUT.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use tokio::io::{AsyncBufRead, AsyncRead, AsyncWrite, AsyncWriteExt};

use super::{
    Fake, GOOGLE_ERROR, Greeting, Lines, Listen, PASSWORD, Phase, Protocol, Starttls, TOKEN, USER,
    decoded, open, read_line, record,
};

/// The name the certificate carries.
pub const HOST: &str = "imap.example.test";
/// Advertised once TLS is on, unless a script says otherwise.
pub const CAPABILITIES: &str = "IMAP4rev2 IMAP4rev1 AUTH=PLAIN AUTH=XOAUTH2 IDLE";

/// The IMAP server of the tests.
pub type FakeImap = Fake<Script>;

/// How the IMAP server behaves.
#[derive(Debug, Clone, Copy)]
pub struct Script {
    /// TLS from the first byte or plaintext.
    pub listen: Listen,
    /// What the server says first.
    pub greeting: Greeting,
    /// Whether commands after the greeting get an answer.
    pub answers: bool,
    /// Advertised once TLS is on.
    pub capabilities: &'static str,
}

impl Script {
    /// TLS from the first byte, an OK greeting, every command answered.
    #[must_use]
    pub fn tls() -> Self {
        Self {
            listen: Listen::Tls,
            greeting: Greeting::Ok,
            answers: true,
            capabilities: CAPABILITIES,
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

    fn capabilities(self, phase: Phase) -> &'static str {
        match phase {
            Phase::Plain(Starttls::Absent) => "IMAP4rev1 LOGINDISABLED",
            Phase::Plain(_) => "IMAP4rev1 STARTTLS LOGINDISABLED",
            Phase::Tls | Phase::Upgraded => self.capabilities,
        }
    }
}

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
            Greeting::Ok => "* OK ready\r\n",
            Greeting::Bye => "* BYE not now\r\n",
            Greeting::Garbage => "220 mail.example.test ESMTP\r\n",
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
            let (tag, command) = line.split_once(' ').unwrap_or((line.as_str(), ""));
            let verb = command
                .split(' ')
                .next()
                .unwrap_or_default()
                .to_ascii_uppercase();
            let reply = match verb.as_str() {
                "CAPABILITY" => format!(
                    "* CAPABILITY {}\r\n{tag} OK done\r\n",
                    self.capabilities(phase)
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
                "LOGIN"
                    if phase.is_tls() && command == format!("LOGIN \"{USER}\" \"{PASSWORD}\"") =>
                {
                    format!("{tag} OK signed in\r\n")
                }
                "LOGIN" => format!("{tag} NO [AUTHENTICATIONFAILED] Authentication failed.\r\n"),
                "AUTHENTICATE" if phase.is_tls() => {
                    xoauth2(&mut reader, &mut writer, lines, tag).await?
                }
                "LOGOUT" => {
                    writer
                        .write_all(format!("* BYE bye\r\n{tag} OK done\r\n").as_bytes())
                        .await
                        .ok();
                    return None;
                }
                _ => format!("{tag} BAD unknown command\r\n"),
            };
            writer.write_all(reply.as_bytes()).await.ok()?;
        }
    }
}

/// The XOAUTH2 exchange as Google runs it: an empty challenge, the
/// identity; on a wrong token a challenge carrying the error, an empty
/// answer and then NO. AUTHENTICATE is only answered over TLS.
async fn xoauth2<R: AsyncBufRead + Unpin, W: AsyncWrite + Unpin>(
    reader: &mut R,
    writer: &mut W,
    lines: &Lines,
    tag: &str,
) -> Option<String> {
    writer.write_all(b"+ \r\n").await.ok()?;
    let identity = decoded(lines, Phase::Tls, &read_line(reader).await?);
    if identity == format!("user={USER}\x01auth=Bearer {TOKEN}\x01\x01") {
        return Some(format!("{tag} OK signed in\r\n"));
    }
    let error = BASE64.encode(GOOGLE_ERROR);
    writer
        .write_all(format!("+ {error}\r\n").as_bytes())
        .await
        .ok()?;
    let empty = read_line(reader).await?;
    record(lines, Phase::Tls, &format!("SASL {empty:?}"));
    Some(format!(
        "{tag} NO [AUTHENTICATIONFAILED] Invalid credentials (Failure)\r\n"
    ))
}
