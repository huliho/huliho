// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! One message on a signed-in submission connection (RFC 5321 section
//! 3.3): MAIL FROM, one RCPT TO per address with every reply read, DATA
//! with the message, QUIT. Every reply maps to a fixed outcome; the
//! server's text never travels.

use std::time::Duration;

use mail_send::smtp::message::Parameters;
use mail_send::{Error as ClientError, SmtpClient};
use smtp_proto::Response;
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;

use crate::message::{MAX_MESSAGE_BYTES, fits_address};

/// What DATA may take from its command to the reply after the message:
/// the download route's total, since a message of 40 MiB on a slow link
/// needs it.
pub const DATA_TIMEOUT: Duration = Duration::from_secs(600);

/// "Requested mail action aborted: exceeded storage allocation" (RFC
/// 1870 section 6).
const EXCEEDED_STORAGE: u16 = 552;

/// "Insufficient system storage", which a server answers with the
/// enhanced code 4.5.3 for too many recipients (RFC 3463 section 3.6).
const INSUFFICIENT_STORAGE: u16 = 452;

/// The enhanced codes the outcome reads: "Too many recipients" and
/// "Message too big for system" (RFC 3463 sections 3.6 and 3.4).
const TOO_MANY_RECIPIENTS: [u8; 3] = [4, 5, 3];
const MESSAGE_TOO_BIG: (u8, u8) = (3, 4);

/// A connection the host opened and signed in for one send.
pub struct Submission {
    client: SmtpClient<TlsStream<TcpStream>>,
    /// What every command but DATA may take.
    step: Duration,
    /// The largest message the server named in its EHLO answer (RFC 1870
    /// section 4); none where it named no SIZE or 0.
    size: Option<u64>,
}

/// Why a message did not go out. Nothing here carries the server's
/// words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendError {
    /// The server refused these recipients; no DATA went out.
    InvalidRecipients(Vec<String>),
    /// The server takes no more recipients than this count.
    TooManyRecipients(usize),
    /// The server refused the sender.
    ForbiddenMailFrom,
    /// The message passes what the server takes: the bound it named.
    TooLarge(u64),
    /// The server was not reached in time or asked to try later.
    Unavailable,
    /// Any other refusal.
    Refused,
}

impl Submission {
    pub(super) fn new(client: SmtpClient<TlsStream<TcpStream>>, size: usize) -> Self {
        let size = u64::try_from(size).ok().filter(|size| *size > 0);
        let step = client.timeout;
        Self { client, step, size }
    }

    /// The bound a refusal for size names: the server's SIZE where it
    /// named one, else the bridge's own.
    fn bound(&self) -> u64 {
        let own = u64::try_from(MAX_MESSAGE_BYTES).unwrap_or(u64::MAX);
        self.size.unwrap_or(own)
    }

    /// Sends the message from the sender to every recipient and quits. A
    /// recipient the server refuses with a permanent reply is collected
    /// while the rest still go, then the transaction is reset and no
    /// DATA goes out. An address outside the bridge's rule never reaches
    /// a command line.
    ///
    /// # Errors
    ///
    /// Returns why the message did not go out.
    pub async fn send(
        mut self,
        mail_from: &str,
        rcpt_to: &[String],
        message: &[u8],
    ) -> Result<(), SendError> {
        if !fits_address(mail_from) {
            return self.ended(Err(SendError::ForbiddenMailFrom)).await;
        }
        let unfit: Vec<String> = rcpt_to
            .iter()
            .filter(|address| !fits_address(address))
            .cloned()
            .collect();
        if !unfit.is_empty() {
            return self.ended(Err(SendError::InvalidRecipients(unfit))).await;
        }
        let weight = u64::try_from(message.len()).unwrap_or(u64::MAX);
        if self.size.is_some_and(|size| weight > size) {
            let bound = self.bound();
            return self.ended(Err(SendError::TooLarge(bound))).await;
        }
        let outcome = self.transaction(mail_from, rcpt_to, message).await;
        self.ended(outcome).await
    }

