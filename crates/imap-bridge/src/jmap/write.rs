// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What an `Email/set` asked, done on the server between two passes over
//! the request, on the account's conversation under the lock and the
//! deadline. Per store folder a SELECT, then the keyword STOREs, the
//! label STOREs of a Gmail account, the moves and the destroys, each
//! group one command set; a move a Gmail account makes into All Mail
//! has its labels stored there afterwards. What the server took is then
//! written to the rows as one state. A write that fails leaves its rows
//! as they were; the next refresh reads what the server holds.

mod keywords;
mod moves;
mod plan;
mod restore;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Map, Value, json};
use tokio::time::{Instant, timeout};

use super::set::{Outcome, SetAsk, SetError, Updated};
use crate::mailboxes::SyncError;
use crate::runtime::{Connector, Link, Wire};
use crate::session::{Moving, Session, SessionError, Writable};
use crate::store::{EmailId, MailboxId, MailboxRow, SetWrite, SetWritten};
use crate::sync::{Cache, blocking};

/// One email a command names.
#[derive(Debug, Clone)]
struct Member {
    /// Where its result stands among the updates and destroys of the
    /// ask.
    index: usize,
    id: EmailId,
    uid: u32,
}

/// Where one update or destroy stands while the work runs: the parts it
/// still waits on and the first failure, which ends it.
#[derive(Debug, Clone, Default)]
struct Tally {
    left: u8,
    failed: Option<SetError>,
}

/// The work of one folder, in the order it runs.
#[derive(Default)]
struct Folder {
    keywords: Vec<keywords::Group>,
    labels: Vec<moves::LabelGroup>,
    moves: Vec<moves::MoveGroup>,
    destroys: Vec<Member>,
}

impl Folder {
    /// Whether the work moves mail or changes a Gmail label, which puts
    /// the mail elsewhere in a search.
    fn moves_mail(&self) -> bool {
        !self.moves.is_empty() || !self.labels.is_empty()
    }

    fn members(&self) -> impl Iterator<Item = &Member> {
        let keywords = self.keywords.iter().flat_map(|group| &group.members);
        let labels = self.labels.iter().flat_map(|group| &group.members);
        let moves = self.moves.iter().flat_map(|group| &group.members);
        keywords.chain(labels).chain(moves).chain(&self.destroys)
    }
}

/// An email as its update should leave it: what the server changes
/// beyond that is part of the answer (RFC 8620 section 5.3).
#[derive(Debug, Clone, Default)]
struct Expected {
    mailboxes: BTreeSet<MailboxId>,
    keywords: BTreeMap<String, bool>,
}

/// What one ask works through.
struct Work {
    folders: BTreeMap<MailboxId, Folder>,
    rows: HashMap<MailboxId, MailboxRow>,
    /// The updates of the ask, then its destroys.
    tallies: Vec<Tally>,
    /// What the server took, for the rows.
    stored: SetWrite,
    /// Moves into All Mail whose labels are stored there afterwards.
    restores: Vec<restore::Restore>,
    moving: Moving,
    state: u64,
}

impl Work {
    /// Whether the account is a Gmail account, as the rows are told too.
    fn gmail(&self) -> bool {
        self.stored.gmail
    }

    /// Whether the server moves mail for this account. A Gmail account
    /// moves with `UID MOVE` alone: a copy and an expunge would run in
    /// All Mail, where the account's own setting decides what an expunge
    /// does.
    fn moves_mail(&self) -> bool {
        if self.gmail() {
            self.moving.move_command
        } else {
            self.moving.moves()
        }
    }

    /// A part of the member's update or destroy is done.
    fn done(&mut self, member: &Member) {
        let tally = &mut self.tallies[member.index];
        tally.left = tally.left.saturating_sub(1);
    }

    /// Every update or destroy of these members ends with the error,
    /// unless an earlier failure ended it.
    fn settle(&mut self, members: &[Member], error: &SetError) {
        for member in members {
            let failed = &mut self.tallies[member.index].failed;
            failed.get_or_insert_with(|| error.clone());
        }
    }

