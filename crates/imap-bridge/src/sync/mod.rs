// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The header sync of one folder: newest first, one fetch and one
//! committed state per batch, the progress on record so a restart
//! carries on where the last batch ended.

pub mod headers;
pub mod mapping;

use std::sync::Arc;

use crate::mailboxes::SyncError;
use crate::seal::Sealer;
use crate::session::{FetchedMessage, MAX_FETCH_MESSAGES, Session, SessionError, UidRange};
use crate::store::{AccountKey, Batch, EmailFacts, MailboxId, MailboxRow, Store};

/// The messages of one batch: one fetch, one transaction, one state.
pub const SYNC_BATCH: usize = MAX_FETCH_MESSAGES;

/// The failed fetches one run of a folder spends on narrowing. One
/// message among `SYNC_BATCH` costs ten of them: nine halvings and the
/// lone fetch with its structure. A run so passes six such messages;
/// the next run starts below them with a fresh budget.
pub const NARROWING_BUDGET: usize = 64;

/// Where a sync writes: the store, the host's sealer and the account.
#[derive(Clone)]
pub struct Cache {
    pub store: Arc<Store>,
    pub sealer: Arc<dyn Sealer>,
    pub key: AccountKey,
}

/// How a folder stands after one batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// More batches wait.
    More,
    /// Every message is in; the counts come from the memberships now.
    Done,
    /// The folder vanished or was renumbered under the sync; nothing
    /// was written and the next mailbox pass decides.
    Stale,
}

/// UIDs that follow each other in the folder's list, lowest first, so
/// the range from the first to the last names exactly them.
struct Slice {
    uids: Vec<u32>,
    structure: bool,
}

impl Slice {
    fn range(&self) -> Option<UidRange> {
        Some(UidRange {
            low: *self.uids.first()?,
            high: *self.uids.last()?,
        })
    }
}

/// The first sync of one folder between its opening and its last
/// batch. It outlives a session: after a failure the caller connects
/// again, calls [`FolderSync::resume`] and goes on.
pub struct FolderSync {
    folder: MailboxId,
    imap_name: String,
    uid_validity: u32,
    /// The UIDs no batch has taken yet, lowest first.
    remaining: Vec<u32>,
    /// What a failed fetch was narrowed to, the highest UIDs last.
    narrowed: Vec<Slice>,
    budget: usize,
}

impl FolderSync {
    /// Opens the folder read-only and lists what is left to fetch.
    /// `None` when there is nothing to do: the folder is done, holds no
    /// mail of its own, has no UIDVALIDITY on record, refuses EXAMINE
    /// or was renumbered since the mailbox pass.
    ///
    /// # Errors
    ///
    /// Returns the session's failure, after which the session must be
    /// dropped, the store's or `Task`.
    pub async fn open<S: Session>(
        session: &mut S,
        cache: &Cache,
        folder: &MailboxRow,
    ) -> Result<Option<Self>, SyncError> {
        let facts = &folder.facts;
        let Some(uid_validity) = facts
            .uid_validity
            .filter(|_| facts.selectable && facts.store)
        else {
            return Ok(None);
        };
        let (store, key, id) = (
            Arc::clone(&cache.store),
            cache.key.clone(),
            folder.id.clone(),
        );
        let progress = blocking(move || store.sync_progress(&key, &id)).await?;
        if progress.done {
            return Ok(None);
        }
        let mut sync = Self {
            folder: folder.id.clone(),
            imap_name: facts.imap_name.clone(),
            uid_validity,
            remaining: Vec::new(),
            narrowed: Vec::new(),
            budget: NARROWING_BUDGET,
        };
        if !sync.resume(session).await? {
            return Ok(None);
        }
        let mut uids = session.uid_list().await?;
        if let Some(lowest) = progress.lowest_synced_uid {
            uids.retain(|uid| *uid < lowest);
        }
        uids.reverse();
        sync.remaining = uids;
        Ok(Some(sync))
    }

