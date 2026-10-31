// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What a `Mailbox/set` asks of the server (RFC 8621 section 2.5), done
//! between two passes over its request on the account's conversation
//! under the lock and the deadline. Every object is checked against the
//! rows before anything is sent; then, in the order RFC 8620 section
//! 5.3 gives creates, updates and destroys, CREATE with SUBSCRIBE,
//! RENAME, SUBSCRIBE or UNSUBSCRIBE and DELETE, each step naming what
//! the steps before it made or moved. What the server took is written
//! to the rows as one state. After a RENAME or a DELETE the server
//! refused, one pass on a fresh connection shows what stands; after any
//! refusal the next use signs in afresh, so on Gmail it sees the
//! folders as they stand.

mod delimiters;
mod failure;
mod names;
mod plan;
mod send;
mod step;

use std::collections::BTreeMap;
use std::sync::Arc;

use tokio::time::{Instant, timeout};

use self::failure::Refused;
use self::send::Work;
use self::step::Step;
use super::{Connector, Link};
use crate::mailboxes::{self, SyncError};
use crate::store::{MailboxId, MailboxSnapshot};
use crate::sync::{Cache, blocking};

/// How a reference names a record by its creation id.
const CREATION_REFERENCE: char = '#';

/// The creation id a reference names, if it is one (RFC 8620 section
/// 5.3).
pub(crate) fn creation_id(reference: &str) -> Option<&str> {
    reference.strip_prefix(CREATION_REFERENCE)
}

/// The creation ids a request resolved so far, each to the id of the
/// record it stands for: the ones the request came with and every
/// mailbox a `Mailbox/set` of it made, the latest winning (RFC 8620
/// sections 3.3 and 5.3).
pub(crate) type CreatedIds = BTreeMap<String, String>;

/// What the writes of one round of a request share: the deadline the
/// first of them sets and the creation ids resolved so far.
pub(crate) struct Round<'a> {
    pub until: Option<Instant>,
    pub created: &'a mut CreatedIds,
}

/// What one `Mailbox/set` asks, each object as its call read it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Ask {
    pub if_in_state: Option<String>,
    pub creates: Vec<Create>,
    pub updates: Vec<Update>,
    /// Each id once, in the order named.
    pub destroys: Vec<String>,
    /// `onDestroyRemoveEmails`.
    pub remove_emails: bool,
}

/// One create: the name the user typed and where it goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Create {
    pub creation_id: String,
    pub name: String,
    /// An id or a creation id of the call after `#`; `None` for the top
    /// level.
    pub parent: Option<String>,
    /// `Null` for no role whatever the server names; `None` leaves the
    /// role to the next pass.
    pub role: Option<Nullable>,
    pub subscribed: Option<bool>,
}

/// One update: every property it names, `None` where it names none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Update {
    pub id: String,
    pub name: Option<String>,
    pub parent: Option<Nullable>,
    pub role: Option<Nullable>,
    pub subscribed: Option<bool>,
}

/// What an update sets a property to that may be null.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Nullable {
    Null,
    Value(String),
}

impl Nullable {
    pub(crate) fn as_deref(&self) -> Option<&str> {
        match self {
            Self::Null => None,
            Self::Value(value) => Some(value),
        }
    }
}

impl From<Option<String>> for Nullable {
    fn from(value: Option<String>) -> Self {
        value.map_or(Self::Null, Self::Value)
    }
}

/// Why one object was not written (RFC 8620 section 5.3, RFC 8621
/// section 2.5). The last two say the server refused the command or
/// could not be asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Refusal {
    Forbidden,
    NotFound,
    InvalidProperties(Vec<&'static str>),
    MailboxHasChild,
    MailboxHasEmail,
    WillDestroy,
    ServerFail,
    ServerUnavailable,
}

