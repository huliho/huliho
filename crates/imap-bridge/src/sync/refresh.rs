// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The on-demand refresh of an account: one mailbox pass, then per
//! folder that is done the new mail above its recorded UIDNEXT, the
//! flags that changed and the messages that left. Every write is one
//! state and goes through only while the folder stands as the refresh
//! read it.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::{Cache, FolderSync, SYNC_BATCH, Step, blocking, mapping};
use crate::mailboxes::{self, SyncError};
use crate::session::{FlagFetch, Flagged, MESSAGE_LIMIT, Session, SessionError, UidRange};
use crate::store::{FlagChange, MailboxId, MailboxRow, Progress, Standing, Synced};

/// What one refresh is given beyond the account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Given<'a> {
    /// Whether CONDSTORE is advertised, so the flags of a folder come as
    /// one CHANGEDSINCE answer.
    pub condstore: bool,
    /// The mailbox the client looked at last; without CONDSTORE only its
    /// flags are scanned.
    pub viewed: Option<&'a MailboxId>,
}

/// One refresh: where it writes and what it knows.
#[derive(Clone, Copy)]
struct Run<'a> {
    cache: &'a Cache,
    given: Given<'a>,
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
    /// session still stands; after `false` the caller drops it and
    /// connects again before the next refresh.
    ///
    /// # Errors
    ///
    /// Returns the store's failure or `Task`. A failure of the session
    /// ends the refresh where it stands and answers `false`, since the
    /// cache is no worse for it.
    pub async fn refresh<S: Session>(
        &mut self,
        session: &mut S,
        cache: &Cache,
        given: Given<'_>,
    ) -> Result<bool, SyncError> {
        let found = match mailboxes::observe(session).await {
            Ok(found) => found,
            Err(error) => return Ok(stands(&error)),
        };
        let (store, key) = (Arc::clone(&cache.store), cache.key.clone());
        blocking(move || store.apply_mailboxes(&key, &found)).await?;
        let (store, key) = (Arc::clone(&cache.store), cache.key.clone());
        let snapshot = blocking(move || store.mailbox_snapshot(&key)).await?;
        let mut rows: Vec<&MailboxRow> = snapshot
            .rows
            .iter()
            .filter(|row| snapshot.done.contains(&row.id))
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
                    return Ok(stands(&error));
                }
                Err(other) => return Err(other),
            }
        }
        Ok(true)
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
            let opened = FolderSync::above(session, refreshing.row, (from, through)).await?;
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

/// One folder inside a refresh, as the pass and the store had it.
#[derive(Clone, Copy)]
struct Refreshing<'a> {
    row: &'a MailboxRow,
    uid_validity: u32,
    progress: Progress,
    /// UIDNEXT as the pass of this refresh read it.
    server_next: u32,
}