    /// Selects the folder on a fresh session. `false` when the server
    /// refuses or the folder was renumbered, which ends this sync.
    ///
    /// # Errors
    ///
    /// Returns the session's failure.
    pub async fn resume<S: Session>(&mut self, session: &mut S) -> Result<bool, SyncError> {
        match session.examine(&self.imap_name).await {
            Ok(selected) => Ok(selected.uid_validity == self.uid_validity),
            Err(SessionError::Refused) => Ok(false),
            Err(other) => Err(other.into()),
        }
    }

    /// Fetches the next batch and writes it as one state.
    ///
    /// A fetch that fails on what the server sent is narrowed before
    /// the failure is returned: a slice of several messages is halved, a
    /// lone message is tried once more without BODYSTRUCTURE and left
    /// out when that fails too. One message a server cannot describe
    /// within the bounds therefore never stalls a folder.
    ///
    /// # Errors
    ///
    /// Returns the session's failure, after which the session must be
    /// dropped and a fresh one resumed, the store's or `Task`.
    pub async fn batch<S: Session>(
        &mut self,
        session: &mut S,
        cache: &Cache,
    ) -> Result<Step, SyncError> {
        let Some(slice) = self.next_slice() else {
            return self.write(cache, Vec::new(), None).await;
        };
        let Some(range) = slice.range() else {
            return Ok(Step::More);
        };
        match session.uid_fetch(range, slice.structure).await {
            Ok(messages) => self.write(cache, messages, Some(range.low)).await,
            Err(error @ SessionError::Protocol(_)) => {
                if let Some(left_out) = self.narrow(slice) {
                    self.write(cache, Vec::new(), Some(left_out)).await?;
                }
                Err(error.into())
            }
            Err(other) => {
                self.narrowed.push(slice);
                Err(other.into())
            }
        }
    }

    /// Every batch until the folder is done or stale.
    ///
    /// # Errors
    ///
    /// As [`FolderSync::batch`].
    pub async fn finish<S: Session>(
        &mut self,
        session: &mut S,
        cache: &Cache,
    ) -> Result<Step, SyncError> {
        loop {
            let step = self.batch(session, cache).await?;
            if step != Step::More {
                return Ok(step);
            }
        }
    }

    /// The narrowed slice with the highest UIDs, else the top of what
    /// remains.
    fn next_slice(&mut self) -> Option<Slice> {
        if let Some(slice) = self.narrowed.pop() {
            return Some(slice);
        }
        if self.remaining.is_empty() {
            return None;
        }
        let from = self.remaining.len().saturating_sub(SYNC_BATCH);
        Some(Slice {
            uids: self.remaining.split_off(from),
            structure: true,
        })
    }

    /// Puts back what a failed slice narrows to, the upper half last so
    /// it goes first and the progress stays one line from the top.
    /// Answers the UID of a message that is left out. With the budget
    /// spent the slice goes back whole and the failures repeat, which
    /// the caller's own retry bound ends.
    fn narrow(&mut self, mut slice: Slice) -> Option<u32> {
        let Some(budget) = self.budget.checked_sub(1) else {
            self.narrowed.push(slice);
            return None;
        };
        self.budget = budget;
        if slice.uids.len() > 1 {
            let upper = slice.uids.split_off(slice.uids.len() / 2);
            let structure = slice.structure;
            self.narrowed.push(slice);
            self.narrowed.push(Slice {
                uids: upper,
                structure,
            });
            return None;
        }
        if slice.structure {
            slice.structure = false;
            self.narrowed.push(slice);
            return None;
        }
        slice.uids.first().copied()
    }

