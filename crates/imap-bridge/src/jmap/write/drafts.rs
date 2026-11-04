// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The creates of an `Email/set`, ahead of its updates and destroys (RFC
//! 8620 section 5.3). Every object is checked against the rows before a
//! command. A call that creates one draft and destroys one email of the
//! mailbox the draft goes to replaces it: one `UID REPLACE` where the
//! server offers it, else the APPEND first and the destroy of the old one
//! only once the new one landed, so a draft is never lost to a failed
//! save. On a Gmail account the old draft goes to Trash, since an expunge
//! outside Spam and Trash is the account's own setting.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::time::Instant;

use super::super::set::{Created, SetAsk, SetError};
use super::plan::may;
use super::{Member, Work, failure};
use crate::mailboxes::SyncError;
use crate::mailboxes::rights::Right;
use crate::message::Draft;
use crate::runtime::drafts::{self, Failure, Stopped, Unsealed};
use crate::runtime::{Connector, Wire};
use crate::session::{AppendAsk, Appended, Appending, Session};
use crate::store::{DraftRow, EmailId, GoneRow, MailboxId, StoreError};
use crate::sync::{Cache, blocking};

/// One create while the work runs.
struct Item {
    creation_id: String,
    draft: Draft,
    /// Where it goes, once the rows admit it.
    target: Option<Target>,
    /// How it ended where it did not reach the server.
    failed: Option<SetError>,
    /// What the server took.
    placed: Option<Placed>,
}

/// The mailbox of a create.
struct Target {
    mailbox: MailboxId,
    wire_name: String,
}

/// A message the server took.
struct Placed {
    /// What the row holds; taken once the bridge writes it.
    unsealed: Option<Unsealed>,
    /// What `APPENDUID` named where a row can stand under it.
    appended: Option<Appended>,
    /// The id its row takes where the bridge writes it.
    id: EmailId,
    /// The keyed hash of its Message-ID, which a pass lands its row by.
    hash: Vec<u8>,
}

/// The rows the bridge writes at once, each with where it goes.
pub(super) type Unwritten = Vec<(Unsealed, MailboxId, Appended)>;

/// The creates of one ask.
pub(super) struct Drafts {
    items: Vec<Item>,
    /// Where the destroy of a replace stands among the tallies.
    pair: Option<usize>,
}

impl Drafts {
    /// Whether a create still waits for the server.
    pub(super) fn wait(&self) -> bool {
        self.items.iter().any(|item| item.target.is_some())
    }

    /// What each create came to, in its order. One no session reached
    /// could not be asked.
    pub(super) fn outcomes(
        &self,
        landed: &HashMap<String, Created>,
    ) -> Vec<(String, Result<Created, SetError>)> {
        let outcome = |item: &Item| {
            if let Some(error) = &item.failed {
                return Err(error.clone());
            }
            let found = landed.get(&item.creation_id).cloned();
            found.ok_or(SetError::ServerUnavailable)
        };
        self.items
            .iter()
            .map(|item| (item.creation_id.clone(), outcome(item)))
            .collect()
    }

    /// The rows `APPENDUID` lets the bridge write at once.
    pub(super) fn rows(&mut self) -> Unwritten {
        let mut rows = Vec::new();
        for item in &mut self.items {
            let (Some(target), Some(placed)) = (&item.target, &mut item.placed) else {
                continue;
            };
            if let (Some(appended), Some(unsealed)) = (placed.appended, placed.unsealed.take()) {
                rows.push((unsealed, target.mailbox.clone(), appended));
            }
        }
        rows
    }

