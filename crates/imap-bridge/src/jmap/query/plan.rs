// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Which folders a search asks of the server, in what order and with
//! which coarse keys. A folder account searches the folders of the
//! scope; a Gmail account searches the stores its rows of the scope can
//! lie in, All Mail under the label of a mailbox scope. Junk and Trash
//! run last. The SQL over the hits makes the scope and the dates exact,
//! so the folders only have to hold every row the result can take.

#[cfg(test)]
mod tests;

use time::{Date, OffsetDateTime};

use super::Filter;
use crate::gmail::{self, ALL_MAIL_ROLE};
use crate::runtime::search::{Folder, Plan};
use crate::session::writable_day;
use crate::store::{MailboxRow, MailboxSnapshot};

/// The roles whose folders run last.
const LAST_ROLES: [&str; 2] = ["junk", "trash"];

/// The plan of a filter over the account's mailboxes as they stand.
pub(super) fn plan(filter: &Filter, snapshot: &MailboxSnapshot) -> Plan {
    let folders = if snapshot.gmail {
        gmail_account(filter, &snapshot.rows)
    } else {
        folder_account(filter, &snapshot.rows)
    };
    Plan {
        text: filter.text.clone(),
        from: filter.from.clone(),
        unseen: filter.unseen(),
        flagged: filter.flagged(),
        since: filter.after.and_then(since),
        before: filter.before.and_then(before),
        gmail: snapshot.gmail,
        folders: last_roles_last(folders),
    }
}

/// The folders of the scope: the mailbox it names, else every folder
/// outside the ones it leaves out, since a row lies in its folder alone.
fn folder_account<'a>(filter: &Filter, rows: &'a [MailboxRow]) -> Vec<(&'a MailboxRow, Folder)> {
    rows.iter()
        .filter(|row| match (&filter.in_mailbox, &filter.other_than) {
            (Some(mailbox), _) => row.id.as_str() == mailbox,
            (None, Some(left_out)) => !left_out.iter().any(|id| id == row.id.as_str()),
            (None, None) => true,
        })
        .filter_map(|row| Some((row, searched(row, None)?)))
        .collect()
}

/// The stores the scope's rows can lie in. A store named is searched
/// alone; a label mailbox is All Mail under its label as a STORE writes
/// it. Without a mailbox All Mail always runs, since its rows may lie in
/// a label the scope keeps; Spam and Trash run where the scope keeps
/// them, a row there lying in its store alone.
fn gmail_account<'a>(filter: &Filter, rows: &'a [MailboxRow]) -> Vec<(&'a MailboxRow, Folder)> {
    let all_mail = rows
        .iter()
        .find(|row| row.facts.store && row.facts.role.as_deref() == Some(ALL_MAIL_ROLE));
    if let Some(mailbox) = &filter.in_mailbox {
        let Some(row) = rows.iter().find(|row| row.id.as_str() == mailbox) else {
            return Vec::new();
        };
        let (store, label) = match &row.facts.gmail_label {
            _ if row.facts.store => (Some(row), None),
            Some(label) => (all_mail, Some(gmail::written(label).to_owned())),
            None => (None, None),
        };
        return store
            .and_then(|store| Some((store, searched(store, label)?)))
            .into_iter()
            .collect();
    }
    let kept = |row: &MailboxRow| {
        let left_out = filter.other_than.as_deref().unwrap_or_default();
        !left_out.iter().any(|id| id == row.id.as_str())
    };
    rows.iter()
        .filter(|row| row.facts.store)
        .filter(|row| row.facts.role.as_deref() == Some(ALL_MAIL_ROLE) || kept(row))
        .filter_map(|row| Some((row, searched(row, None)?)))
        .collect()
}

/// The folder as the session searches it; `None` for one it cannot
/// open or whose rows stand under no UIDVALIDITY.
fn searched(row: &MailboxRow, label: Option<String>) -> Option<Folder> {
    let facts = &row.facts;
    let uid_validity = facts
        .uid_validity
        .filter(|_| facts.selectable && facts.store)?;
    Some(Folder {
        id: row.id.clone(),
        imap_name: facts.imap_name.clone(),
        uid_validity,
        label,
    })
}

/// The folders in the tree's order with Junk and Trash behind the rest.
fn last_roles_last(mut folders: Vec<(&MailboxRow, Folder)>) -> Vec<Folder> {
    folders.sort_by_key(|(row, _)| {
        let role = row.facts.role.as_deref();
        role.is_some_and(|role| LAST_ROLES.contains(&role))
    });
    folders.into_iter().map(|(_, folder)| folder).collect()
}

/// The SINCE day for `after`: the day before its day in UTC. A server
/// compares the day of the internal date as written, leaving out its
/// time and zone (RFC 3501 section 6.4.4); that day lies a day from the
/// day in UTC at most. A day no IMAP date can write is left out, since
/// the SQL keeps the exact test.
fn since(after: i64) -> Option<Date> {
    let day = OffsetDateTime::from_unix_timestamp(after)
        .ok()?
        .date()
        .previous_day()?;
    writable_day(day).then_some(day)
}

/// The BEFORE day for `before`: two days past the day of the last
/// second that matches, one for the zone and one since BEFORE leaves its
/// own day out.
fn before(before: i64) -> Option<Date> {
    let last = OffsetDateTime::from_unix_timestamp(before.checked_sub(1)?).ok()?;
    let day = last.date().next_day()?.next_day()?;
    writable_day(day).then_some(day)
}
