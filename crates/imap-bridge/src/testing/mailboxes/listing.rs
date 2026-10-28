// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! LIST, LSUB and STATUS of the scripted server over the folders as they
//! stand, every option checked against the extensions the script
//! advertises.

use super::folders::rights_line;
use super::{Extension, Mailboxes, Refusal, quote, unquote};
use crate::testing::folder::Folder;

impl Mailboxes {
    /// The answer to LIST, LSUB or STATUS: the untagged lines, then the
    /// tagged one. A folder command goes to the folder model; any other
    /// verb is BAD.
    pub(in crate::testing) fn answer(&self, verb: &str, command: &str, tag: &str) -> String {
        let reply = match verb {
            "LIST" => self.list(command),
            "LSUB" => Ok(self.lsub()),
            "STATUS" => self.status(command),
            "CREATE" | "RENAME" | "DELETE" | "SUBSCRIBE" | "UNSUBSCRIBE" | "MYRIGHTS" => {
                return self.folder_command(verb, command, tag);
            }
            _ => Err(Refusal::Bad),
        };
        match reply {
            Ok(lines) => format!("{lines}{tag} OK done\r\n"),
            Err(Refusal::Bad) => format!("{tag} BAD not offered\r\n"),
            Err(Refusal::No) => format!("{tag} NO refused\r\n"),
        }
    }

    /// `LIST "" "*"` with an optional RETURN list, every option checked
    /// against the extensions (RFC 5258, RFC 6154 section 2, RFC 5819,
    /// RFC 8440).
    fn list(&self, command: &str) -> Result<String, Refusal> {
        let options = match command.split_once(" RETURN (") {
            Some((_, rest)) if self.has(Extension::ListExtended) => rest.trim_end_matches(')'),
            Some(_) => return Err(Refusal::Bad),
            None => "",
        };
        if options.contains("SPECIAL-USE") && !self.has(Extension::SpecialUse) {
            return Err(Refusal::Bad);
        }
        let status_items = options.split_once("STATUS (").map(|(_, items)| items);
        if status_items.is_some() && !self.has(Extension::ListStatus) {
            return Err(Refusal::Bad);
        }
        let my_rights = options.contains("MYRIGHTS");
        if my_rights && !self.has(Extension::ListMyrights) {
            return Err(Refusal::Bad);
        }
        let subscribed = options.contains("SUBSCRIBED");
        let mut lines = String::new();
        for (index, folder) in self.folders().iter().enumerate() {
            lines.push_str(&self.list_line("LIST", folder, subscribed));
            // RFC 5819 section 2: a mailbox STATUS would refuse gets no line.
            if let Some(items) = status_items
                && folder.is_selectable()
                && !folder.refuses_status
            {
                lines.push_str(&self.status_line(folder, items)?);
            }
            // RFC 8440 section 3: after the LIST line, for a mailbox that exists.
            if my_rights && folder.is_selectable() {
                lines.push_str(&rights_line(folder));
            }
            if self.chatter && index == 0 {
                lines.push_str("* OK [ALERT] scripted\r\n");
            }
        }
        Ok(lines)
    }

    fn lsub(&self) -> String {
        self.folders()
            .iter()
            .filter(|folder| folder.subscribed)
            .map(|folder| self.list_line("LSUB", folder, false))
            .collect()
    }

    /// `STATUS <name> (<items>)` on one folder.
    fn status(&self, command: &str) -> Result<String, Refusal> {
        let rest = command.strip_prefix("STATUS ").ok_or(Refusal::Bad)?;
        let (name, rest) = unquote(rest).ok_or(Refusal::Bad)?;
        let items = rest
            .trim()
            .strip_prefix('(')
            .and_then(|items| items.strip_suffix(')'))
            .ok_or(Refusal::Bad)?;
        let folders = self.folders();
        let folder = folders
            .iter()
            .find(|folder| folder.name == name && folder.is_selectable() && !folder.refuses_status)
            .ok_or(Refusal::No)?;
        self.status_line(folder, items)
    }

    fn list_line(&self, verb: &str, folder: &Folder, subscribed: bool) -> String {
        let mut attributes = folder.attributes.clone();
        if subscribed && folder.subscribed {
            attributes.push("\\Subscribed");
        }
        if self.has(Extension::SpecialUse)
            && let Some(special) = folder.special_use
        {
            attributes.push(special);
        }
        format!(
            "* {verb} ({}) \"{}\" {}\r\n",
            attributes.join(" "),
            self.delimiter,
            quote(&folder.name)
        )
    }

    /// The STATUS line for the items asked, in the order asked;
    /// HIGHESTMODSEQ needs CONDSTORE (RFC 7162 section 3.1.7). A label
    /// folder counts the messages of All Mail under its label.
    fn status_line(&self, folder: &Folder, items: &str) -> Result<String, Refusal> {
        let (messages, unseen) = self
            .label_counts(folder)
            .unwrap_or((folder.messages, folder.unseen));
        let mut pairs = Vec::new();
        for item in items.split_whitespace() {
            let value = match item {
                "MESSAGES" => u64::from(messages),
                "UNSEEN" => u64::from(unseen),
                "UIDNEXT" => u64::from(folder.uid_next),
                "UIDVALIDITY" => u64::from(folder.uid_validity),
                "HIGHESTMODSEQ" if self.has(Extension::Condstore) => folder.highest_modseq,
                _ => return Err(Refusal::Bad),
            };
            pairs.push(format!("{item} {value}"));
        }
        Ok(format!(
            "* STATUS {} ({})\r\n",
            quote(&folder.name),
            pairs.join(" ")
        ))
    }
}