    /// Every draft's row as the store holds it. A row that did not go in
    /// lands through one pass, within what is left of the deadline; a
    /// pass that cannot run leaves the message on the server and the
    /// create without a row. The state afterwards when a pass ran.
    ///
    /// # Errors
    ///
    /// Returns the store's failure or `Task`.
    pub(super) async fn landed<C: Connector>(
        &self,
        wire: &mut Wire<C>,
        cache: &Cache,
        (until, written): (Option<Instant>, &[EmailId]),
    ) -> Result<(HashMap<String, Created>, Option<u64>), SyncError> {
        let placed = || self.items.iter().filter_map(|item| item.placed.as_ref());
        let mut state = None;
        if placed().any(|placed| !written.contains(&placed.id)) {
            let targets: Vec<MailboxId> = (self.items.iter())
                .filter(|item| item.placed.is_some())
                .filter_map(|item| Some(item.target.as_ref()?.mailbox.clone()))
                .collect();
            drafts::pass(wire, cache, (until, &targets)).await?;
            let (store, key) = (Arc::clone(&cache.store), cache.key.clone());
            state = Some(blocking(move || store.state(&key)).await?);
        }
        let mut landed = HashMap::new();
        for item in &self.items {
            let Some(placed) = &item.placed else {
                continue;
            };
            let (store, key) = (Arc::clone(&cache.store), cache.key.clone());
            let (hash, id) = (placed.hash.clone(), placed.id.clone());
            let wrote = written.contains(&placed.id);
            let found = blocking(move || {
                if wrote {
                    store.draft_written(&key, &id)
                } else {
                    store.landed(&key, &hash)
                }
            });
            if let Some(row) = found.await? {
                let created = Created {
                    id: row.id,
                    thread: row.thread,
                    size: row.size,
                };
                landed.insert(item.creation_id.clone(), created);
            }
        }
        Ok((landed, state))
    }
}

/// Seals the rows the bridge writes at once.
///
/// # Errors
///
/// Returns the host's failure to seal or the encoding's.
pub(super) fn sealed(rows: Unwritten, cache: &Cache) -> Result<Vec<DraftRow>, StoreError> {
    rows.into_iter()
        .map(|(unsealed, folder, appended)| {
            unsealed.sealed((folder, appended), cache.sealer.as_ref(), &cache.key)
        })
        .collect()
}

/// The creates against the rows: a mailbox they lack or that holds no
/// mail is invalid, one whose rights lack `i` forbidden (RFC 4314 section
/// 4). A destroy whose email lies in the one mailbox of the one create
/// waits on it and ends with a create that was refused.
pub(super) fn planned(ask: &SetAsk, work: &mut Work) -> Drafts {
    let items: Vec<Item> = ask
        .creates
        .iter()
        .map(|(creation_id, draft)| {
            let (target, failed) = match target(draft, work) {
                Ok(target) => (Some(target), None),
                Err(error) => (None, Some(error)),
            };
            Item {
                creation_id: creation_id.clone(),
                draft: draft.clone(),
                target,
                failed,
                placed: None,
            }
        })
        .collect();
    let pair = pair_of(ask, work);
    if items.iter().any(|item| item.failed.is_some()) {
        settle_pair(work, pair, &SetError::ServerFail);
    }
    Drafts { items, pair }
}

fn target(draft: &Draft, work: &Work) -> Result<Target, SetError> {
    let invalid = || SetError::InvalidProperties(vec!["mailboxIds".to_owned()]);
    let mailbox = MailboxId::from(draft.mailbox.clone());
    let row = work.rows.get(&mailbox).ok_or_else(invalid)?;
    if !row.facts.selectable {
        return Err(invalid());
    }
    if !may(work, &mailbox, Right::AddItems) {
        return Err(SetError::Forbidden);
    }
    let wire_name = row.facts.imap_name.clone();
    Ok(Target { mailbox, wire_name })
}

/// The tally of the destroy that waits on the one create of the ask.
fn pair_of(ask: &SetAsk, work: &Work) -> Option<usize> {
    let ([(_, draft)], [destroyed]) = (ask.creates.as_slice(), ask.destroy.as_slice()) else {
        return None;
    };
    let index = ask.updates.len();
    let row = work
        .found
        .iter()
        .find(|row| row.id.as_str() == destroyed.as_str())?;
    let member = row
        .mailboxes
        .iter()
        .any(|mailbox| mailbox.as_str() == draft.mailbox);
    (member && work.tallies[index].failed.is_none()).then_some(index)
}

