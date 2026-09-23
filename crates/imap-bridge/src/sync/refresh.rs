// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The on-demand refresh of an account: one mailbox pass, then per
//! folder that is done the new mail above its recorded UIDNEXT, the
//! flags that changed and the messages that left. Every write is one
//! state and goes through only while the folder stands as the refresh
//! read it. On Gmail a row whose UID left waits for the pass to find it
//! in another store; what the pass does not find leaves at its end.

mod folder;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use self::folder::Refreshing;
use super::{Cache, FolderSync, Step, blocking};
use crate::mailboxes::{self, SyncError};
use crate::session::{Session, SessionError};
use crate::store::{MailboxId, MailboxRow, Progress, Synced};

/// What one refresh is given beyond the account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Given<'a> {
    /// Whether CONDSTORE is advertised, so the flags of a folder come as
    /// one CHANGEDSINCE answer.
    pub condstore: bool,
    /// Whether the account is a Gmail account on a server that says so:
    /// the labels ride every fetch and a row whose UID left waits for
    /// the pass.
    pub gmail: bool,
    /// The mailbox the client looked at last; without CONDSTORE only its
    /// flags are scanned.
    pub viewed: Option<&'a MailboxId>,
}

/// One refresh: where it writes and what it knows.
#[derive(Clone, Copy)]
pub(super) struct Run<'a> {
    pub(super) cache: &'a Cache,
    pub(super) given: Given<'a>,
}

/// What one refresh leaves behind: whether the session still stands and
/// whether a store folder waits for its first sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refreshed {
    /// After `false` the caller drops the session and connects again
    /// before the next refresh.
    pub stands: bool,
    /// A store folder whose first sync is not done, which the account's
    /// task picks up.
    pub undone: bool,
}

/// What the refreshes of one account carry from one to the next.
#[derive(Default)]
pub struct Refresher {
    /// The folders whose flags are scanned range by range, since one
    /// CHANGEDSINCE answer passed its bound.
    scanned: HashSet<MailboxId>,
    /// The walks upward a failure cut short, resumed by the next
    /// refresh with their narrowing.
    pending: HashMap<MailboxId, FolderSync>,
    /// The folder the next refresh starts at, so one that fails every
    /// time still lets the others through.
    first: Option<MailboxId>,
}

impl Refresher {
    /// One pass over an account, on one session. Answers whether the
    /// session still stands and whether a store folder waits for its
    /// first sync.
    ///
    /// # Errors
    ///
    /// Returns the store's failure or `Task`. A failure of the session
    /// ends the refresh where it stands and reads as a session that
    /// does not stand, since the cache is no worse for it.
    pub async fn refresh<S: Session>(
        &mut self,
        session: &mut S,
        cache: &Cache,
        given: Given<'_>,
    ) -> Result<Refreshed, SyncError> {
        let found = match mailboxes::observe(session, given.gmail).await {
            Ok(found) => found,
            Err(error) => return Ok(ended(&error, false)),
        };
        let (store, key) = (Arc::clone(&cache.store), cache.key.clone());
        blocking(move || store.apply_mailboxes(&key, &found)).await?;
        let (store, key) = (Arc::clone(&cache.store), cache.key.clone());
        let snapshot = blocking(move || store.mailbox_snapshot(&key)).await?;
        let undone = snapshot
            .rows
            .iter()
            .any(|row| row.facts.store && !snapshot.done.contains(&row.id));
        // A label mailbox is done with its store and has no UIDs of its own.
        let mut rows: Vec<&MailboxRow> = snapshot
            .rows
            .iter()
            .filter(|row| row.facts.store && snapshot.done.contains(&row.id))
            .collect();
        if let Some(first) = self.first.take()
            && let Some(index) = rows.iter().position(|row| row.id == first)
        {
            rows.rotate_left(index);
        }
        let run = Run { cache, given };
        for (index, row) in rows.iter().enumerate() {
            match self.folder(session, run, row).await {
                Ok(()) => {}
                Err(SyncError::Session(error)) => {
                    self.first = rows.get(index + 1).map(|row| row.id.clone());
                    return Ok(ended(&error, undone));
                }
                Err(other) => return Err(other),
            }
        }
        if given.gmail {
            let (store, key) = (Arc::clone(&cache.store), cache.key.clone());
            blocking(move || store.sweep_parked(&key)).await?;
        }
        Ok(Refreshed {
            stands: true,
            undone,
        })
    }