impl Refreshing<'_> {
    /// The folder and the UIDVALIDITY as owned values, for a store call
    /// off the runtime.
    fn owned(&self) -> (MailboxId, u32) {
        (self.row.id.clone(), self.uid_validity)
    }

    async fn stored(&self, cache: &Cache) -> Result<Vec<u32>, SyncError> {
        let (store, key, (folder, uid_validity)) =
            (Arc::clone(&cache.store), cache.key.clone(), self.owned());
        blocking(move || {
            let standing = Standing {
                folder: &folder,
                uid_validity,
            };
            store.stored_uids(&key, &standing)
        })
        .await
    }

    /// The flags that changed: one CHANGEDSINCE answer where CONDSTORE
    /// is advertised, a scan of the stored UIDs range by range for the
    /// viewed folder without it and for a folder whose CHANGEDSINCE
    /// answer passed its bound.
    async fn flags<S: Session>(
        &self,
        session: &mut S,
        run: Run<'_>,
        scanned: &mut HashSet<MailboxId>,
    ) -> Result<(), SyncError> {
        let id = &self.row.id;
        let scan = scanned.contains(id) || (!run.given.condstore && run.given.viewed == Some(id));
        if scan {
            return self.scan(session, run.cache).await;
        }
        let since = self.progress.synced.highest_modseq;
        let Some(since) = since.filter(|_| run.given.condstore) else {
            return Ok(());
        };
        match session.uid_flags(FlagFetch::ChangedSince(since)).await {
            Ok(flagged) => self.apply(run.cache, &flagged).await,
            Err(error @ SessionError::Protocol(words)) => {
                if words == MESSAGE_LIMIT {
                    scanned.insert(id.clone());
                }
                Err(error.into())
            }
            Err(other) => Err(other.into()),
        }
    }

    /// The stored UIDs in ranges of `SYNC_BATCH`, each asked for its
    /// flags.
    async fn scan<S: Session>(&self, session: &mut S, cache: &Cache) -> Result<(), SyncError> {
        for range in ranges(&self.stored(cache).await?) {
            let flagged = session.uid_flags(FlagFetch::Range(range)).await?;
            self.apply(cache, &flagged).await?;
        }
        Ok(())
    }

    /// Writes what the flags say in transactions of `SYNC_BATCH`: a
    /// message that gained `\Deleted` leaves, the keywords of the rest
    /// are compared to the rows.
    async fn apply(&self, cache: &Cache, flagged: &[Flagged]) -> Result<(), SyncError> {
        let deleted: Vec<u32> = flagged
            .iter()
            .filter(|found| mapping::is_deleted(&found.flags))
            .map(|found| found.uid)
            .collect();
        for uids in deleted.chunks(SYNC_BATCH) {
            self.write(cache, Write::Remove(uids.to_vec())).await?;
        }
        let changes: Vec<FlagChange> = flagged
            .iter()
            .filter(|found| !mapping::is_deleted(&found.flags))
            .map(|found| FlagChange {
                uid: found.uid,
                keywords: mapping::keywords(&found.flags),
            })
            .collect();
        for batch in changes.chunks(SYNC_BATCH) {
            self.write(cache, Write::Flags(batch.to_vec())).await?;
        }
        Ok(())
    }

    /// MESSAGES fell short of what the cache knows plus what arrived:
    /// the UID list names what is left and the stored UIDs it lacks
    /// leave.
    async fn expunged<S: Session>(&self, session: &mut S, cache: &Cache) -> Result<(), SyncError> {
        let present: HashSet<u32> = session.uid_list().await?.into_iter().collect();
        let gone: Vec<u32> = self
            .stored(cache)
            .await?
            .into_iter()
            .filter(|uid| !present.contains(uid))
            .collect();
        for uids in gone.chunks(SYNC_BATCH) {
            self.write(cache, Write::Remove(uids.to_vec())).await?;
        }
        Ok(())
    }

    async fn advance(&self, cache: &Cache, synced: Synced) -> Result<(), SyncError> {
        let (store, key, (folder, uid_validity)) =
            (Arc::clone(&cache.store), cache.key.clone(), self.owned());
        blocking(move || {
            let standing = Standing {
                folder: &folder,
                uid_validity,
            };
            store.advance(&key, &standing, synced)
        })
        .await
    }

    async fn write(&self, cache: &Cache, write: Write) -> Result<(), SyncError> {
        let (store, key, (folder, uid_validity)) =
            (Arc::clone(&cache.store), cache.key.clone(), self.owned());
        blocking(move || {
            let standing = Standing {
                folder: &folder,
                uid_validity,
            };
            match write {
                Write::Remove(uids) => store.remove_uids(&key, &standing, &uids),
                Write::Flags(changes) => store.apply_flags(&key, &standing, &changes),
            }
        })
        .await
        .map(|_state| ())
    }
}

/// One write of the refresh, moved to the blocking thread whole.
enum Write {
    Remove(Vec<u32>),
    Flags(Vec<FlagChange>),
}

/// Sorted UIDs as ranges of `SYNC_BATCH` messages at most, each range
/// ending on stored UIDs.
fn ranges(uids: &[u32]) -> Vec<UidRange> {
    uids.chunks(SYNC_BATCH)
        .filter_map(|chunk| {
            Some(UidRange {
                low: *chunk.first()?,
                high: *chunk.last()?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_uids_are_asked_in_ranges_of_one_batch() {
        let count = u32::try_from(SYNC_BATCH).unwrap();
        let uids: Vec<u32> = (1..=count + 2).map(|uid| uid * 2).collect();
        let found = ranges(&uids);
        assert_eq!(found.len(), 2);
        assert_eq!(
            found[0],
            UidRange {
                low: 2,
                high: count * 2
            }
        );
        assert_eq!(
            found[1],
            UidRange {
                low: (count + 1) * 2,
                high: (count + 2) * 2
            }
        );
        assert!(ranges(&[]).is_empty());
    }

    #[test]
    fn a_refusal_keeps_the_session_and_any_other_failure_ends_it() {
        assert!(stands(&SessionError::Refused));
        assert!(!stands(&SessionError::Closed));
        assert!(!stands(&SessionError::Timeout));
        assert!(!stands(&SessionError::Protocol(MESSAGE_LIMIT)));
    }
}
