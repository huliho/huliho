// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! UID SEARCH of the scripted server over a window of sequence numbers
//! with the keys the bridge sends (RFC 3501 section 6.4.4): `CHARSET`,
//! `TEXT`, `FROM`, `UNSEEN`, `FLAGGED`, `SINCE` and `BEFORE`, a string as
//! an atom, quoted or a literal; on a Gmail account `X-GM-RAW` and
//! `X-GM-LABELS` as well. Every key must match.

use std::fmt::Write as _;

use super::{Conversation, Message};
use crate::testing::command::{Command, Part};
use crate::testing::mailboxes::{Extension, Mailboxes, unquote};

/// The months an IMAP date names (RFC 3501 section 9).
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The charsets the server searches in.
const CHARSETS: [&str; 2] = ["US-ASCII", "UTF-8"];

/// The bytes of a SEARCH line before its first UID and after its last.
const SEARCH_FRAME_BYTES: usize = "* SEARCH\r\n".len();

/// One key of a search as the server read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    /// `<low>:<high>` in sequence numbers.
    Window(usize, usize),
    Text(String),
    From(String),
    Unseen,
    Flagged,
    /// SINCE, as the date was written.
    Since(String),
    /// BEFORE, as the date was written.
    Before(String),
    GmailRaw(String),
    GmailLabels(String),
}

/// A search as the server read it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Search {
    pub charset: Option<String>,
    pub keys: Vec<Key>,
}

impl Search {
    /// Whether the search reads the messages: a text, a sender or a Gmail
    /// query is matched against them; a window, a flag or a date is not.
    #[must_use]
    pub fn reads_messages(&self) -> bool {
        self.keys
            .iter()
            .any(|key| matches!(key, Key::Text(_) | Key::From(_) | Key::GmailRaw(_)))
    }
}

/// One word of a command: an atom, or a string that came quoted or as a
/// literal.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Word {
    Atom(String),
    String(String),
}

impl Word {
    fn atom(&self) -> Option<&str> {
        match self {
            Self::Atom(atom) => Some(atom),
            Self::String(_) => None,
        }
    }

    /// An astring: an atom or a string (RFC 3501 section 9).
    fn into_string(self) -> String {
        match self {
            Self::Atom(text) | Self::String(text) => text,
        }
    }
}

/// The words of a command in order, the tag first; `None` for a quoted
/// string that never closes.
fn words(command: &Command) -> Option<Vec<Word>> {
    let mut words = Vec::new();
    for (index, part) in command.parts.iter().enumerate() {
        let text = match part {
            Part::Literal(literal) => {
                words.push(Word::String(literal.clone()));
                continue;
            }
            Part::Text(text) => text,
        };
        let literal_follows = matches!(command.parts.get(index + 1), Some(Part::Literal(_)));
        let mut rest = text.trim_start();
        while !rest.is_empty() {
            if rest.starts_with('"') {
                let (string, after) = unquote(rest)?;
                words.push(Word::String(string));
                rest = after.trim_start();
                continue;
            }
            let (atom, after) = rest.split_once(' ').unwrap_or((rest, ""));
            let marker = after.is_empty() && literal_follows && atom.starts_with('{');
            if !marker {
                words.push(Word::Atom(atom.to_owned()));
            }
            rest = after.trim_start();
        }
    }
    Some(words)
}

/// The search a `UID SEARCH` command asks; `None` for one the server
/// cannot read.
#[must_use]
pub fn parse(command: &Command) -> Option<Search> {
    let mut words = words(command)?.into_iter().skip(1);
    let verb = (words.next()?, words.next()?);
    if verb
        != (
            Word::Atom("UID".to_owned()),
            Word::Atom("SEARCH".to_owned()),
        )
    {
        return None;
    }
    let mut words = words.peekable();
    let mut search = Search::default();
    if words.peek().and_then(Word::atom) == Some("CHARSET") {
        words.next();
        search.charset = Some(words.next()?.into_string());
    }
    while let Some(word) = words.next() {
        let atom = word.atom()?.to_ascii_uppercase();
        let key = match atom.as_str() {
            "TEXT" => Key::Text(words.next()?.into_string()),
            "FROM" => Key::From(words.next()?.into_string()),
            "UNSEEN" => Key::Unseen,
            "FLAGGED" => Key::Flagged,
            "SINCE" => Key::Since(date(words.next()?)?),
            "BEFORE" => Key::Before(date(words.next()?)?),
            "X-GM-RAW" => Key::GmailRaw(words.next()?.into_string()),
            "X-GM-LABELS" => Key::GmailLabels(label(words.next()?)?),
            window => {
                let (low, high) = window.split_once(':')?;
                Key::Window(low.parse().ok()?, high.parse().ok()?)
            }
        };
        search.keys.push(key);
    }
    Some(search)
}

