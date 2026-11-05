// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The fixed shapes of a redacted SMTP conversation and how its lines
//! are read: the EHLO name, the one AUTH line, the head a reply keeps
//! and where a message follows a DATA.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;

use super::{GREETING, RECORDED_USER};
use crate::testing::PASSWORD;
use crate::testing::smtp::HOST;

/// The name the redacted EHLO says; the replayer reads an EHLO by its
/// verb alone, since every machine says its own.
pub const EHLO_NAME: &str = "client.example.test";

/// A reply opens with a three-digit code (RFC 5321 section 4.2.1).
const CODE_CHARS: usize = 3;

/// The code that asks for the message after DATA (RFC 5321 section
/// 4.2.3).
pub const START_INPUT: &str = "354";

/// The verb of a command, in upper case.
#[must_use]
pub fn verb(line: &str) -> String {
    line.split(' ')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase()
}

/// Whether the line opens a conversation with EHLO or HELO.
#[must_use]
pub fn is_ehlo(line: &str) -> bool {
    matches!(verb(line).as_str(), "EHLO" | "HELO")
}

/// The one AUTH line of a redacted conversation: PLAIN with its initial
/// response (RFC 4954 section 4, RFC 4616).
#[must_use]
pub fn fixed_auth() -> String {
    let identity = format!("\0{RECORDED_USER}\0{PASSWORD}");
    format!("AUTH PLAIN {}", BASE64.encode(identity))
}

/// The user an AUTH PLAIN line with its initial response signs in;
/// `None` for any other line.
#[must_use]
pub fn plain_user(line: &str) -> Option<String> {
    let (mechanism, initial) = line.strip_prefix("AUTH ")?.split_once(' ')?;
    if !mechanism.eq_ignore_ascii_case("PLAIN") {
        return None;
    }
    let decoded = BASE64.decode(initial.trim()).ok()?;
    let identity = String::from_utf8(decoded).ok()?;
    let mut parts = identity.split('\0');
    let (_authorization, user) = (parts.next()?, parts.next()?);
    Some(user.to_owned())
}

/// Whether the next command is the message: a DATA the server answered
/// with 354.
#[must_use]
pub fn takes_message(command: &str, reply: &str) -> bool {
    verb(command) == "DATA" && reply.starts_with(START_INPUT)
}

/// Whether more lines of the same reply follow (RFC 5321 section 4.2.1).
#[must_use]
pub fn reply_continues(line: &str) -> bool {
    line.as_bytes().get(CODE_CHARS) == Some(&b'-')
}

/// The code with its separator; the whole line where it is shorter.
fn code(line: &str) -> &str {
    let head = CODE_CHARS + 1;
    if line.is_char_boundary(head) {
        &line[..head]
    } else {
        line
    }
}

/// Whether the word is an enhanced status code of class, subject and
/// detail (RFC 3463 section 2).
fn is_status(word: &str) -> bool {
    let parts: Vec<&str> = word.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

/// The head of a reply (its code with its separator plus the enhanced
/// status code where one follows) and the words after it.
#[must_use]
pub fn reply_head(line: &str) -> (&str, &str) {
    let mut head = code(line).len();
    let first = line[head..].split(' ').next().unwrap_or_default();
    if is_status(first) {
        head += first.len();
    }
    (&line[..head], line[head..].trim_start())
}

/// The same reply with its words replaced.
#[must_use]
pub fn fixed_reply(line: &str) -> String {
    let (head, _) = reply_head(line);
    if head.ends_with([' ', '-']) {
        format!("{head}{GREETING}")
    } else {
        format!("{head} {GREETING}")
    }
}

/// Whether a reply is one the redaction left.
#[must_use]
pub fn is_fixed_reply(line: &str) -> bool {
    fixed_reply(line) == line
}

/// The first line of an EHLO answer under the fixed host, its code and
/// separator kept (RFC 5321 section 4.1.1.1).
#[must_use]
pub fn fixed_ehlo_host(line: &str) -> String {
    format!("{}{HOST} {GREETING}", code(line))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reply_splits_into_its_head_and_its_words_rfc5321_4_2_rfc3463() {
        assert_eq!(
            reply_head("250 2.1.0 OK k4mb - gsmtp"),
            ("250 2.1.0", "OK k4mb - gsmtp")
        );
        assert_eq!(reply_head("354  Go ahead"), ("354 ", "Go ahead"));
        assert_eq!(reply_head("250-SIZE 35882577"), ("250-", "SIZE 35882577"));
        assert_eq!(reply_head("220"), ("220", ""));
        assert_eq!(fixed_reply("250 2.1.0 OK k4mb - gsmtp"), "250 2.1.0 ready");
        assert_eq!(fixed_reply("354  Go ahead"), "354 ready");
        assert_eq!(fixed_reply("220"), "220 ready");
        assert!(is_fixed_reply("221 2.0.0 ready") && !is_fixed_reply("221 2.0.0 bye"));
        assert!(reply_continues("250-PIPELINING") && !reply_continues("250 SMTPUTF8"));
        assert_eq!(
            fixed_ehlo_host("250-smtp.gmail.com at your service, [203.0.113.9]"),
            "250-smtp.example.test ready"
        );
    }

    #[test]
    fn the_fixed_auth_line_names_the_recorded_user_rfc4616() {
        assert_eq!(plain_user(&fixed_auth()).as_deref(), Some(RECORDED_USER));
        let live = format!(
            "AUTH PLAIN {}",
            BASE64.encode("\0eric@gmail.com\0app password")
        );
        assert_eq!(plain_user(&live).as_deref(), Some("eric@gmail.com"));
        assert_eq!(plain_user("AUTH LOGIN"), None);
        assert_eq!(plain_user("AUTH PLAIN"), None);
        assert_eq!(plain_user("MAIL FROM:<a@b.c>"), None);
    }

    #[test]
    fn a_message_follows_a_data_the_server_took_rfc5321_4_1_1_4() {
        assert!(takes_message("DATA", "354 ready"));
        assert!(!takes_message("DATA", "503 5.5.1 bad sequence"));
        assert!(!takes_message("RSET", "354 ready"));
        assert!(is_ehlo("EHLO mac.local") && is_ehlo("helo x") && !is_ehlo("QUIT"));
        assert_eq!(verb("mail FROM:<a@b.c>"), "MAIL");
    }
}
