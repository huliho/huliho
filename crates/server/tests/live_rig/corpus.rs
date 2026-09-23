// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The seeded corpus: RFC 5322 messages in threads of four, the same
//! for every target and every run, one second apart so the newest has
//! the highest number, each carrying a marker header a later run clears
//! by.

use std::fmt::Write as _;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

/// The header every corpus message carries.
pub const MARKER: (&str, &str) = ("X-Huliho-Live", "suite");

/// The header that tells this run's messages from an earlier run's: a
/// server that knows an email by the hash of its bytes would read the
/// same message appended again as one it already had.
const RUN_HEADER: &str = "X-Huliho-Run";

/// The messages one thread holds: a root and its replies.
pub const THREAD_SIZE: u32 = 4;

/// One second per message, so a corpus fits the one day the dates are
/// spelled in.
pub const SECONDS_PER_DAY: u32 = 86_400;
const SECONDS_PER_HOUR: u32 = 3600;
const SECONDS_PER_MINUTE: u32 = 60;

const DOMAIN: &str = "live.example";

/// This run's mark: the moment it started, in milliseconds.
fn run() -> &'static str {
    static RUN: OnceLock<String> = OnceLock::new();
    RUN.get_or_init(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .to_string()
    })
}

/// One message of the corpus by its number, counted from one.
#[derive(Debug, Clone, Copy)]
pub struct Seed(pub u32);

impl Seed {
    /// The number of the thread's root; a root is its own.
    pub fn root(self) -> u32 {
        (self.0 - 1) / THREAD_SIZE * THREAD_SIZE + 1
    }

    pub fn message_id(self) -> String {
        format!("<live{}@{DOMAIN}>", self.0)
    }

    pub fn subject(self) -> String {
        if self.root() == self.0 {
            format!("Thread {}", self.root())
        } else {
            format!("Re: Thread {}", self.root())
        }
    }

    /// The instant as INTERNALDATE spells it (RFC 3501 section 9): a
    /// day in the past, since Dovecot stamps its own time on a message
    /// appended with a date in the future.
    pub fn internal_date(self) -> String {
        let (hour, minute, second) = self.clock();
        format!("01-Jan-2024 {hour:02}:{minute:02}:{second:02} +0000")
    }

    /// The same instant as the Date header spells it (RFC 5322
    /// section 3.3).
    fn date(self) -> String {
        let (hour, minute, second) = self.clock();
        format!("Mon, 01 Jan 2024 {hour:02}:{minute:02}:{second:02} +0000")
    }

    fn clock(self) -> (u32, u32, u32) {
        assert!(self.0 < SECONDS_PER_DAY, "the corpus fits one day");
        (
            self.0 / SECONDS_PER_HOUR,
            self.0 / SECONDS_PER_MINUTE % SECONDS_PER_MINUTE,
            self.0 % SECONDS_PER_MINUTE,
        )
    }

    /// The message as APPEND takes it.
    pub fn rfc5322(self) -> String {
        let mut message = format!(
            "From: Sanne <sanne@{DOMAIN}>\r\nTo: mo@{DOMAIN}\r\nSubject: {}\r\nDate: {}\r\nMessage-ID: {}\r\n",
            self.subject(),
            self.date(),
            self.message_id()
        );
        if self.root() != self.0 {
            let parent = Seed(self.root()).message_id();
            let _ = write!(message, "In-Reply-To: {parent}\r\nReferences: {parent}\r\n");
        }
        let _ = write!(
            message,
            "{}: {}\r\n{RUN_HEADER}: {}\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nBody of message {}.\r\n",
            MARKER.0,
            MARKER.1,
            run(),
            self.0
        );
        message
    }
}

/// The number of a corpus message from its Message-ID; `None` for any
/// other mail.
pub fn number_of(message_id: &str) -> Option<u32> {
    message_id
        .trim_matches(['<', '>'])
        .strip_prefix("live")?
        .strip_suffix(&format!("@{DOMAIN}"))?
        .parse()
        .ok()
}
