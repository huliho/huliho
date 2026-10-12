// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What each update and destroy of an `Email/set` asks of which folder,
//! read against the rows before anything is sent. A folder account holds
//! a message in one folder, so a change of its mailboxes is a move from
//! one to another (RFC 8621 sections 1.3.1 and 4.6). On a Gmail account
//! a label is added or removed on the store the row lies in, entering
//! Spam or Trash is a move between the stores, after which the row
//! carries that store alone, and leaving either is a move into All Mail
//! with the labels of the patch stored there. A destroy on Gmail stays
//! in Spam and Trash, since the account's own setting decides what an
//! expunge does anywhere else.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use super::super::set::{MailboxEdit, Patch, SetAsk, SetError};
use super::keywords;
use super::moves::{self, Relabel};
use super::{Expected, Member, Tally, Work};
use crate::gmail::ALL_MAIL_ROLE;
use crate::mailboxes::SyncError;
use crate::session::Moving;
use crate::store::{KeywordRow, MailboxId, MailboxRow, SetWrite};
use crate::sync::{Cache, blocking};

/// The roles of the two Gmail stores a destroy may reach.
const DESTROYABLE_ROLES: [&str; 2] = ["junk", "trash"];

/// What an update does to the mailboxes of its email.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Plan {
    Nothing,
    /// Gmail labels on the store the row lies in.
    Labels(Relabel),
    /// A move to another folder and, into All Mail, the labels stored
    /// there afterwards.
    Move {
        to: MailboxId,
        then: Option<Relabel>,
    },
}

impl Plan {
    /// The labels the plan stores, if any.
    fn relabel(&self) -> Option<&Relabel> {
        match self {
            Self::Labels(relabel)
            | Self::Move {
                then: Some(relabel),
                ..
            } => Some(relabel),
            Self::Nothing | Self::Move { then: None, .. } => None,
        }
    }
}

/// The rows of the ask, the work per folder and how each update should
/// leave its email; `None` when the state is not the one the ask names.
/// An id the account does not hold is not found and a row no UID names
/// cannot be written now.
pub(super) async fn planned(
    cache: &Cache,
    ask: &SetAsk,
) -> Result<Option<(Work, Vec<Option<Expected>>)>, SyncError> {
    let (store, key) = (Arc::clone(&cache.store), cache.key.clone());
    let ids: Vec<String> = ask
        .updates
        .iter()
        .map(|(id, _)| id.clone())
        .chain(ask.destroy.iter().cloned())
        .collect();
    let (state, found, rows) = blocking(move || {
        let names: Vec<&str> = ids.iter().map(String::as_str).collect();
        let (state, found) = store.keyword_rows(&key, &names)?;
        let rows: HashMap<MailboxId, MailboxRow> = store
            .mailbox_snapshot(&key)?
            .rows
            .into_iter()
            .map(|row| (row.id.clone(), row))
            .collect();
        Ok((state, found, rows))
    })
    .await?;
    if ask
        .if_in_state
        .as_ref()
        .is_some_and(|named| *named != state.to_string())
    {
        return Ok(None);
    }
    let labeled = rows.values().any(|row| row.facts.gmail_label.is_some());
    let row_of = |id: &str| found.iter().find(|row| row.id.as_str() == id);
    let mut work = Work {
        folders: std::collections::BTreeMap::new(),
        rows,
        tallies: vec![Tally::default(); ask.updates.len() + ask.destroy.len()],
        stored: SetWrite {
            gmail: labeled,
            ..SetWrite::default()
        },
        restores: Vec::new(),
        moving: Moving::default(),
        state,
    };
    let mut expected = Vec::with_capacity(ask.updates.len());
    for (index, (id, patch)) in ask.updates.iter().enumerate() {
        let row = row_of(id);
        expected.push(row.map(|row| expect(row, patch)));
        if let Err(error) = update(&mut work, (index, row), patch, labeled) {
            work.tallies[index].failed = Some(error);
        }
    }
    for (offset, id) in ask.destroy.iter().enumerate() {
        let index = ask.updates.len() + offset;
        if let Err(error) = destroy(&mut work, index, row_of(id), labeled) {
            work.tallies[index].failed = Some(error);
        }
    }
    Ok(Some((work, expected)))
}

