// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The scan of a send: the greeting and every reply outside the EHLO
//! answer keep their code alone, the EHLO answer names the fixed host,
//! and a client line is the fixed EHLO, the fixed AUTH, a command of the
//! envelope or the redacted message after DATA. Anything else is a
//! finding, a SASL answer included, since one has no fixed shape.

use super::{Place, header, note, text};
use crate::testing::record::fixed::fill;
use crate::testing::record::fixed::opaque_tokens;
use crate::testing::record::fixed::smtp::{
    EHLO_NAME, fixed_auth, fixed_ehlo_host, is_ehlo, is_fixed_reply, reply_head, takes_message,
    verb,
};
use crate::testing::transcript::{Conversation, Line, Wire};

/// The commands a client line may open with beside the fixed EHLO and
/// AUTH (RFC 5321 section 4.1.1).
const ENVELOPE_VERBS: [&str; 6] = ["MAIL", "RCPT", "DATA", "RSET", "NOOP", "QUIT"];

/// What the message a client sent opens with once redacted.
const MESSAGE_HEAD: &str = "Message-ID: ";

/// The findings of one send.
pub(super) fn conversation(
    session: usize,
    conversation: &Conversation,
    findings: &mut Vec<String>,
) {
    let mut message_next = false;
    for (exchange, recorded) in conversation.exchanges.iter().enumerate() {
        let place = Place {
            wire: Wire::Smtp,
            session,
            exchange,
        };
        let command = recorded.client.as_deref();
        match command {
            None if exchange == 0 => {}
            None => note(&place, findings, "an exchange without a command"),
            Some(line) if message_next => message(&place, line, findings),
            Some(line) => client_line(&place, line, findings),
        }
        server_lines(&place, command, &recorded.server, findings);
        message_next = command
            .zip(recorded.server.last())
            .is_some_and(|(command, last)| takes_message(command, last.opening()));
    }
}

fn client_line(place: &Place, line: &str, findings: &mut Vec<String>) {
    let verb = verb(line);
    if is_ehlo(line) {
        if line != format!("EHLO {EHLO_NAME}") {
            note(place, findings, "the EHLO line is not the fixed one");
        }
        return;
    }
    if verb == "AUTH" {
        if line != fixed_auth() {
            note(place, findings, "the AUTH line is not the fixed one");
        }
        return;
    }
    if !ENVELOPE_VERBS.contains(&verb.as_str()) {
        note(place, findings, "a client line of no known command");
        return;
    }
    text(place, line, findings);
}

/// The message keeps its Message-ID alone, in the fixed shape, over
/// filler.
fn message(place: &Place, message: &str, findings: &mut Vec<String>) {
    let Some((head, body)) = message.split_once("\r\n\r\n") else {
        note(place, findings, "a message a client sent keeps its text");
        return;
    };
    if !head.starts_with(MESSAGE_HEAD) || head.contains("\r\n") {
        note(place, findings, "a message a client sent keeps its header");
    }
    header(place, head, findings);
    text(place, head, findings);
    if body != fill(body.len()) {
        note(place, findings, "a message a client sent is not the filler");
    }
}