    /// The members whose update or destroy still stands, in the order
    /// of their UIDs, so neighbors fold into ranges.
    fn standing(&self, members: &[Member]) -> Vec<Member> {
        let mut standing: Vec<Member> = members
            .iter()
            .filter(|member| self.tallies[member.index].failed.is_none())
            .cloned()
            .collect();
        standing.sort_unstable_by_key(|member| member.uid);
        standing
    }

    /// The wire name of a folder the store holds.
    fn wire_name(&self, folder: &MailboxId) -> Option<&str> {
        let row = self.rows.get(folder)?;
        Some(row.facts.imap_name.as_str())
    }
}

/// Does what the ask names and answers what came of every update and
/// destroy. A state other than the one `ifInState` names ends it before
/// anything is sent. The conversation is held from that check until the
/// rows are written, so two writes of one account never pass on one
/// state. The writes of one round share a deadline: `until` is set by
/// the first that goes to the server and a write that finds it over
/// sends nothing.
///
/// # Errors
///
/// Returns the store's failure or `Task`.
pub(super) async fn apply<C: Connector>(
    cache: &Cache,
    link: &Link<C>,
    ask: &SetAsk,
    until: &mut Option<Instant>,
) -> Result<Outcome, SyncError> {
    let mut wire = link.wire.lock().await;
    let Some((mut work, expected)) = plan::planned(cache, ask).await? else {
        return Ok(Outcome::StateMismatch);
    };
    if !work.folders.is_empty() {
        let until = *until.get_or_insert_with(|| Instant::now() + wire.deadline());
        let left = until.saturating_duration_since(Instant::now());
        let moves = work.folders.values().any(Folder::moves_mail);
        send(&mut wire, cache, &mut work, left).await;
        if moves {
            link.search.forget();
        }
    }
    let Work {
        tallies,
        stored,
        state,
        ..
    } = work;
    let written = if stored.is_empty() {
        None
    } else {
        let (store, key) = (Arc::clone(&cache.store), cache.key.clone());
        Some(blocking(move || store.write_set(&key, &stored)).await?)
    };
    let (old_state, new_state) = written
        .as_ref()
        .map_or((state, state), |written| (written.before, written.after));
    let (of_updates, of_destroys) = tallies.split_at(ask.updates.len());
    let updated = ask
        .updates
        .iter()
        .zip(of_updates)
        .zip(&expected)
        .map(|(((id, _), tally), expected)| {
            let written = written.as_ref();
            let result = ended(tally).and_then(|()| beyond(id, expected.as_ref(), written));
            (id.clone(), result)
        })
        .collect();
    let destroyed = ask
        .destroy
        .iter()
        .zip(of_destroys)
        .map(|(id, tally)| (id.clone(), ended(tally)))
        .collect();
    Ok(Outcome::Stored {
        old_state,
        new_state,
        updated,
        destroyed,
    })
}

/// How an update or a destroy ended: its failure or done. One a session
/// left before its turn could not be asked.
fn ended(tally: &Tally) -> Result<(), SetError> {
    match (&tally.failed, tally.left) {
        (Some(error), _) => Err(error.clone()),
        (None, 0) => Ok(()),
        (None, _) => Err(SetError::ServerUnavailable),
    }
}

/// An update done: the properties the server changed beyond what the
/// patch asked, as they stand. A row gone or moved elsewhere by the time
/// it was written is not found.
fn beyond(
    id: &str,
    expected: Option<&Expected>,
    written: Option<&SetWritten>,
) -> Result<Updated, SetError> {
    let Some(written) = written else {
        return Ok(None);
    };
    if written.stale.iter().any(|stale| stale.as_str() == id) {
        return Err(SetError::NotFound);
    }
    let found = written.emails.get(&EmailId::from(id.to_owned()));
    let (Some(expected), Some(found)) = (expected, found) else {
        return Ok(None);
    };
    let mut changed = Map::new();
    if found.mailboxes != expected.mailboxes {
        let mailboxes: Map<String, Value> = found
            .mailboxes
            .iter()
            .map(|mailbox| (mailbox.to_string(), Value::Bool(true)))
            .collect();
        changed.insert("mailboxIds".to_owned(), Value::Object(mailboxes));
    }
    if found.keywords != expected.keywords {
        changed.insert("keywords".to_owned(), json!(found.keywords));
    }
    Ok((!changed.is_empty()).then_some(changed))
}