impl Conversation {
    /// The answer to a UID SEARCH with its tagged line: BAD for a search
    /// the server cannot read, a number the folder does not hold (RFC
    /// 3501 section 9) or a Gmail key while the extension is off; NO with
    /// `BADCHARSET` for a charset it does not search in. The first search
    /// of a connection is followed by the expunges the behavior asks for.
    pub(in crate::testing) fn search(
        &mut self,
        mailboxes: &Mailboxes,
        command: &Command,
        tag: &str,
    ) -> String {
        let Some(search) = parse(command) else {
            return format!("{tag} BAD no such search\r\n");
        };
        let behavior = mailboxes.behavior;
        if let Some(charset) = &search.charset {
            let known = match behavior.only_charset {
                Some(only) => only.eq_ignore_ascii_case(charset),
                None => CHARSETS
                    .iter()
                    .any(|known| known.eq_ignore_ascii_case(charset)),
            };
            if !known {
                return format!("{tag} NO [BADCHARSET (US-ASCII)] not here\r\n");
            }
        }
        let gmail = |key: &Key| matches!(key, Key::GmailRaw(_) | Key::GmailLabels(_));
        if search.keys.iter().any(gmail) && !mailboxes.has(Extension::Gmail) {
            return format!("{tag} BAD not offered\r\n");
        }
        let found = self
            .folder(mailboxes)
            .and_then(|folder| found(&folder.mail, &search).map(|uids| (folder, uids)));
        let Some((folder, uids)) = found else {
            return format!("{tag} BAD no such sequence number\r\n");
        };
        let mut lines = String::from("* SEARCH");
        for uid in uids {
            let _ = write!(lines, " {uid}");
        }
        if let Some(bytes) = behavior.search_bytes {
            let pairs = bytes
                .saturating_sub(SEARCH_FRAME_BYTES)
                .div_ceil(" 1".len());
            lines.push_str(&" 1".repeat(pairs));
        }
        lines.push_str("\r\n");
        let leaving = behavior.expunges.min(folder.mail.len());
        if !self.searched && leaving > 0 {
            mailboxes.expunge(&folder.name, leaving);
            lines.push_str(&"* 1 EXPUNGE\r\n".repeat(leaving));
        }
        self.searched = true;
        format!("{lines}{tag} OK done\r\n")
    }
}

/// The UIDs of the messages in the window that every key matches; `None`
/// without a window the folder holds.
fn found(mail: &[Message], search: &Search) -> Option<Vec<u32>> {
    let (low, high) = search.keys.iter().find_map(|key| match key {
        Key::Window(low, high) => Some((*low, *high)),
        _ => None,
    })?;
    let window = mail.get(low.checked_sub(1)?..high)?;
    let matching = window
        .iter()
        .filter(|message| search.keys.iter().all(|key| matches(message, key)));
    Some(matching.map(|message| message.uid).collect())
}

/// Whether one key matches the message; a string matches as a substring
/// without regard to case (RFC 3501 section 6.4.4).
fn matches(message: &Message, key: &Key) -> bool {
    let text = || format!("{}{}", message.header, message.body);
    match key {
        Key::Window(..) => true,
        Key::Text(text_key) => holds(&text(), text_key),
        Key::From(sender) => from_field(&message.header).is_some_and(|from| holds(from, sender)),
        Key::Unseen => !has_flag(message, "\\Seen"),
        Key::Flagged => has_flag(message, "\\Flagged"),
        Key::Since(written) => day(&message.internal_date) >= day(written),
        Key::Before(written) => day(&message.internal_date) < day(written),
        Key::GmailRaw(raw) => raw_terms(raw).iter().all(|term| {
            let bare = term.replace('"', "");
            match bare.strip_prefix("from:") {
                Some(sender) => from_field(&message.header).is_some_and(|from| holds(from, sender)),
                None => holds(&text(), &bare),
            }
        }),
        Key::GmailLabels(label) => message
            .labels
            .iter()
            .any(|held| held.eq_ignore_ascii_case(label)),
    }
}