/// One update into the work of its folder: the keywords it changes and
/// what it does to the mailboxes, each a part it waits on.
fn update(
    work: &mut Work,
    (index, row): (usize, Option<&KeywordRow>),
    patch: &Patch,
    labeled: bool,
) -> Result<(), SetError> {
    let row = row.ok_or(SetError::NotFound)?;
    let uid = row.uid.ok_or(SetError::ServerUnavailable)?;
    let remove = keywords::removed(&patch.keywords, row);
    let keyword_part = !(patch.keywords.set.is_empty() && remove.is_empty());
    let plan = match &patch.mailboxes {
        Some(edit) => mailboxes(row, edit, &work.rows, labeled)?,
        None => Plan::Nothing,
    };
    if !keyword_part && plan == Plan::Nothing {
        return Ok(());
    }
    let member = Member {
        index,
        id: row.id.clone(),
        uid,
    };
    let folder = work.folders.entry(row.folder.clone()).or_default();
    let mut parts = 0;
    if keyword_part {
        keywords::join(
            &mut folder.keywords,
            &patch.keywords,
            remove,
            member.clone(),
        );
        parts += 1;
    }
    match plan {
        Plan::Nothing => {}
        Plan::Labels(relabel) => {
            moves::join_labels(&mut folder.labels, relabel, member);
            parts += 1;
        }
        Plan::Move { to, then } => {
            moves::join_move(&mut folder.moves, (to, then), member);
            parts += 1;
        }
    }
    work.tallies[index].left = parts;
    Ok(())
}

/// One destroy into the work of its folder; on a Gmail account the row
/// lies in Spam or Trash.
fn destroy(
    work: &mut Work,
    index: usize,
    row: Option<&KeywordRow>,
    labeled: bool,
) -> Result<(), SetError> {
    let row = row.ok_or(SetError::NotFound)?;
    let uid = row.uid.ok_or(SetError::ServerUnavailable)?;
    if labeled {
        let role = work
            .rows
            .get(&row.folder)
            .and_then(|folder| folder.facts.role.as_deref());
        if !role.is_some_and(|role| DESTROYABLE_ROLES.contains(&role)) {
            return Err(SetError::Forbidden);
        }
    }
    let member = Member {
        index,
        id: row.id.clone(),
        uid,
    };
    work.folders
        .entry(row.folder.clone())
        .or_default()
        .destroys
        .push(member);
    work.tallies[index].left = 1;
    Ok(())
}

/// What the mailbox edit does to the row. Every mailbox it names is one
/// the account holds that can hold mail (RFC 8620 section 5.3, a
/// reference to a record that does not exist); entering a mailbox the
/// email is in and leaving one it is not in change nothing. On a folder
/// account an email left in no mailbox is invalid under the paths that
/// took it out and one left in two has too many mailboxes. On a Gmail
/// account a label no STORE run can carry fails the update with nothing
/// sent.
fn mailboxes(
    row: &KeywordRow,
    edit: &MailboxEdit,
    rows: &HashMap<MailboxId, MailboxRow>,
    labeled: bool,
) -> Result<Plan, SetError> {
    let known = |id: &String| {
        rows.get(&MailboxId::from(id.clone()))
            .is_some_and(|row| row.facts.selectable)
    };
    let unknown: Vec<&String> = edit
        .add
        .iter()
        .chain(&edit.remove)
        .filter(|id| !known(id))
        .collect();
    if !unknown.is_empty() {
        return Err(invalid(edit, unknown.into_iter().map(String::as_str)));
    }
    let current: BTreeSet<MailboxId> = row.mailboxes.iter().cloned().collect();
    let named = |ids: &[String]| -> BTreeSet<MailboxId> {
        ids.iter().map(|id| MailboxId::from(id.clone())).collect()
    };
    let add = named(&edit.add);
    let remove = if edit.whole {
        current.difference(&add).cloned().collect()
    } else {
        named(&edit.remove)
    };
    let entering: BTreeSet<MailboxId> = add.difference(&current).cloned().collect();
    let leaving: BTreeSet<MailboxId> = remove.intersection(&current).cloned().collect();
    if labeled {
        let plan = labels(row, (edit, rows), (&entering, &leaving))?;
        return match plan.relabel() {
            Some(relabel) if !moves::sendable(rows, relabel) => Err(SetError::ServerFail),
            _ => Ok(plan),
        };
    }
    let mut result = current.clone();
    result.retain(|mailbox| !leaving.contains(mailbox));
    result.extend(entering.iter().cloned());
    match (result.len(), result.iter().next()) {
        (0, _) => Err(invalid(edit, leaving.iter().map(MailboxId::as_str))),
        (1, Some(to)) if result != current => Ok(Plan::Move {
            to: to.clone(),
            then: None,
        }),
        (1, _) => Ok(Plan::Nothing),
        _ => Err(SetError::TooManyMailboxes),
    }
}

