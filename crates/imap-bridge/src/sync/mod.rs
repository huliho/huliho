// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The header sync of one folder: newest first, one fetch and one
//! committed state per batch, the progress on record so a restart
//! carries on where the last batch ended. The new mail of a folder that
//! is done takes the same walk upward.

pub mod headers;
pub mod mapping;
pub mod preview;
pub mod refresh;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use crate::gmail;
use crate::mailboxes::SyncError;
use crate::seal::Sealer;
use crate::session::{
    FetchItems, FetchedMessage, MAX_FETCH_MESSAGES, Session, SessionError, UidRange,
};
use crate::store::{AccountKey, Advance, Batch, EmailFacts, MailboxId, MailboxRow, Store, Synced};

/// The messages of one batch: one fetch, one transaction, one state.
pub const SYNC_BATCH: usize = MAX_FETCH_MESSAGES;

/// The failed fetches one run of a folder spends on narrowing. One
/// message among `SYNC_BATCH` costs ten of them: nine halvings and the
/// lone fetch with its structure. A run so passes six such messages;
/// the next run starts below them with a fresh budget.
pub const NARROWING_BUDGET: usize = 64;

/// The UIDs between the recorded UIDNEXT and the server's that a walk
/// upward asks for range by range; a wider gap takes the UID list, so a
/// server that jumps its UIDs buys no commands.
pub const REFRESH_GAP: u32 = 10_000;

/// Where a sync writes: the store, the host's sealer and the account.
#[derive(Clone)]
pub struct Cache {
    pub store: Arc<Store>,
    pub sealer: Arc<dyn Sealer>,
    pub key: AccountKey,
    /// The host's word that the account is a Gmail account; it holds
    /// once the server advertises `X-GM-EXT-1`.
    pub gmail: bool,
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

/// Which way a sync walks the UIDs of its folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Walk {
    /// The first sync, newest first, with the server's values of the
    /// opening.
    Down { synced: Synced },
    /// The new mail of a folder that is done, oldest first, so every
    /// batch moves the recorded UIDNEXT.
    Up,
}

/// UIDs lowest first, so the range from the first to the last names
/// them and nothing a batch before it took.
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

/// The sync of one folder between its opening and its last batch. It
/// outlives a session: after a failure the caller connects again, calls
/// [`FolderSync::resume`] and goes on.
pub struct FolderSync {
    folder: MailboxId,
    imap_name: String,
    uid_validity: u32,
    walk: Walk,
    /// Whether the Gmail items ride every fetch.
    gmail: bool,
    /// The UIDs no batch has taken yet, the next ones last.
    remaining: Vec<u32>,
    /// What a failed fetch was narrowed to, the next slice last.
    narrowed: Vec<Slice>,
    budget: usize,
    /// The highest UID a walk upward covers.
    top: u32,
    answered: u32,
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
        let Some(mut sync) = Self::of(
            folder,
            Walk::Down {
                synced: Synced::default(),
            },
        ) else {
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
        if cache.gmail {
            sync.gmail = gmail::confirmed(true, &session.capabilities().await?);
        }
        let selected = match session.examine(&sync.imap_name).await {
            Ok(selected) if selected.uid_validity == sync.uid_validity => selected,
            Ok(_) | Err(SessionError::Refused) => return Ok(None),
            Err(other) => return Err(other.into()),
        };
        sync.walk = Walk::Down {
            synced: Synced {
                uid_next: selected.uid_next,
                highest_modseq: selected.highest_modseq,
                messages: Some(selected.messages),
            },
        };
        let mut uids = session.uid_list().await?;
        if let Some(lowest) = progress.lowest_synced_uid {
            uids.retain(|uid| *uid < lowest);
        }
        uids.reverse();
        sync.remaining = uids;
        Ok(Some(sync))
    }

    /// The new mail of a folder that is done, on a session with the
    /// folder selected: the UIDs `from` up to and including `through`,
    /// the Gmail items asked when `gmail` holds for the session. `None`
    /// when nothing lies in between or the folder is no store.
    ///
    /// # Errors
    ///
    /// As [`FolderSync::open`].
    pub async fn above<S: Session>(
        session: &mut S,
        folder: &MailboxRow,
        (from, through): (u32, u32),
        gmail: bool,
    ) -> Result<Option<Self>, SyncError> {
        let from = from.max(1);
        let Some(mut sync) = Self::of(folder, Walk::Up).filter(|_| from <= through) else {
            return Ok(None);
        };
        sync.gmail = gmail;
        sync.top = through;
        sync.remaining = if through - from < REFRESH_GAP {
            (from..=through).rev().collect()
        } else {
            let mut uids = session.uid_list().await?;
            uids.retain(|uid| (from..=through).contains(uid));
            uids
        };
        Ok(Some(sync))
    }