/// Sends the work on the account's session within what is left of the
/// deadline; with none of it left the session is not asked at all. A
/// deadline that fires reads as a session that does not stand.
async fn send<C: Connector>(wire: &mut Wire<C>, cache: &Cache, work: &mut Work, left: Duration) {
    if left.is_zero() {
        return;
    }
    let run = async {
        let Ok((session, moving)) = wire.session_with(cache).await else {
            return false;
        };
        work.moving = moving;
        store(session, work).await
    };
    if !timeout(left, run).await.unwrap_or(false) {
        wire.drop_session();
    }
}

/// Every folder on one session, then the labels of what moved into All
/// Mail. Whether the session still stands; a failure that ends it
/// leaves what comes after it as it was.
async fn store<S: Session>(session: &mut S, work: &mut Work) -> bool {
    let folders = std::mem::take(&mut work.folders);
    for (folder, items) in folders {
        if !folder_stored(session, work, &folder, items).await {
            return false;
        }
    }
    restore::restored(session, work).await
}

/// One folder: SELECT, then its keywords, labels, moves and destroys in
/// order. A server that offers no way to move mail gets no move and no
/// label asked, a Gmail server without MOVE no move and one without
/// UIDPLUS no destroy; a folder with nothing left to do is not selected.
/// Whether the session still stands.
async fn folder_stored<S: Session>(
    session: &mut S,
    work: &mut Work,
    folder: &MailboxId,
    items: Folder,
) -> bool {
    if !work.moving.moves() {
        let labels = items.labels.iter().flat_map(|group| &group.members);
        let labels: Vec<Member> = labels.cloned().collect();
        work.settle(&labels, &SetError::Forbidden);
    }
    if !work.moves_mail() {
        let moves = items.moves.iter().flat_map(|group| &group.members);
        let moves: Vec<Member> = moves.cloned().collect();
        work.settle(&moves, &SetError::Forbidden);
    }
    if !work.moving.uidplus {
        work.settle(&items.destroys, &SetError::Forbidden);
    }
    let members: Vec<Member> = items.members().cloned().collect();
    if work.standing(&members).is_empty() {
        return true;
    }
    let Some((writable, stands)) = selected(session, work, folder, &members).await else {
        return true;
    };
    let Some(writable) = writable else {
        return stands;
    };
    keywords::stored(session, work, (folder, &writable), items.keywords).await
        && moves::labeled(session, work, folder, items.labels).await
        && moves::moved(session, work, (folder, &writable), items.moves).await
        && moves::destroyed(session, work, (folder, &writable), &items.destroys).await
}

/// SELECT of a folder for its members' writes. `None` when the folder
/// is gone from the store or renumbered, which cannot be written now; a
/// folder the server refused or opened read-only settles its members
/// and comes back without a mailbox, with whether the session stands.
async fn selected<S: Session>(
    session: &mut S,
    work: &mut Work,
    folder: &MailboxId,
    members: &[Member],
) -> Option<(Option<Writable>, bool)> {
    let facts = &work.rows.get(folder)?.facts;
    let (name, uid_validity) = (facts.imap_name.clone(), facts.uid_validity);
    match session.select(&name).await {
        Ok(writable) if Some(writable.uid_validity) != uid_validity => None,
        // A folder the server opened read-only takes no write.
        Ok(writable) if writable.read_only => {
            work.settle(members, &SetError::Forbidden);
            Some((None, true))
        }
        Ok(writable) => Some((Some(writable), true)),
        Err(error) => {
            let (ends, stands) = failure(&error);
            work.settle(members, &ends);
            Some((None, stands))
        }
    }
}

/// What a failed command comes to: the error its updates end with and
/// whether the session still stands. A NO is the server's refusal and
/// keeps the session, an answer that cannot be read is its failure and
/// costs it, anything else leaves the server unreached.
fn failure(error: &SessionError) -> (SetError, bool) {
    match error {
        SessionError::Refused => (SetError::ServerFail, true),
        SessionError::Protocol(_) => (SetError::ServerFail, false),
        _ => (SetError::ServerUnavailable, false),
    }
}
