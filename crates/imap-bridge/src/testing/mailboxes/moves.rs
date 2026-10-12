// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! UID MOVE, UID COPY and UID EXPUNGE of the scripted server. A move or
//! a copy gives the messages new UIDs in the target and names them in
//! `COPYUID` where UIDPLUS is on (RFC 4315 section 3); a move sends the
//! code on an untagged OK ahead of its expunges (RFC 6851 section 4.3).
//! An expunge removes the messages of its set that carry `\Deleted`. On
//! a Gmail account a message that enters Spam or Trash loses the Inbox
//! label.

use std::fmt::Write as _;

use super::gmail::{SPAM, TRASH};
use super::{Extension, Mailboxes, Refusal, unquote};
use crate::testing::behavior::Storing;
use crate::testing::fetch::in_set;
use crate::testing::folder::Folder;
use crate::testing::messages::Message;

/// What one of the three commands answers ahead of its tagged OK and
/// the code that OK carries.
type Answered = (String, String);

impl Mailboxes {
    /// The answer to UID MOVE, UID COPY or UID EXPUNGE on the mailbox the
    /// connection has open, its tagged line included; `None` for any
    /// other command. A move and an expunge need the mailbox open for
    /// writing; MOVE and UID EXPUNGE need their extension.
    pub(in crate::testing) fn transfer(
        &self,
        (open, writable): (Option<&str>, bool),
        command: &str,
        tag: &str,
    ) -> Option<String> {
        let written = open.filter(|_| writable);
        if is_transfer(command) {
            match self.behavior.moving {
                Storing::Refuses => return Some(format!("{tag} NO refused\r\n")),
                Storing::Bad => return Some(format!("{tag} BAD refused\r\n")),
                _ => {}
            }
        }
        let reply = if let Some(rest) = command.strip_prefix("UID MOVE ") {
            self.moved(written, rest, true)
        } else if let Some(rest) = command.strip_prefix("UID COPY ") {
            self.moved(open, rest, false)
        } else {
            self.expunged(written, command.strip_prefix("UID EXPUNGE ")?)
        };
        Some(match reply {
            Ok((lines, code)) => format!("{lines}{tag} OK {code}done\r\n"),
            Err(Refusal::Bad) => format!("{tag} BAD not offered\r\n"),
            Err(Refusal::No) => format!("{tag} NO refused\r\n"),
        })
    }

    /// `<set> "<name>"`: the messages of the set to the folder of that
    /// name under its next UIDs, taken from the source when `removes`.
    fn moved(&self, from: Option<&str>, rest: &str, removes: bool) -> Result<Answered, Refusal> {
        if removes && !self.has(Extension::Move) {
            return Err(Refusal::Bad);
        }
        let (set, target) = rest.split_once(' ').ok_or(Refusal::Bad)?;
        let (target, _) = unquote(target).ok_or(Refusal::Bad)?;
        let mut folders = self.lock();
        let source = folders
            .iter()
            .position(|folder| Some(folder.name.as_str()) == from)
            .ok_or(Refusal::No)?;
        let into = folders
            .iter()
            .position(|folder| folder.name == target && folder.is_selectable())
            .filter(|into| *into != source && !folders[*into].read_only)
            .ok_or(Refusal::No)?;
        let top = folders[source].top();
        let taken: Vec<(usize, Message)> = folders[source]
            .mail
            .iter()
            .enumerate()
            .filter(|(_, message)| in_set(set, message.uid, top))
            .map(|(index, message)| (index + 1, message.clone()))
            .collect();
        let gmail_store = self.has(Extension::Gmail) && (target == SPAM || target == TRASH);
        let pairs: Vec<(u32, u32)> = taken
            .iter()
            .map(|(_, message)| {
                (
                    message.uid,
                    arrive(&mut folders[into], message, gmail_store),
                )
            })
            .collect();
        let code = self.copyuid(&pairs, folders[into].uid_validity);
        if !removes {
            let tagged = code.map(|code| format!("[{code}] "));
            return Ok((String::new(), tagged.unwrap_or_default()));
        }
        let mut lines = code
            .map(|code| format!("* OK [{code}] Moved\r\n"))
            .unwrap_or_default();
        let numbers: Vec<usize> = taken.iter().map(|(number, _)| *number).collect();
        folders[source]
            .mail
            .retain(|message| !in_set(set, message.uid, top));
        folders[source].recount();
        expunge_lines(&mut lines, &numbers);
        Ok((lines, String::new()))
    }

