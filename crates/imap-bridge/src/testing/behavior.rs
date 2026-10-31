// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The ways the scripted IMAP server misbehaves on a selected mailbox,
//! each a switch a test sets.

use std::time::Duration;

use super::command::Continuation;

/// How the server misbehaves on a selected mailbox.
#[derive(Debug, Clone, Copy, Default)]
pub struct Behavior {
    /// Lines nobody asked for ahead of the messages of every UID FETCH
    /// answer: this many EXISTS lines, then an EXPUNGE, a flag update
    /// for the first message, a message outside the range and, after
    /// the messages, a second line for the first one.
    pub volunteered: usize,
    /// A MODSEQ item on every FETCH line (RFC 7162 section 3.1.4), this
    /// value in place of the message's own.
    pub fetch_modseq: Option<u64>,
    /// EXAMINE of a missing folder and a folder command on one answer NO
    /// with the name in raw UTF-8, as Dovecot does.
    pub utf8_no: bool,
    /// A connection closes instead of answering once it has answered
    /// this many UID FETCH commands.
    pub drops_after: Option<usize>,
    /// A body fetch that names this UID answers NO.
    pub refuses_body_of: Option<u32>,
    /// Every window answer names this origin octet whatever was asked.
    pub misplaced_origin: Option<usize>,
    /// The lowest this many messages leave the folder behind the first
    /// UID SEARCH answer of a connection, an EXPUNGE line for each (RFC
    /// 3501 section 7.4.1).
    pub expunges: usize,
    /// EXAMINE answers without its EXISTS line.
    pub no_exists: bool,
    /// The three Gmail items on every header line and the labels on
    /// every flag line, asked for or not.
    pub gmail_items: bool,
    /// The UID FETCH with this index on a connection never gets an
    /// answer; the connection holds until the client gives up.
    pub stalls_at: Option<usize>,
    /// What a UID STORE comes to.
    pub storing: Storing,
    /// The STOREs a connection takes as usual before `storing` holds.
    pub storing_from: usize,
    /// The code a UID MOVE or a UID COPY answers with in place of the
    /// `COPYUID` of what it did, written as the wire carries it.
    pub copyuid: Option<&'static str>,
    /// What a UID MOVE, a UID COPY or a UID EXPUNGE comes to.
    pub moving: Storing,
    /// The SELECT with this index on a connection opens its folder
    /// `[READ-ONLY]`, as a folder whose rights changed under the client
    /// does; a MOVE into the folder before that still lands.
    pub read_only_at: Option<usize>,
    /// The SELECT with this index on a connection shows a UIDVALIDITY
    /// one past the folder's, as a folder deleted and made again under
    /// the client does; a MOVE into the folder before that names the
    /// folder's own.
    pub renumbered_at: Option<usize>,
    /// What a literal that waits for the continuation request gets.
    pub continuation: Continuation,
    /// The one charset a search runs in; a search in any other answers
    /// NO with the `BADCHARSET` code (RFC 3501 section 6.4.4).
    pub only_charset: Option<&'static str>,
    /// Every UID SEARCH answers one SEARCH line of at least this many
    /// bytes, its line break included.
    pub search_bytes: Option<usize>,
    /// Every UID SEARCH that reads the messages, for a text, a sender or
    /// a Gmail query, waits this long before its answer, as a server that
    /// reads every message for a word does; the UID list of a sync
    /// answers at once.
    pub search_delay: Option<Duration>,
    /// Every folder command answers NO with this response code, written
    /// as the wire carries it.
    pub folder_refusal: Option<&'static str>,
    /// A SUBSCRIBE of a folder that exists answers NO with this response
    /// code, written as the wire carries it; CREATE and UNSUBSCRIBE
    /// answer as usual.
    pub subscribe_refusal: Option<&'static str>,
    /// What a DELETE of a selectable folder with folders under it comes
    /// to.
    pub parent_delete: ParentDelete,
    /// What a LOGOUT comes to; the BYE and the tagged OK as it stands.
    pub logging_out: Storing,
}

/// How the server takes a DELETE of a folder with folders under it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ParentDelete {
    /// NO with `HASCHILDREN` (RFC 9051 section 6.3.5).
    #[default]
    Refuses,
    /// Its mail goes and its name stays as `\Noselect` (RFC 3501 section
    /// 6.3.4).
    KeepsName,
}

/// How the server takes a write command.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Storing {
    /// The command does what it says and the tagged OK follows.
    #[default]
    Stores,
    /// NO, with nothing changed.
    Refuses,
    /// BAD, with nothing changed.
    Bad,
    /// The connection closes instead of answering.
    Drops,
    /// No answer ever comes; the connection holds.
    Stalls,
}