    /// Widens a walk upward to `through`. `false` when the gap is wider
    /// than a walk asks for range by range, so the caller starts afresh.
    pub fn extend_to(&mut self, through: u32) -> bool {
        if self.walk != Walk::Up || through <= self.top {
            return true;
        }
        if through - self.top >= REFRESH_GAP {
            return false;
        }
        let more: Vec<u32> = (self.top + 1..=through).rev().collect();
        self.remaining.splice(0..0, more);
        self.top = through;
        true
    }

    fn of(folder: &MailboxRow, walk: Walk) -> Option<Self> {
        let facts = &folder.facts;
        let uid_validity = facts
            .uid_validity
            .filter(|_| facts.selectable && facts.store)?;
        Some(Self {
            folder: folder.id.clone(),
            imap_name: facts.imap_name.clone(),
            uid_validity,
            walk,
            gmail: false,
            remaining: Vec::new(),
            narrowed: Vec::new(),
            budget: NARROWING_BUDGET,
            top: 0,
            answered: 0,
        })
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

    /// The messages the server answered so far, the ones flagged
    /// `\Deleted` included, which no row stands for.
    #[must_use]
    pub fn answered(&self) -> u32 {
        self.answered
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
        let items = FetchItems {
            structure: slice.structure,
            gmail: self.gmail,
        };
        match session.uid_fetch(range, items).await {
            Ok(messages) => {
                let count = u32::try_from(messages.len()).unwrap_or(u32::MAX);
                self.answered = self.answered.saturating_add(count);
                self.write(cache, messages, Some(range)).await
            }
            Err(error @ SessionError::Protocol(_)) => {
                if let Some(uid) = self.narrow(slice) {
                    let left_out = UidRange {
                        low: uid,
                        high: uid,
                    };
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

    /// The narrowed slice that goes next, else the next `SYNC_BATCH`
    /// UIDs of what remains.
    fn next_slice(&mut self) -> Option<Slice> {
        if let Some(slice) = self.narrowed.pop() {
            return Some(slice);
        }
        if self.remaining.is_empty() {
            return None;
        }
        let from = self.remaining.len().saturating_sub(SYNC_BATCH);
        let mut uids = self.remaining.split_off(from);
        if self.walk == Walk::Up {
            uids.reverse();
        }
        Some(Slice {
            uids,
            structure: true,
        })
    }

    /// Puts back what a failed slice narrows to. The half the walk
    /// reaches first goes last, so it is fetched first and the progress
    /// stays one line. Answers the UID of a message that is left out.
    /// With the budget spent the slice goes back whole and the failures
    /// repeat, which the caller's own retry bound ends.
    fn narrow(&mut self, mut slice: Slice) -> Option<u32> {
        let Some(budget) = self.budget.checked_sub(1) else {
            self.narrowed.push(slice);
            return None;
        };
        self.budget = budget;
        if slice.uids.len() > 1 {
            let structure = slice.structure;
            let upper = Slice {
                uids: slice.uids.split_off(slice.uids.len() / 2),
                structure,
            };
            let (first, second) = match self.walk {
                Walk::Down { .. } => (upper, slice),
                Walk::Up => (slice, upper),
            };
            self.narrowed.push(second);
            self.narrowed.push(first);
            return None;
        }
        if slice.structure {
            slice.structure = false;
            self.narrowed.push(slice);
            return None;
        }
        slice.uids.first().copied()
    }

    /// Maps and writes off the runtime. A walk down is done once
    /// nothing waits, which one last write without messages records; a
    /// walk up has nothing to record then.
    async fn write(
        &self,
        cache: &Cache,
        messages: Vec<FetchedMessage>,
        passed: Option<UidRange>,
    ) -> Result<Step, SyncError> {
        let done = self.narrowed.is_empty() && self.remaining.is_empty();
        let advance = match (self.walk, passed) {
            (Walk::Down { synced }, passed) => Advance::Down {
                lowest_uid: passed.map(|range| range.low),
                done,
                synced,
            },
            (Walk::Up, Some(range)) => Advance::Up {
                uid_next: range.high.saturating_add(1),
                arrived: u32::try_from(messages.len()).unwrap_or(u32::MAX),
            },
            (Walk::Up, None) => return Ok(Step::Done),
        };
        let (cache, folder, uid_validity) = (cache.clone(), self.folder.clone(), self.uid_validity);
        let state = blocking(move || {
            let emails: Vec<EmailFacts> = messages.iter().filter_map(mapping::email).collect();
            let batch = Batch {
                folder: &folder,
                uid_validity,
                emails: &emails,
                advance,
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
pub(crate) async fn blocking<T: Send + 'static>(
    call: impl FnOnce() -> Result<T, crate::store::StoreError> + Send + 'static,
) -> Result<T, SyncError> {
    tokio::task::spawn_blocking(call)
        .await
        .map_err(|_join| SyncError::Task)?
        .map_err(SyncError::from)
}
