// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! CREATE, RENAME, DELETE, SUBSCRIBE, UNSUBSCRIBE and MYRIGHTS of the
//! scripted server over the folders as they stand. A refusal carries the
//! code a real server sends (RFC 5530, RFC 9051 section 7.1, RFC 6154
//! section 3), which the client library's parser drops. On a Gmail
//! account a label folder takes its label along: the mail stays in All
//! Mail, a name that stands is refused with the name in raw UTF-8 and one
//! that reads as a label under Gmail's fold is refused as well, as is one
//! with white space in a row or a path past Gmail's limits.

use super::{Extension, Mailboxes, quote, unquote};
use crate::testing::behavior::{ParentDelete, Renaming};
use crate::testing::folder::{Folder, MyRights};
use crate::utf7;

/// The rights MYRIGHTS names on a folder that names none of its own:
/// an owner's, as Cyrus answers for INBOX.
pub const OWNER_RIGHTS: &str = "lrswipkxtecdan";

const ALREADY_EXISTS: &str = "ALREADYEXISTS";
const NONEXISTENT: &str = "NONEXISTENT";
const HAS_CHILDREN: &str = "HASCHILDREN";
const USE_ATTRIBUTE: &str = "USEATTR";

/// The folders Gmail keeps for itself sit under this name.
const GMAIL_SYSTEM: &str = "[Gmail]";

/// Greek capital sigma, which Gmail tells apart from its small letter in a
/// name such as `Greek Σ`; the script does so in every name.
const CAPITAL_SIGMA: char = '\u{3a3}';

/// The capital sharp s, which Gmail tells apart from its small letter.
const CAPITAL_SHARP_S: char = '\u{1e9e}';

/// Full-width ASCII, which Gmail reads as ASCII.
const FULL_WIDTH: std::ops::RangeInclusive<u32> = 0xff01..=0xff5e;

/// How far full-width ASCII stands from ASCII.
const FULL_WIDTH_OFFSET: u32 = 0xfee0;

/// The ASCII marks Gmail reads as a space between two letters.
const READ_AS_SPACE: &str = "\"&(){|}-";

/// The UTF-16 code units of a whole path Gmail takes.
const MAX_PATH_UNITS: usize = 225;

/// The bytes of UTF-8 of a whole path Gmail takes.
const MAX_PATH_BYTES: usize = 255;

/// Gmail's NO to two white space characters in a row.
const EXCESS_WHITE_SPACE: &str = "[CANNOT] Folder contains excess whitespace";

/// Gmail's NO to a whole path past `MAX_PATH_UNITS`.
const NOT_ALLOWED: &str = "[CANNOT] Folder name is not allowed.";

/// Gmail's NO to a whole path past `MAX_PATH_BYTES`.
const SYSTEM_ERROR: &str = "System Error";