fn holds(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

fn has_flag(message: &Message, flag: &str) -> bool {
    message
        .flags
        .iter()
        .any(|held| held.eq_ignore_ascii_case(flag))
}

/// The value of the From field of a header block.
fn from_field(header: &str) -> Option<&str> {
    header.split("\r\n").find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("From").then_some(value)
    })
}

/// A label of an X-GM-LABELS key. A backslash in an atom is no IMAP
/// syntax, and Gmail answers a system label written so with BAD or with
/// nothing, so the server refuses it.
fn label(word: Word) -> Option<String> {
    let atom = word.atom().is_some();
    let label = word.into_string();
    (!(atom && label.starts_with('\\'))).then_some(label)
}

/// A date the server can read, as it was written.
fn date(word: Word) -> Option<String> {
    let date = word.into_string();
    day(&date).map(|_| date)
}

/// The day of an IMAP date, its time left out, as year, month and day;
/// `None` for one the server cannot read.
fn day(date: &str) -> Option<(u32, usize, u32)> {
    let date = date.split(' ').next()?;
    let mut fields = date.split('-');
    let (day, month, year) = (fields.next()?, fields.next()?, fields.next()?);
    let month = MONTHS.iter().position(|name| *name == month)?;
    Some((year.parse().ok()?, month, day.parse().ok()?))
}

/// The terms of an X-GM-RAW string: whitespace apart, a run in double
/// quotes one term.
fn raw_terms(raw: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for c in raw.chars() {
        if c.is_whitespace() && !quoted {
            terms.extend((!current.is_empty()).then(|| std::mem::take(&mut current)));
            continue;
        }
        if c == '"' {
            quoted = !quoted;
        }
        current.push(c);
    }
    terms.extend((!current.is_empty()).then_some(current));
    terms
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(parts: &[Part]) -> Command {
        Command {
            parts: parts.to_vec(),
        }
    }

    fn text(text: &str) -> Part {
        Part::Text(text.to_owned())
    }

    #[test]
    fn a_search_reads_its_keys_with_strings_as_atoms_quoted_or_literals_rfc3501_9() {
        let read = parse(&command(&[
            text("A1 UID SEARCH CHARSET UTF-8 1:9 TEXT {5+}"),
            Part::Literal("a\r\n\"b".to_owned()),
            text(" FROM \"anna\" UNSEEN SINCE 1-Feb-2026 X-GM-LABELS \"\\\\Inbox\""),
        ]))
        .unwrap();
        assert_eq!(read.charset.as_deref(), Some("UTF-8"));
        assert_eq!(
            read.keys,
            [
                Key::Window(1, 9),
                Key::Text("a\r\n\"b".to_owned()),
                Key::From("anna".to_owned()),
                Key::Unseen,
                Key::Since("1-Feb-2026".to_owned()),
                Key::GmailLabels("\\Inbox".to_owned()),
            ]
        );
        for unread in [
            "A1 UID SEARCH OR 1 2",
            "A1 SEARCH 1:2",
            "A1 UID SEARCH TEXT",
            "A1 UID SEARCH 1:2 SINCE 1-Foo-2026",
            "A1 UID SEARCH 1:2 X-GM-LABELS \\Inbox",
        ] {
            assert_eq!(parse(&command(&[text(unread)])), None, "{unread}");
        }
    }

    #[test]
    fn a_search_reads_the_messages_for_a_text_a_sender_or_a_raw_string_alone() {
        let reads = |line: &str| parse(&command(&[text(line)])).unwrap().reads_messages();
        assert!(reads("A1 UID SEARCH 1:9 TEXT plan"));
        assert!(reads("A1 UID SEARCH 1:9 FROM anna"));
        assert!(reads("A1 UID SEARCH 1:9 X-GM-RAW plan"));
        assert!(!reads("A1 UID SEARCH 1:9"));
        assert!(!reads("A1 UID SEARCH 1:9 UNSEEN SINCE 1-Feb-2026"));
    }

    #[test]
    fn a_raw_string_splits_on_whitespace_outside_quotes() {
        assert_eq!(
            raw_terms("invoice  from:\"Anna Jansen\" \"q3 plan\""),
            ["invoice", "from:\"Anna Jansen\"", "\"q3 plan\""]
        );
        assert_eq!(day("01-Jan-2026 00:00:05 +0000"), Some((2026, 0, 1)));
        assert_eq!(day("1-Feb-2026"), Some((2026, 1, 1)));
        assert_eq!(day("1-Foo-2026"), None);
    }
}