/// Every create in its order on one session, each ahead of the updates
/// and the destroys. Whether the session still stands.
pub(super) async fn sent<S: Session>(
    session: &mut S,
    cache: &Cache,
    (work, drafted): (&mut Work, &mut Drafts),
    (appending, until): (Appending, Instant),
) -> bool {
    let pair = drafted.pair;
    for item in &mut drafted.items {
        let Some(target) = &item.target else {
            continue;
        };
        let built = match drafts::built(session, cache, &item.draft, appending.limit).await {
            Ok(built) => built,
            Err(Stopped { failure, stands }) => {
                let error = set_error(failure);
                settle_pair(work, pair, &error);
                item.failed = Some(error);
                if stands {
                    continue;
                }
                return false;
            }
        };
        let ask = AppendAsk {
            mailbox: &target.wire_name,
            flags: &built.flags,
            message: &built.message,
            literals: appending.literals,
            until: Some(until),
        };
        // A row on Gmail needs the ids a pass reads, which a `UID REPLACE`
        // in Spam or Trash would skip.
        let old = pair
            .filter(|_| appending.replace && !work.gmail())
            .and_then(|index| old_draft(work, index));
        let result = match &old {
            Some((folder, member)) => replaced(session, work, (folder, member), &ask).await,
            None => session.append(&ask).await.map_err(|error| failure(&error)),
        };
        match result {
            Ok(appended) => {
                let Some((folder, member)) = &old else {
                    item.placed = Some(placed(built.unsealed, appended, work.gmail()));
                    continue;
                };
                old_left(work, (folder, member));
                item.placed = Some(placed(built.unsealed, appended, false));
            }
            Err((error, stands)) => {
                settle_pair(work, pair, &error);
                item.failed = Some(error);
                if !stands {
                    return false;
                }
            }
        }
    }
    true
}

/// What the server took. On a Gmail account `APPENDUID` names a UID of
/// the label folder and of no store, so a pass lands the row there.
fn placed(unsealed: Unsealed, appended: Option<Appended>, gmail: bool) -> Placed {
    Placed {
        appended: appended.filter(|_| !gmail),
        id: unsealed.id.clone(),
        hash: unsealed.hash.clone(),
        unsealed: Some(unsealed),
    }
}

/// SELECT of the old draft's folder as a STORE selects it, then `UID
/// REPLACE` (RFC 8508 section 3.3). A folder renumbered since the sync
/// names no message by that UID and is not asked. What it comes to, with
/// whether the session still stands after a failure.
async fn replaced<S: Session>(
    session: &mut S,
    work: &mut Work,
    (folder, member): (&MailboxId, &Member),
    ask: &AppendAsk<'_>,
) -> Result<Option<Appended>, (SetError, bool)> {
    let members = std::slice::from_ref(member);
    let Some((writable, stands)) = super::selected(session, work, folder, members).await else {
        return Err((SetError::ServerUnavailable, true));
    };
    if writable.is_none() {
        let error = work.tallies[member.index].failed.clone();
        return Err((error.unwrap_or(SetError::ServerFail), stands));
    }
    session
        .replace(member.uid, ask)
        .await
        .map_err(|error| failure(&error))
}

/// The destroy that waits on the create, where it can ride one command.
fn old_draft(work: &Work, index: usize) -> Option<(MailboxId, Member)> {
    work.folders.iter().find_map(|(folder, items)| {
        let member = items.destroys.iter().find(|member| member.index == index)?;
        Some((folder.clone(), member.clone()))
    })
}

/// The message a `UID REPLACE` took away: its destroy is done and its row
/// leaves with the new one landing.
fn old_left(work: &mut Work, (folder, member): (&MailboxId, &Member)) {
    work.done(member);
    work.stored.gone.push(GoneRow {
        id: member.id.clone(),
        folder: folder.clone(),
        uid: member.uid,
    });
    if let Some(entry) = work.folders.get_mut(folder) {
        entry.destroys.retain(|held| held.index != member.index);
    }
}

/// The destroy of a pair ends with the create that did not land: a
/// refusal of any kind as the server's, a server out of reach as that.
fn settle_pair(work: &mut Work, pair: Option<usize>, error: &SetError) {
    if let Some(index) = pair {
        let held = match error {
            SetError::ServerUnavailable => SetError::ServerUnavailable,
            _ => SetError::ServerFail,
        };
        work.tallies[index].failed.get_or_insert(held);
    }
}

fn set_error(failure: Failure) -> SetError {
    match failure {
        Failure::BlobNotFound(ids) => SetError::BlobNotFound(ids),
        Failure::TooLarge => SetError::TooLarge,
        Failure::ServerFail => SetError::ServerFail,
        Failure::ServerUnavailable => SetError::ServerUnavailable,
    }
}