    /// One folder that is done: its new mail, its flags, what left. A
    /// folder STATUS shows as the cache last saw it costs no command.
    async fn folder<S: Session>(
        &mut self,
        session: &mut S,
        run: Run<'_>,
        row: &MailboxRow,
    ) -> Result<(), SyncError> {
        let facts = &row.facts;
        let (Some(uid_validity), Some(server_next)) = (facts.uid_validity, facts.uid_next) else {
            return Ok(());
        };
        let (store, key, id) = (
            Arc::clone(&run.cache.store),
            run.cache.key.clone(),
            row.id.clone(),
        );
        let progress = blocking(move || store.sync_progress(&key, &id)).await?;
        if self.unchanged(run, row, &progress) {
            return Ok(());
        }
        let selected = match session.examine(&facts.imap_name).await {
            Ok(selected) if selected.uid_validity == uid_validity => selected,
            Ok(_) | Err(SessionError::Refused) => return Ok(()),
            Err(other) => return Err(other.into()),
        };
        let refreshing = Refreshing {
            row,
            uid_validity,
            progress,
            server_next,
            gmail: run.given.gmail,
        };
        let Some(arrived) = self.new_mail(session, run, &refreshing).await? else {
            return Ok(());
        };
        refreshing.flags(session, run, &mut self.scanned).await?;
        // The count on record already holds what earlier batches of the
        // walk answered, so only this refresh's messages are added.
        let counted = progress.synced.messages.unwrap_or(facts.total_emails);
        if facts.total_emails < counted.saturating_add(arrived) {
            refreshing.expunged(session, run.cache).await?;
        }
        let synced = Synced {
            uid_next: Some(server_next),
            highest_modseq: selected.highest_modseq,
            messages: Some(facts.total_emails),
        };
        refreshing.advance(run.cache, synced).await
    }

    /// Whether STATUS shows the folder as the cache last saw it and no
    /// walk waits for it, so nothing has changed that a command would
    /// find. Without CONDSTORE a flag change shows in no STATUS item, so
    /// the folder the client looks at is read regardless.
    fn unchanged(&self, run: Run<'_>, row: &MailboxRow, progress: &Progress) -> bool {
        let facts = &row.facts;
        let shown = Synced {
            uid_next: facts.uid_next,
            highest_modseq: facts.highest_modseq,
            messages: Some(facts.total_emails),
        };
        let flags_unseen = !run.given.condstore && run.given.viewed == Some(&row.id);
        progress.synced == shown && !flags_unseen && !self.pending.contains_key(&row.id)
    }

    /// Fetches what arrived above the recorded UIDNEXT, oldest first,
    /// under the narrowing of the first sync. A walk a failure cut
    /// short waits for the next refresh with its narrowing kept. The
    /// messages the server answered in this refresh, which the count on
    /// record does not hold yet; `None` when the folder went stale under
    /// the walk.
    async fn new_mail<S: Session>(
        &mut self,
        session: &mut S,
        run: Run<'_>,
        refreshing: &Refreshing<'_>,
    ) -> Result<Option<u32>, SyncError> {
        let Some(through) = refreshing.server_next.checked_sub(1) else {
            return Ok(Some(0));
        };
        let id = &refreshing.row.id;
        let kept = self
            .pending
            .remove(id)
            .and_then(|mut pending| pending.extend_to(through).then_some(pending));
        let mut sync = if let Some(pending) = kept {
            pending
        } else {
            let from = match refreshing.progress.synced.uid_next {
                Some(from) => from,
                // A folder without a recorded UIDNEXT: what it holds
                // ends where the highest stored UID does.
                None => refreshing
                    .stored(run.cache)
                    .await?
                    .last()
                    .map_or(1, |uid| uid + 1),
            };
            let range = (from, through);
            let opened = FolderSync::above(session, refreshing.row, range, run.given.gmail).await?;
            let Some(sync) = opened else {
                return Ok(Some(0));
            };
            sync
        };
        let before = sync.answered();
        match sync.finish(session, run.cache).await {
            Ok(Step::Stale) => Ok(None),
            Ok(_) => Ok(Some(sync.answered().saturating_sub(before))),
            Err(error @ SyncError::Session(_)) => {
                self.pending.insert(id.clone(), sync);
                Err(error)
            }
            Err(other) => Err(other),
        }
    }
}

/// Whether the session still stands after this failure: a tagged NO
/// leaves it usable, anything else ends it.
fn stands(error: &SessionError) -> bool {
    matches!(error, SessionError::Refused)
}

/// A refresh the session's failure ended.
fn ended(error: &SessionError, undone: bool) -> Refreshed {
    Refreshed {
        stands: stands(error),
        undone,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::MESSAGE_LIMIT;

    #[test]
    fn a_refusal_keeps_the_session_and_any_other_failure_ends_it() {
        assert!(stands(&SessionError::Refused));
        assert!(!stands(&SessionError::Closed));
        assert!(!stands(&SessionError::Timeout));
        assert!(!stands(&SessionError::Protocol(MESSAGE_LIMIT)));
    }
}