    async fn transaction(
        &mut self,
        mail_from: &str,
        rcpt_to: &[String],
        message: &[u8],
    ) -> Result<(), SendError> {
        let none = Parameters::new();
        if let Err(error) = self.client.mail_from(mail_from, &none).await {
            return Err(match reply(error) {
                Ok(reply) if too_big(&reply) => SendError::TooLarge(self.bound()),
                Ok(reply) if permanent(&reply) => SendError::ForbiddenMailFrom,
                _ => SendError::Unavailable,
            });
        }
        let mut refused = Vec::new();
        let mut passed = 0;
        for address in rcpt_to {
            let Err(error) = self.client.rcpt_to(address, &none).await else {
                passed += 1;
                continue;
            };
            match reply(error) {
                Ok(reply) if permanent(&reply) => refused.push(address.clone()),
                Ok(reply) if too_many(&reply) => {
                    let _reset = self.client.rset().await;
                    return Err(SendError::TooManyRecipients(passed));
                }
                _ => return Err(SendError::Unavailable),
            }
        }
        if !refused.is_empty() {
            let _reset = self.client.rset().await;
            return Err(SendError::InvalidRecipients(refused));
        }
        self.client.timeout = DATA_TIMEOUT;
        match self.client.data(message).await {
            Ok(()) => Ok(()),
            Err(error) => Err(match reply(error) {
                Ok(reply) if too_big(&reply) => SendError::TooLarge(self.bound()),
                Ok(reply) if permanent(&reply) => SendError::Refused,
                _ => SendError::Unavailable,
            }),
        }
    }

    /// QUIT under the step's timeout, whatever came before; a QUIT that
    /// fails changes nothing.
    async fn ended(mut self, outcome: Result<(), SendError>) -> Result<(), SendError> {
        self.client.timeout = self.step;
        let _ended = self.client.quit().await;
        outcome
    }
}

/// The server's reply where the library got one; any other failure is
/// the connection's.
fn reply(error: ClientError) -> Result<Response<String>, ()> {
    match error {
        ClientError::UnexpectedReply(reply) => Ok(reply),
        _ => Err(()),
    }
}

/// A permanent negative reply (RFC 5321 section 4.2.1).
fn permanent(reply: &Response<String>) -> bool {
    (500..600).contains(&reply.code)
}

/// A refusal for size: 552, or a permanent reply with X.3.4.
fn too_big(reply: &Response<String>) -> bool {
    let (subject, detail) = MESSAGE_TOO_BIG;
    reply.code == EXCEEDED_STORAGE
        || (permanent(reply) && reply.esc[1] == subject && reply.esc[2] == detail)
}

/// 452 with 4.5.3 (RFC 5321 section 4.5.3.1.10).
fn too_many(reply: &Response<String>) -> bool {
    reply.code == INSUFFICIENT_STORAGE && reply.esc == TOO_MANY_RECIPIENTS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(code: u16, esc: [u8; 3]) -> Response<String> {
        Response {
            code,
            esc,
            message: "the server's own words".to_owned(),
        }
    }

    #[test]
    fn the_replies_read_by_their_codes_rfc5321_4_2_rfc3463() {
        assert!(permanent(&response(550, [5, 1, 1])));
        assert!(!permanent(&response(451, [4, 3, 0])));
        assert!(too_big(&response(552, [5, 3, 4])));
        assert!(too_big(&response(552, [0, 0, 0])));
        assert!(too_big(&response(554, [5, 3, 4])));
        assert!(!too_big(&response(554, [5, 7, 1])));
        assert!(!too_big(&response(452, [4, 3, 4])));
        assert!(too_many(&response(452, [4, 5, 3])));
        assert!(!too_many(&response(452, [4, 3, 1])));
        assert!(!too_many(&response(552, [5, 5, 3])));
    }

    #[test]
    fn a_failure_without_a_reply_is_the_connections() {
        assert!(reply(ClientError::Timeout).is_err());
        let refused = reply(ClientError::UnexpectedReply(response(550, [5, 1, 1])));
        assert_eq!(refused.map(|reply| reply.code), Ok(550));
    }
}