/// What came of one ask.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// `ifInState` named another state than the account stood at.
    StateMismatch,
    Done {
        /// The rows as the call found them, whose state is the old one.
        before: Box<MailboxSnapshot>,
        new_state: u64,
        /// Every create by creation id with the id it took.
        created: Vec<(String, Result<MailboxId, Refusal>)>,
        /// Every update by id as written with the id of the mailbox it
        /// changed.
        updated: Vec<(String, Result<MailboxId, Refusal>)>,
        /// Every destroy by id as written with the id of the mailbox
        /// that left.
        destroyed: Vec<(String, Result<MailboxId, Refusal>)>,
    },
}

/// Does what the ask names and answers what came of every object. The
/// conversation is held from the state check until the rows are
/// written. The writes of one round share a deadline: `until` is set by
/// the first that goes to the server and a write that finds it over
/// sends nothing. Every mailbox made joins the round's creation ids.
///
/// # Errors
///
/// Returns the store's failure or `Task`.
pub(crate) async fn apply<C: Connector>(
    cache: &Cache,
    link: &Link<C>,
    ask: &Ask,
    round: &mut Round<'_>,
) -> Result<Outcome, SyncError> {
    let mut wire = link.wire.lock().await;
    let (store, key) = (Arc::clone(&cache.store), cache.key.clone());
    let snapshot = blocking(move || store.mailbox_snapshot(&key)).await?;
    let state = snapshot.state;
    if ask
        .if_in_state
        .as_ref()
        .is_some_and(|named| *named != state.to_string())
    {
        return Ok(Outcome::StateMismatch);
    }
    let planned = plan::planned(&snapshot, ask, round.created);
    let mut work = Work::new(ask, &snapshot, &planned);
    let (local, remote): (Vec<Step>, Vec<Step>) =
        planned.steps.into_iter().partition(Step::is_local);
    if !remote.is_empty() {
        let until = *round
            .until
            .get_or_insert_with(|| Instant::now() + wire.deadline());
        let left = until.saturating_duration_since(Instant::now());
        let stands = !left.is_zero() && {
            let run = async {
                let Ok(session) = wire.session(cache).await else {
                    return false;
                };
                work.remote(session, &remote).await
            };
            timeout(left, run).await.unwrap_or(false)
        };
        if !stands || work.refused != Refused::Nothing {
            wire.drop_session();
        }
        if work.moves_mail {
            link.search.forget();
        }
    }
    // After the steps on the session, so a mailbox the call creates is
    // a row by then.
    work.local(local);
    let changes = std::mem::take(&mut work.changes);
    let (store, key) = (Arc::clone(&cache.store), cache.key.clone());
    let written = blocking(move || store.write_mailboxes(&key, &changes)).await?;
    let mut new_state = written.after;
    if work.refused == Refused::Move {
        new_state = settle(&mut wire, cache, round.until)
            .await?
            .unwrap_or(new_state);
    }
    let outcome = work.outcome(snapshot, new_state);
    if let Outcome::Done { created, .. } = &outcome {
        for (creation_id, id) in created {
            if let Ok(id) = id {
                round.created.insert(creation_id.clone(), id.to_string());
            }
        }
    }
    Ok(outcome)
}

/// One pass on a fresh connection after a RENAME or a DELETE the
/// server refused, within what is left of the deadline, so the rows show
/// what stands (Cyrus renames a tree one mailbox at a time). The state
/// afterwards; `None` when no pass ran.
async fn settle<C: Connector>(
    wire: &mut super::Wire<C>,
    cache: &Cache,
    until: Option<Instant>,
) -> Result<Option<u64>, SyncError> {
    let left = until.map_or(wire.deadline(), |until| {
        until.saturating_duration_since(Instant::now())
    });
    wire.drop_session();
    let run = async {
        let session = wire.session(cache).await.ok()?;
        Some(mailboxes::sync(session, cache).await)
    };
    match timeout(left, run).await {
        Ok(Some(Ok(state))) => {
            wire.unpassed = false;
            Ok(Some(state))
        }
        Ok(Some(Err(SyncError::Session(_))) | None) | Err(_) => {
            wire.drop_session();
            Ok(None)
        }
        Ok(Some(Err(other))) => Err(other),
    }
}
