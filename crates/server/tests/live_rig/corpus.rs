// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The seeded corpus: RFC 5322 messages in threads of four, the same
//! for every target and every run, one second apart so the newest has
//! the highest number, each carrying a marker header a later run clears
//! by. A message's number picks the word its body mentions and its
//! sender, so a search knows which messages it has to find.

use std::fmt::Write as _;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// The header every corpus message carries.
pub const MARKER: (&str, &str) = ("X-Huliho-Live", "suite");

/// The header that tells every appended copy from any other, in this run
/// or an earlier one: a server that knows an email by the hash of its
/// bytes would read the same message appended again as one it already
/// had.
const RUN_HEADER: &str = "X-Huliho-Run";

/// The messages one thread holds: a root and its replies.
pub const THREAD_SIZE: u32 = 4;

/// One second per message, so a corpus fits the one day the dates are
/// spelled in.
pub const SECONDS_PER_DAY: u32 = 86_400;
const SECONDS_PER_HOUR: u32 = 3600;
const SECONDS_PER_MINUTE: u32 = 60;

const DOMAIN: &str = "live.example";

/// The words the bodies mention, message `n` the one at `n` modulo their
/// count. No other text of the corpus contains one of them, and there
/// are more words than a thread holds messages, so no thread holds two
/// messages that share a word.
pub const SEARCH_WORDS: [&str; 6] = [
    "quillwort",
    "horsetail",
    "clubmoss",
    "liverwort",
    "spikemoss",
    "bladderfern",
];

/// The senders, message `n` from the one at `n` modulo their count: a
/// name and the surname that is also the address's local part, which no
/// other text of the corpus contains.
pub const SENDERS: [(&str, &str); 7] = [
    ("Tomas Lindqvist", "lindqvist"),
    ("Ada Okafor", "okafor"),
    ("Bram Haverkamp", "haverkamp"),
    ("Lucia Castellano", "castellano"),
    ("Niamh Brannigan", "brannigan"),
    ("Kenji Takahashi", "takahashi"),
    ("Odile Moreau", "moreau"),
];

/// The item at `number` modulo the list's length.
fn cycled<T: Copy>(items: &[T], number: u32) -> T {
    items[usize::try_from(number).unwrap() % items.len()]
}

/// What the rich message says in its HTML part, the image it shows and
/// the attachment it carries.
pub const RICH_WORDS: &str = "Hello from the rich message";
pub const RICH_IMAGE_CID: &str = "logo@live.example";
pub const RICH_IMAGE_NAME: &str = "logo.png";
pub const RICH_NOTES_NAME: &str = "notes.txt";
pub const RICH_NOTES: &str = "Notes of the rich message.";

/// A PNG of one transparent pixel under base64.
const PIXEL_PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

/// The mark of one appended copy: the moment the run started in
/// milliseconds and the count of the copies made before this one.
fn stamp() -> String {
    static RUN: OnceLock<u128> = OnceLock::new();
    static COPIES: AtomicU64 = AtomicU64::new(0);
    let run = RUN.get_or_init(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    });
    format!("{run}-{}", COPIES.fetch_add(1, Ordering::Relaxed))
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

    /// The same instant as a `UTCDate` (RFC 8620 section 1.4), which is
    /// how a filter names it.
    pub fn received_at(self) -> String {
        let (hour, minute, second) = self.clock();
        format!("2024-01-01T{hour:02}:{minute:02}:{second:02}Z")
    }

    /// The word the body mentions.
    pub fn word(self) -> &'static str {
        cycled(&SEARCH_WORDS, self.0)
    }

    /// The sender's name and surname.
    pub fn sender(self) -> (&'static str, &'static str) {
        cycled(&SENDERS, self.0)
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

    /// The header fields every shape of the message shares, through
    /// MIME-Version.
    fn header(self) -> String {
        let (name, surname) = self.sender();
        let mut header = format!(
            "From: {name} <{surname}@{DOMAIN}>\r\nTo: mo@{DOMAIN}\r\nSubject: {}\r\nDate: {}\r\nMessage-ID: {}\r\n",
            self.subject(),
            self.date(),
            self.message_id()
        );
        if self.root() != self.0 {
            let parent = Seed(self.root()).message_id();
            let _ = write!(header, "In-Reply-To: {parent}\r\nReferences: {parent}\r\n");
        }
        let _ = write!(
            header,
            "{}: {}\r\n{RUN_HEADER}: {}\r\nMIME-Version: 1.0\r\n",
            MARKER.0,
            MARKER.1,
            stamp()
        );
        header
    }

    /// The message as APPEND takes it.
    pub fn rfc5322(self) -> String {
        format!(
            "{}Content-Type: text/plain; charset=utf-8\r\n\r\nBody of message {}, which mentions {}.\r\n",
            self.header(),
            self.0,
            self.word()
        )
    }

    /// The same message with an HTML body that carries a script and an
    /// image by `cid:`, the image beside it and a text attachment.
    pub fn rich(self) -> String {
        format!(
            "{}Content-Type: multipart/mixed; boundary=\"mixed\"\r\n\r\n--mixed\r\nContent-Type: multipart/related; boundary=\"related\"\r\n\r\n--related\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<html><body><p>{RICH_WORDS}</p><script>top.x=1</script><img src=\"cid:{RICH_IMAGE_CID}\"></body></html>\r\n--related\r\nContent-Type: image/png\r\nContent-ID: <{RICH_IMAGE_CID}>\r\nContent-Transfer-Encoding: base64\r\nContent-Disposition: inline; filename=\"{RICH_IMAGE_NAME}\"\r\n\r\n{PIXEL_PNG}\r\n--related--\r\n--mixed\r\nContent-Type: text/plain; name=\"{RICH_NOTES_NAME}\"\r\nContent-Disposition: attachment; filename=\"{RICH_NOTES_NAME}\"\r\n\r\n{RICH_NOTES}\r\n--mixed--\r\n",
            self.header()
        )
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