/// The lines a server answered one command with: the EHLO answer names
/// the fixed host on its first line and keeps its keywords on the rest;
/// any other reply, the greeting included, keeps its code alone.
fn server_lines(place: &Place, command: Option<&str>, lines: &[Line], findings: &mut Vec<String>) {
    for (index, line) in lines.iter().enumerate() {
        let reply = line.opening();
        match command {
            Some(command) if is_ehlo(command) && index == 0 => {
                if reply != fixed_ehlo_host(reply) {
                    note(place, findings, "the EHLO answer keeps its host");
                }
            }
            Some(command) if is_ehlo(command) => {
                // The keywords after the code, which an EHLO keyword can
                // otherwise read as a token with.
                let (_, keywords) = reply_head(reply);
                text(place, keywords, findings);
                for (start, end) in opaque_tokens(keywords) {
                    let token = &keywords[start..end];
                    note(
                        place,
                        findings,
                        &format!("an opaque token remains in the EHLO answer ({token})"),
                    );
                }
            }
            _ => {
                if !is_fixed_reply(reply) {
                    note(place, findings, "a reply keeps its words");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::scan;
    use super::*;
    use crate::testing::transcript::{Exchange, Transcript};

    fn exchange(client: Option<&str>, server: &[&str]) -> Exchange {
        Exchange {
            client: client.map(str::to_owned),
            server: server.iter().map(|line| Line::text(line)).collect(),
        }
    }

    fn transcript(exchanges: Vec<Exchange>) -> Transcript {
        Transcript {
            sessions: Vec::new(),
            smtp: vec![Conversation { exchanges }],
        }
    }

    fn kinds(findings: &[String]) -> Vec<&str> {
        findings
            .iter()
            .map(|finding| finding.split_once(": ").unwrap().1)
            .collect()
    }

    #[test]
    fn a_clean_send_has_no_findings() {
        let auth = fixed_auth();
        let message = format!("Message-ID: <d1@example.test>\r\n\r\n{}", fill(40));
        let clean = transcript(vec![
            exchange(None, &["220 ready"]),
            exchange(
                Some("EHLO client.example.test"),
                &[
                    "250-smtp.example.test ready",
                    "250-SIZE 35882577",
                    "250-AUTH LOGIN PLAIN XOAUTH2 PLAIN-CLIENTTOKEN OAUTHBEARER XOAUTH",
                    "250-ENHANCEDSTATUSCODES",
                    "250 SMTPUTF8",
                ],
            ),
            exchange(Some(&auth), &["235 2.7.0 ready"]),
            exchange(Some("MAIL FROM:<sanne@example.test>"), &["250 2.1.0 ready"]),
            exchange(Some("RCPT TO:<sanne@example.test>"), &["250 2.1.5 ready"]),
            exchange(Some("DATA"), &["354 ready"]),
            exchange(Some(&message), &["250 2.0.0 ready"]),
            exchange(Some("QUIT"), &["221 2.0.0 ready"]),
        ]);
        assert_eq!(scan(&clean), Vec::<String>::new());
    }

    #[test]
    fn every_kind_of_leak_in_a_send_is_a_finding_of_its_own() {
        let leaky = transcript(vec![
            exchange(None, &["220 smtp.gmail.com ESMTP ready"]),
            exchange(
                Some("EHLO Erics-MacBook.local"),
                &[
                    "250-smtp.gmail.com at your service, [203.0.113.9]",
                    "250-ID 5b1f17b1804b1-49fbbf104a9mb2431653045e9",
                    "250 SMTPUTF8",
                ],
            ),
            exchange(Some("AUTH LOGIN"), &["334 VXNlcm5hbWU6"]),
            exchange(Some("ZXJpY0BnbWFpbC5jb20="), &["334 UGFzc3dvcmQ6"]),
            exchange(
                Some("MAIL FROM:<eric@gmail.com>"),
                &["250 2.1.0 OK k4mb - gsmtp"],
            ),
            exchange(Some("DATA"), &["354 ready"]),
            exchange(
                Some("Message-ID: <abc@mail.gmail.com>\r\nSubject: Lunch\r\n\r\nDear Eric"),
                &["250 2.0.0 ready"],
            ),
        ]);
        let findings = scan(&leaky);
        assert!(
            findings
                .iter()
                .all(|finding| finding.starts_with("smtp session 0 exchange "))
        );
        assert_eq!(
            kinds(&findings),
            [
                "a reply keeps its words",
                "the EHLO line is not the fixed one",
                "the EHLO answer keeps its host",
                "an opaque token remains in the EHLO answer (5b1f17b1804b1-49fbbf104a9mb2431653045e9)",
                "the AUTH line is not the fixed one",
                "a reply keeps its words",
                "a client line of no known command",
                "a reply keeps its words",
                "an address remains: eric@gmail.com",
                "a reply keeps its words",
                "a message a client sent keeps its header",
                "a message id remains: <abc@mail.gmail.com>",
                "a subject remains: Lunch",
                "an address remains: abc@mail.gmail.com",
                "a message a client sent is not the filler",
            ]
        );
        assert!(!findings.iter().any(|finding| finding.contains("ZXJpY0")));
    }
}