/// Why a folder command failed: BAD for a form the script does not
/// read, NO with the code a server would send.
enum Failure {
    Bad,
    No(Option<&'static str>),
    /// On Gmail, a name that reads as another under its fold.
    Variant,
    /// On Gmail, a name it refuses whatever stands beside it, in its words.
    Said(&'static str),
}

/// The quoted names after a verb with their escapes undone, then what
/// follows them. `None` when the first word is not a quoted string.
#[must_use]
pub(crate) fn arguments(rest: &str) -> Option<(Vec<String>, &str)> {
    let mut names = Vec::new();
    let mut rest = rest;
    while rest.starts_with('"') {
        let (name, after) = unquote(rest)?;
        names.push(name);
        rest = after.strip_prefix(' ').unwrap_or(after);
    }
    (!names.is_empty()).then_some((names, rest))
}

impl Mailboxes {
    /// The answer to one folder command, its tagged line included.
    /// MYRIGHTS without the ACL extension is a command the server does
    /// not know; a missing name may be named in raw UTF-8, as Dovecot
    /// does.
    pub(super) fn folder_command(&self, verb: &str, command: &str, tag: &str) -> String {
        if verb == "MYRIGHTS" && !self.has(Extension::Acl) {
            return format!("{tag} BAD unknown command\r\n");
        }
        if let Some(code) = self.behavior.folder_refusal {
            return format!("{tag} NO [{code}] refused\r\n");
        }
        let rest = command.split_once(' ').map_or("", |(_, rest)| rest);
        let read = arguments(rest);
        let reply = match &read {
            Some((names, after)) => self.folder(verb, names, after),
            None => Err(Failure::Bad),
        };
        let named = || {
            let taken = read.iter().find_map(|(names, _)| names.last());
            taken.map_or_else(String::new, |name| utf7::decode(name))
        };
        match reply {
            Ok(lines) => format!("{lines}{tag} OK done\r\n"),
            Err(Failure::Bad) => format!("{tag} BAD not read\r\n"),
            Err(Failure::No(None)) => format!("{tag} NO refused\r\n"),
            Err(Failure::No(Some(NONEXISTENT))) if self.behavior.utf8_no => {
                format!("{tag} NO [{NONEXISTENT}] Mailbox doesn't exist: Caf\u{e9}\r\n")
            }
            Err(Failure::No(Some(ALREADY_EXISTS))) if self.has(Extension::Gmail) => {
                let taken = named();
                format!("{tag} NO [{ALREADY_EXISTS}] Duplicate folder name {taken} (Failure)\r\n")
            }
            Err(Failure::Variant) if verb == "RENAME" => {
                let taken = named();
                format!(
                    "{tag} NO [{ALREADY_EXISTS}] New folder name {taken} conflicts with existing folder name. (Failure)\r\n"
                )
            }
            Err(Failure::Variant) => format!(
                "{tag} NO [{ALREADY_EXISTS}] Folder name conflicts with existing folder name. (Failure)\r\n"
            ),
            Err(Failure::No(Some(code))) => format!("{tag} NO [{code}] refused\r\n"),
            Err(Failure::Said(words)) => format!("{tag} NO {words} (Failure)\r\n"),
        }
    }

    fn folder(&self, verb: &str, names: &[String], after: &str) -> Result<String, Failure> {
        match (verb, names) {
            // A role asked at creation the script does not honor (RFC 6154 section 3).
            ("CREATE", [_]) if !after.is_empty() => Err(Failure::No(Some(USE_ATTRIBUTE))),
            _ if !after.is_empty() => Err(Failure::Bad),
            ("CREATE", [name]) => self.create(name),
            ("RENAME", [from, to]) => self.rename(from, to),
            ("DELETE", [name]) => self.delete(name),
            ("SUBSCRIBE", [name]) => self.subscription(name, true),
            ("UNSUBSCRIBE", [name]) => self.subscription(name, false),
            ("MYRIGHTS", [name]) => self
                .lock()
                .iter()
                .find(|folder| folder.name == *name)
                .and_then(rights_line)
                .ok_or(Failure::No(Some(NONEXISTENT))),
            _ => Err(Failure::Bad),
        }
    }

    /// A new folder, unsubscribed, under a UIDVALIDITY no folder had; on
    /// Gmail a label folder.
    fn create(&self, name: &str) -> Result<String, Failure> {
        let gmail = self.has(Extension::Gmail);
        if gmail {
            gmail_rules(name)?;
        }
        let mut folders = self.lock();
        if name.eq_ignore_ascii_case("INBOX") || folders.iter().any(|folder| folder.name == name) {
            return Err(Failure::No(Some(ALREADY_EXISTS)));
        }
        if gmail && holds_variant(&folders, name, name) {
            return Err(Failure::Variant);
        }
        if gmail && name.starts_with(GMAIL_SYSTEM) {
            return Err(Failure::No(None));
        }
        let uid_validity = folders.iter().map(|folder| folder.uid_validity).max();
        let folder = if gmail {
            Folder::labeled(name, name)
        } else {
            Folder::new(name)
        };
        folders.push(Folder {
            subscribed: false,
            uid_validity: uid_validity.unwrap_or_default() + 1,
            ..folder
        });
        Ok(String::new())
    }

    /// The folder and every folder under it under the new name (RFC
    /// 3501 section 6.3.5); on Gmail the labels and the messages that
    /// carry them follow. As Dovecot does, the subscriptions stay with
    /// the old names, so the new ones start unsubscribed (RFC 9051
    /// section 6.3.6). The script keeps INBOX where it is.
    fn rename(&self, from: &str, to: &str) -> Result<String, Failure> {
        if self.has(Extension::Gmail) {
            gmail_rules(to)?;
        }
        let mut folders = self.lock();
        self.editable(&folders, from)?;
        if to.eq_ignore_ascii_case("INBOX") || folders.iter().any(|folder| folder.name == to) {
            return Err(Failure::No(Some(ALREADY_EXISTS)));
        }
        if self.has(Extension::Gmail) && holds_variant(&folders, to, from) {
            return Err(Failure::Variant);
        }
        let under = format!("{from}{}", self.delimiter);
        let mut relabeled = Vec::new();
        for folder in folders.iter_mut() {
            let renamed = if folder.name == from {
                to.to_owned()
            } else if let Some(rest) = folder.name.strip_prefix(&under) {
                format!("{to}{}{rest}", self.delimiter)
            } else {
                continue;
            };
            if let Some(label) = folder.label.as_mut() {
                relabeled.push((std::mem::replace(label, renamed.clone()), renamed.clone()));
            }
            folder.name = renamed;
            folder.subscribed = false;
        }
        for message in folders.iter_mut().flat_map(|folder| folder.mail.iter_mut()) {
            for label in &mut message.labels {
                if let Some((_, new)) = relabeled.iter().find(|(old, _)| old == label) {
                    label.clone_from(new);
                }
            }
        }
        if self.behavior.renaming == Renaming::StopsPartway {
            return Err(Failure::No(None));
        }
        Ok(String::new())
    }

    /// The folder, unless folders stand under it: then NO with
    /// `HASCHILDREN` (RFC 9051 section 6.3.5) or, where the behavior
    /// says so, its mail gone and its name kept as `\Noselect` (RFC 3501
    /// section 6.3.4). On Gmail its label leaves every message.
    fn delete(&self, name: &str) -> Result<String, Failure> {
        let mut folders = self.lock();
        let index = self.editable(&folders, name)?;
        let under = format!("{name}{}", self.delimiter);
        if folders.iter().any(|folder| folder.name.starts_with(&under)) {
            let keeps = self.behavior.parent_delete == ParentDelete::KeepsName;
            if !folders[index].is_selectable() || !keeps {
                return Err(Failure::No(Some(HAS_CHILDREN)));
            }
            folders[index] = Folder::noselect(name);
            return Ok(String::new());
        }
        let gone = folders.remove(index);
        if let Some(label) = gone.label {
            for message in folders.iter_mut().flat_map(|folder| folder.mail.iter_mut()) {
                message.labels.retain(|held| *held != label);
            }
        }
        Ok(String::new())
    }

    /// SUBSCRIBE of a missing name answers NO, as does one of a folder
    /// that exists where the behavior says so; UNSUBSCRIBE of a missing
    /// name is done already (RFC 9051 section 6.3.8).
    fn subscription(&self, name: &str, subscribed: bool) -> Result<String, Failure> {
        let mut folders = self.lock();
        match folders.iter_mut().find(|folder| folder.name == name) {
            Some(_) if subscribed && self.behavior.subscribe_refusal.is_some() => {
                return Err(Failure::No(self.behavior.subscribe_refusal));
            }
            Some(folder) => folder.subscribed = subscribed,
            None if subscribed => return Err(Failure::No(Some(NONEXISTENT))),
            None => {}
        }
        Ok(String::new())
    }

    /// Where a folder a RENAME or DELETE may touch stands: never INBOX
    /// and, on Gmail, never a folder Gmail keeps for itself.
    fn editable(&self, folders: &[Folder], name: &str) -> Result<usize, Failure> {
        if name.eq_ignore_ascii_case("INBOX") {
            return Err(Failure::No(None));
        }
        let index = folders.iter().position(|folder| folder.name == name);
        let index = index.ok_or(Failure::No(Some(NONEXISTENT)))?;
        let folder = &folders[index];
        if self.has(Extension::Gmail)
            && (folder.name.starts_with(GMAIL_SYSTEM) || folder.special_use.is_some())
        {
            return Err(Failure::No(None));
        }
        Ok(index)
    }
}

/// What Gmail refuses of a name whatever stands beside it, as far as the
/// tests need it.
fn gmail_rules(wire: &str) -> Result<(), Failure> {
    let path = utf7::decode(wire);
    let spaces: Vec<bool> = path.chars().map(char::is_whitespace).collect();
    if spaces.windows(2).any(|pair| pair[0] && pair[1]) {
        return Err(Failure::Said(EXCESS_WHITE_SPACE));
    }
    if path.encode_utf16().count() > MAX_PATH_UNITS {
        return Err(Failure::Said(NOT_ALLOWED));
    }
    if path.len() > MAX_PATH_BYTES {
        return Err(Failure::Said(SYSTEM_ERROR));
    }
    Ok(())
}

/// How Gmail tells label names apart, as far as the tests need it: every
/// letter in one case, full-width ASCII as ASCII and the marks it reads as a
/// space as one, its capital sigma and capital sharp s left alone.
fn gmail_fold(wire: &str) -> String {
    let mut fold = String::new();
    for letter in utf7::decode(wire).chars() {
        if matches!(letter, CAPITAL_SIGMA | CAPITAL_SHARP_S) {
            fold.push(letter);
            continue;
        }
        let code = u32::from(letter);
        let narrow = FULL_WIDTH
            .contains(&code)
            .then(|| char::from_u32(code - FULL_WIDTH_OFFSET))
            .flatten()
            .unwrap_or(letter);
        if narrow.is_whitespace() || READ_AS_SPACE.contains(narrow) {
            fold.push(' ');
        } else {
            fold.extend(narrow.to_lowercase());
        }
    }
    fold
}

/// Whether a folder other than `except` reads as the name under Gmail's
/// fold.
fn holds_variant(folders: &[Folder], name: &str, except: &str) -> bool {
    let fold = gmail_fold(name);
    folders
        .iter()
        .any(|folder| folder.name != except && gmail_fold(&folder.name) == fold)
}

/// The MYRIGHTS line of one folder (RFC 4314 section 3.8); none for a
/// folder the server cannot look up.
pub(super) fn rights_line(folder: &Folder) -> Option<String> {
    let rights = match folder.rights {
        MyRights::Owner => OWNER_RIGHTS,
        MyRights::Letters(letters) => letters,
        MyRights::Refused => return None,
    };
    Some(format!("* MYRIGHTS {} {rights}\r\n", quote(&folder.name)))
}