/// What the edit does to a row of a Gmail account, whose store is All
/// Mail, Spam or Trash.
fn labels(
    row: &KeywordRow,
    (edit, rows): (&MailboxEdit, &HashMap<MailboxId, MailboxRow>),
    (entering, leaving): (&BTreeSet<MailboxId>, &BTreeSet<MailboxId>),
) -> Result<Plan, SetError> {
    let store = |id: &&MailboxId| rows.get(*id).is_some_and(|row| row.facts.store);
    let all_mail = |id: &MailboxId| {
        rows.get(id).and_then(|row| row.facts.role.as_deref()) == Some(ALL_MAIL_ROLE)
    };
    let stores: Vec<&MailboxId> = entering.iter().filter(store).collect();
    let relabel = Relabel {
        add: entering.iter().filter(|id| !store(id)).cloned().collect(),
        remove: leaving.iter().filter(|id| !store(id)).cloned().collect(),
    };
    let added = || invalid(edit, relabel.add.iter().map(MailboxId::as_str));
    let here = &row.folder;
    match stores.as_slice() {
        [_, _, ..] => Err(invalid(edit, stores.iter().map(|id| id.as_str()))),
        [target] if all_mail(target) => Ok(Plan::Move {
            to: (*target).clone(),
            then: Some(relabel),
        }),
        // A row of Spam or Trash carries its store alone.
        [_] if !relabel.add.is_empty() => Err(added()),
        [target] => Ok(Plan::Move {
            to: (*target).clone(),
            then: None,
        }),
        // A message leaves All Mail for Spam or Trash alone.
        [] if all_mail(here) && leaving.contains(here) => Err(invalid(edit, [here.as_str()])),
        [] if all_mail(here) && relabel == Relabel::default() => Ok(Plan::Nothing),
        [] if all_mail(here) => Ok(Plan::Labels(relabel)),
        [] if leaving.contains(here) && relabel.add.is_empty() => {
            Err(invalid(edit, leaving.iter().map(MailboxId::as_str)))
        }
        [] if leaving.contains(here) => {
            let target = rows
                .values()
                .find(|row| row.facts.store && all_mail(&row.id));
            let target = target.ok_or(SetError::ServerUnavailable)?;
            Ok(Plan::Move {
                to: target.id.clone(),
                then: Some(relabel),
            })
        }
        [] if relabel.add.is_empty() => Ok(Plan::Nothing),
        [] => Err(added()),
    }
}

/// The paths of the patch that named these mailboxes, each once.
fn invalid<'a>(edit: &MailboxEdit, mailboxes: impl IntoIterator<Item = &'a str>) -> SetError {
    let paths: BTreeSet<String> = mailboxes.into_iter().map(|id| edit.path(id)).collect();
    SetError::InvalidProperties(paths.into_iter().collect())
}

/// How the update should leave the email when the server does what its
/// patch asks and nothing more.
fn expect(row: &KeywordRow, patch: &Patch) -> Expected {
    let mut mailboxes: BTreeSet<MailboxId> = row.mailboxes.iter().cloned().collect();
    if let Some(edit) = &patch.mailboxes {
        if edit.whole {
            mailboxes.clear();
        }
        for id in &edit.remove {
            mailboxes.remove(&MailboxId::from(id.clone()));
        }
        mailboxes.extend(edit.add.iter().map(|id| MailboxId::from(id.clone())));
    }
    let edit = &patch.keywords;
    let mut keywords = if edit.whole {
        std::collections::BTreeMap::new()
    } else {
        row.keywords.clone()
    };
    for keyword in &edit.clear {
        keywords.remove(keyword);
    }
    keywords.extend(edit.set.iter().map(|keyword| (keyword.clone(), true)));
    Expected {
        mailboxes,
        keywords,
    }
}

#[cfg(test)]
mod tests;
