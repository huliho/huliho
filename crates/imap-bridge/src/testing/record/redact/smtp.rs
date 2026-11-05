// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The redaction of an SMTP conversation: the EHLO name, the AUTH line
//! and the words of every reply become fixed text, the EHLO answer keeps
//! its keywords under the fixed host, the addresses of the envelope map
//! as every address does and the message after DATA keeps its Message-ID
//! alone.

use super::Redactor;
use crate::testing::record::fixed::RECORDED_USER;
use crate::testing::record::fixed::smtp::{
    EHLO_NAME, fixed_auth, fixed_ehlo_host, fixed_reply, is_ehlo, plain_user, takes_message,
};
use crate::testing::transcript::{Conversation, Exchange, Line};

impl Redactor {
    /// One submission connection with every personal value replaced.
    pub(super) fn smtp_conversation(&mut self, conversation: &Conversation) -> Conversation {
        let mut message_next = false;
        let mut exchanges = Vec::new();
        for exchange in &conversation.exchanges {
            let command = exchange.client.as_deref();
            let client = command.map(|line| {
                if message_next {
                    self.message(line)
                } else {
                    self.smtp_command(line)
                }
            });
            let server = exchange
                .server
                .iter()
                .enumerate()
                .map(|(index, line)| self.reply(command, index, line))
                .collect();
            message_next = command
                .zip(exchange.server.last())
                .is_some_and(|(command, last)| takes_message(command, last.opening()));
            exchanges.push(Exchange { client, server });
        }
        Conversation { exchanges }
    }

    /// An EHLO says the fixed name, an AUTH signs the recorded user in
    /// with the live user mapped to it wherever it recurs, any other
    /// command has its addresses mapped.
    fn smtp_command(&mut self, line: &str) -> String {
        if is_ehlo(line) {
            return format!("EHLO {EHLO_NAME}");
        }
        if let Some(user) = plain_user(line) {
            self.addresses
                .entry(user)
                .or_insert_with(|| RECORDED_USER.to_owned());
            return fixed_auth();
        }
        self.text(line)
    }

    /// The EHLO answer keeps its keywords under the fixed host; any other
    /// reply keeps its code alone.
    fn reply(&mut self, command: Option<&str>, index: usize, line: &Line) -> Line {
        let text = line.opening();
        let redacted = match command {
            Some(command) if is_ehlo(command) && index == 0 => fixed_ehlo_host(text),
            Some(command) if is_ehlo(command) => self.text(text),
            _ => fixed_reply(text),
        };
        Line::text(&redacted)
    }
}

#[cfg(test)]
mod tests {
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD as BASE64;

    use super::*;
    use crate::testing::record::fixed::fill;

    fn exchange(client: Option<&str>, server: &[&str]) -> Exchange {
        Exchange {
            client: client.map(str::to_owned),
            server: server.iter().map(|line| Line::text(line)).collect(),
        }
    }

    fn clients(conversation: &Conversation) -> Vec<Option<&str>> {
        conversation
            .exchanges
            .iter()
            .map(|exchange| exchange.client.as_deref())
            .collect()
    }

    fn replies(conversation: &Conversation, exchange: usize) -> Vec<&str> {
        conversation.exchanges[exchange]
            .server
            .iter()
            .map(Line::opening)
            .collect()
    }

    #[test]
    fn a_send_loses_its_names_credential_addresses_and_message_rfc5321_4_1() {
        let auth = format!(
            "AUTH PLAIN {}",
            BASE64.encode("\0eric@gmail.com\0app password")
        );
        let message = "Message-ID: <d1@example.test>\r\nFrom: Eric <eric@gmail.com>\r\nTo: <mo@mail.example>\r\nSubject: Geheim\r\n\r\nDe tekst\r\n";
        let recorded = Conversation {
            exchanges: vec![
                exchange(None, &["220 smtp.gmail.com ESMTP k4mb12345678abc - gsmtp"]),
                exchange(
                    Some("EHLO Erics-MacBook.local"),
                    &[
                        "250-smtp.gmail.com at your service, [203.0.113.9]",
                        "250-SIZE 35882577",
                        "250-AUTH LOGIN PLAIN XOAUTH2",
                        "250 SMTPUTF8",
                    ],
                ),
                exchange(Some(&auth), &["235 2.7.0 Accepted"]),
                exchange(
                    Some("MAIL FROM:<eric@gmail.com>"),
                    &["250 2.1.0 OK k4mb12345678abc - gsmtp"],
                ),
                exchange(
                    Some("RCPT TO:<mo@mail.example>"),
                    &["250 2.1.5 OK k4mb12345678abc - gsmtp"],
                ),
                exchange(Some("DATA"), &["354  Go ahead k4mb12345678abc - gsmtp"]),
                exchange(Some(message), &["250 2.0.0 OK  1762300000 k4mb - gsmtp"]),
                exchange(Some("QUIT"), &["221 2.0.0 closing connection k4mb - gsmtp"]),
            ],
        };
        let redacted = Redactor::default().smtp_conversation(&recorded);
        let fixed_auth = fixed_auth();
        let expected_message = format!(
            "Message-ID: <d1@example.test>\r\n\r\n{}",
            fill(message.len() - "Message-ID: <d1@example.test>\r\n\r\n".len())
        );
        assert_eq!(
            clients(&redacted),
            [
                None,
                Some("EHLO client.example.test"),
                Some(fixed_auth.as_str()),
                Some("MAIL FROM:<sanne@example.test>"),
                Some("RCPT TO:<user2@example.test>"),
                Some("DATA"),
                Some(expected_message.as_str()),
                Some("QUIT"),
            ]
        );
        assert_eq!(replies(&redacted, 0), ["220 ready"]);
        assert_eq!(
            replies(&redacted, 1),
            [
                "250-smtp.example.test ready",
                "250-SIZE 35882577",
                "250-AUTH LOGIN PLAIN XOAUTH2",
                "250 SMTPUTF8",
            ]
        );
        assert_eq!(replies(&redacted, 2), ["235 2.7.0 ready"]);
        assert_eq!(replies(&redacted, 5), ["354 ready"]);
        assert_eq!(replies(&redacted, 6), ["250 2.0.0 ready"]);
        assert_eq!(replies(&redacted, 7), ["221 2.0.0 ready"]);
        let json = serde_json::to_string(&redacted).unwrap();
        for word in [
            "gmail.com",
            "app password",
            "MacBook",
            "203.0.113.9",
            "Geheim",
            "De tekst",
            "k4mb",
        ] {
            assert!(!json.contains(word), "{word} remains: {json}");
        }
    }

    #[test]
    fn a_data_the_server_refused_is_followed_by_a_command_not_a_message() {
        let recorded = Conversation {
            exchanges: vec![
                exchange(None, &["220 ready"]),
                exchange(Some("DATA"), &["503 5.5.1 MAIL first"]),
                exchange(Some("QUIT"), &["221 2.0.0 bye"]),
            ],
        };
        let redacted = Redactor::default().smtp_conversation(&recorded);
        assert_eq!(clients(&redacted), [None, Some("DATA"), Some("QUIT")]);
        assert_eq!(replies(&redacted, 1), ["503 5.5.1 ready"]);
    }
}