    /// `<set>`: the messages of the set that carry `\Deleted` leave, an
    /// EXPUNGE line for each (RFC 4315 section 2.1).
    fn expunged(&self, from: Option<&str>, set: &str) -> Result<Answered, Refusal> {
        if !self.has(Extension::Uidplus) {
            return Err(Refusal::Bad);
        }
        let from = from.ok_or(Refusal::No)?;
        let mut lines = String::new();
        self.edit(from, |folder| {
            let top = folder.top();
            let leaves = |message: &Message| {
                in_set(set, message.uid, top)
                    && message
                        .flags
                        .iter()
                        .any(|flag| flag.eq_ignore_ascii_case("\\Deleted"))
            };
            let numbers: Vec<usize> = folder
                .mail
                .iter()
                .enumerate()
                .filter(|(_, message)| leaves(message))
                .map(|(index, _)| index + 1)
                .collect();
            folder.mail.retain(|message| !leaves(message));
            expunge_lines(&mut lines, &numbers);
        });
        Ok((lines, String::new()))
    }

    /// What `COPYUID` names for these pairs, unless the behavior names a
    /// code of its own. No code without UIDPLUS or when nothing went.
    fn copyuid(&self, pairs: &[(u32, u32)], uid_validity: u32) -> Option<String> {
        if !self.has(Extension::Uidplus) || pairs.is_empty() {
            return None;
        }
        if let Some(code) = self.behavior.copyuid {
            return Some(code.to_owned());
        }
        let source: Vec<u32> = pairs.iter().map(|(from, _)| *from).collect();
        let target: Vec<u32> = pairs.iter().map(|(_, to)| *to).collect();
        let (source, target) = (folded(&source), folded(&target));
        Some(format!("COPYUID {uid_validity} {source} {target}"))
    }
}

/// Whether the command is a UID MOVE, a UID COPY or a UID EXPUNGE.
pub(in crate::testing) fn is_transfer(command: &str) -> bool {
    ["UID MOVE ", "UID COPY ", "UID EXPUNGE "]
        .iter()
        .any(|verb| command.starts_with(verb))
}

/// The message as it lands in the folder under the folder's next UID;
/// a Gmail message entering Spam or Trash drops the Inbox label.
fn arrive(folder: &mut Folder, message: &Message, gmail_store: bool) -> u32 {
    let uid = folder.uid_next;
    let mut labels = message.labels.clone();
    if gmail_store {
        labels.retain(|label| !label.eq_ignore_ascii_case("\\Inbox"));
    }
    folder.mail.push(Message {
        uid,
        modseq: folder.highest_modseq + 1,
        labels,
        ..message.clone()
    });
    folder.recount();
    uid
}

/// An EXPUNGE line per sequence number, the highest first, so each
/// number still holds when its line is read.
fn expunge_lines(lines: &mut String, numbers: &[usize]) {
    for number in numbers.iter().rev() {
        let _ = write!(lines, "* {number} EXPUNGE\r\n");
    }
}

/// The UIDs in the order given, runs of neighbors as ranges.
fn folded(uids: &[u32]) -> String {
    let mut runs: Vec<(u32, u32)> = Vec::new();
    for uid in uids {
        match runs.last_mut() {
            Some((_, last)) if last.checked_add(1) == Some(*uid) => *last = *uid,
            _ => runs.push((*uid, *uid)),
        }
    }
    let written: Vec<String> = runs
        .iter()
        .map(|(first, last)| {
            if first == last {
                first.to_string()
            } else {
                format!("{first}:{last}")
            }
        })
        .collect();
    written.join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_of_neighbors_fold_in_the_order_given() {
        assert_eq!(folded(&[4, 5, 6, 9, 2]), "4:6,9,2");
        assert_eq!(folded(&[7]), "7");
    }
}