    /// Maps and writes off the runtime; the folder is done once nothing
    /// waits.
    async fn write(
        &self,
        cache: &Cache,
        messages: Vec<FetchedMessage>,
        lowest_uid: Option<u32>,
    ) -> Result<Step, SyncError> {
        let done = self.narrowed.is_empty() && self.remaining.is_empty();
        let (cache, folder, uid_validity) = (cache.clone(), self.folder.clone(), self.uid_validity);
        let state = blocking(move || {
            let emails: Vec<EmailFacts> = messages.iter().filter_map(mapping::email).collect();
            let batch = Batch {
                folder: &folder,
                uid_validity,
                emails: &emails,
                lowest_uid,
                done,
            };
            cache
                .store
                .apply_batch(&cache.key, &batch, cache.sealer.as_ref())
        })
        .await?;
        Ok(match state {
            None => Step::Stale,
            Some(_) if done => Step::Done,
            Some(_) => Step::More,
        })
    }
}

/// Runs a store call off the runtime.
async fn blocking<T: Send + 'static>(
    call: impl FnOnce() -> Result<T, crate::store::StoreError> + Send + 'static,
) -> Result<T, SyncError> {
    tokio::task::spawn_blocking(call)
        .await
        .map_err(|_join| SyncError::Task)?
        .map_err(SyncError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sync(remaining: Vec<u32>) -> FolderSync {
        FolderSync {
            folder: MailboxId::generate(),
            imap_name: "INBOX".to_owned(),
            uid_validity: 1,
            remaining,
            narrowed: Vec::new(),
            budget: NARROWING_BUDGET,
        }
    }

    fn uids(slice: &Slice) -> (Vec<u32>, bool) {
        (slice.uids.clone(), slice.structure)
    }

    #[test]
    fn a_batch_is_the_top_of_what_remains() {
        let count = u32::try_from(SYNC_BATCH).unwrap();
        let mut sync = sync((1..=count + 3).collect());
        let first = sync.next_slice().unwrap();
        assert_eq!(
            first.range(),
            Some(UidRange {
                low: 4,
                high: count + 3
            })
        );
        assert_eq!(first.uids.len(), SYNC_BATCH);
        let rest = sync.next_slice().unwrap();
        assert_eq!(rest.range(), Some(UidRange { low: 1, high: 3 }));
        assert!(sync.next_slice().is_none());
    }

    #[test]
    fn a_failed_slice_halves_with_the_upper_half_first_down_to_one_message() {
        let mut sync = sync(vec![1, 2, 3, 4, 5]);
        let whole = sync.next_slice().unwrap();
        assert_eq!(sync.narrow(whole), None);
        let upper = sync.next_slice().unwrap();
        assert_eq!(uids(&upper), (vec![3, 4, 5], true));
        assert_eq!(sync.narrow(upper), None);
        let top = sync.next_slice().unwrap();
        assert_eq!(uids(&top), (vec![4, 5], true));
        assert_eq!(sync.narrow(top), None);
        let lone = sync.next_slice().unwrap();
        assert_eq!(uids(&lone), (vec![5], true));
        assert_eq!(sync.narrow(lone), None);
        let bare = sync.next_slice().unwrap();
        assert_eq!(uids(&bare), (vec![5], false));
        assert_eq!(sync.narrow(bare), Some(5));
        let waiting: Vec<_> = sync.narrowed.iter().map(uids).collect();
        assert_eq!(
            waiting,
            [(vec![1, 2], true), (vec![3], true), (vec![4], true)]
        );
    }

    #[test]
    fn past_the_budget_a_slice_goes_back_whole() {
        let mut sync = sync(vec![1, 2, 3, 4]);
        sync.budget = 1;
        let whole = sync.next_slice().unwrap();
        assert_eq!(sync.narrow(whole), None);
        let upper = sync.next_slice().unwrap();
        assert_eq!(sync.narrow(upper), None);
        assert_eq!(uids(&sync.next_slice().unwrap()), (vec![3, 4], true));
        assert_eq!(sync.budget, 0);
    }

    #[test]
    fn the_budget_passes_six_hostile_messages_in_one_batch() {
        let per_message = SYNC_BATCH.ilog2() as usize + 2;
        assert!(6 * per_message <= NARROWING_BUDGET);
        assert!(7 * per_message > NARROWING_BUDGET);
    }
}
