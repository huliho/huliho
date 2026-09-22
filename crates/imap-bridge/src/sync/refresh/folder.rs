// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! One folder inside a refresh: the flags that changed, the messages
//! that left and how far the folder stands, each written while the
//! folder stands as the pass read it.

use std::collections::HashSet;
use std::sync::Arc;

use super::Run;
use crate::mailboxes::SyncError;
use crate::session::{FlagFetch, Flagged, MESSAGE_LIMIT, Session, SessionError, UidRange};
use crate::store::{FlagChange, MailboxId, MailboxRow, Progress, Standing, Synced};
use crate::sync::{Cache, SYNC_BATCH, blocking, mapping};

/// One folder inside a refresh, as the pass and the store had it.
#[derive(Clone, Copy)]
pub(super) struct Refreshing<'a> {
    pub(super) row: &'a MailboxRow,
    pub(super) uid_validity: u32,
    pub(super) progress: Progress,
    /// UIDNEXT as the pass of this refresh read it.
    pub(super) server_next: u32,
    /// Whether the labels ride both flag fetches and a row whose UID left
    /// waits for the pass instead of leaving, since another store of the
    /// account may hold its message.
    pub(super) gmail: bool,
}

impl Refreshing<'_> {
    /// The folder and the UIDVALIDITY as owned values, for a store call
    /// off the runtime.
    fn owned(&self) -> (MailboxId, u32) {
        (self.row.id.clone(), self.uid_validity)
    }

    pub(super) async fn stored(&self, cache: &Cache) -> Result<Vec<u32>, SyncError> {
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
    /// answer passed its bound. On Gmail the labels ride along.
    pub(super) async fn flags<S: Session>(
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
        let fetch = FlagFetch::ChangedSince(since);
        match session.uid_flags(fetch, self.gmail).await {
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
            let flagged = session
                .uid_flags(FlagFetch::Range(range), self.gmail)
                .await?;
            self.apply(cache, &flagged).await?;
        }
        Ok(())
    }

    /// Writes what the flags say in transactions of `SYNC_BATCH`: a
    /// message that gained `\Deleted` leaves, the keywords and the
    /// labels of the rest are compared to the rows.
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
                labels: found.labels.as_deref().map(mapping::labels),
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
    pub(super) async fn expunged<S: Session>(
        &self,
        session: &mut S,
        cache: &Cache,
    ) -> Result<(), SyncError> {
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

    pub(super) async fn advance(&self, cache: &Cache, synced: Synced) -> Result<(), SyncError> {
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

    /// One write off the runtime. A message that left a Gmail store is
    /// parked for the pass rather than destroyed.
    async fn write(&self, cache: &Cache, write: Write) -> Result<(), SyncError> {
        let (store, key, (folder, uid_validity)) =
            (Arc::clone(&cache.store), cache.key.clone(), self.owned());
        let gmail = self.gmail;
        blocking(move || {
            let standing = Standing {
                folder: &folder,
                uid_validity,
            };
            match write {
                Write::Remove(uids) if gmail => store.park_uids(&key, &standing, &uids).map(|_| ()),
                Write::Remove(uids) => store.remove_uids(&key, &standing, &uids).map(|_| ()),
                Write::Flags(changes) => store.apply_flags(&key, &standing, &changes).map(|_| ()),
            }
        })
        .await
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
}
