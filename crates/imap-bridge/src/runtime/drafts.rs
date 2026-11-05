// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A draft between the two passes of its `Email/set`, on the account's
//! conversation under the lock and the deadline: every blob it names read
//! whole, the message built off the runtime and bounded before a byte is
//! sent, then what its row holds. Where the server names no UID a store
//! can hold, one pass before the answer lands the row by the Message-ID
//! the bridge wrote.

use std::collections::BTreeMap;
use std::sync::Arc;

use tokio::time::{Instant, timeout};

use super::{Connector, Wire};
use crate::blob::{self, BlobError};
use crate::gmail::ALL_MAIL_ROLE;
use crate::mailboxes::SyncError;
use crate::message::{Blobs, Draft, MAX_MESSAGE_BYTES, Stamp, build, weight};
use crate::seal::Sealer;
use crate::session::{Appended, Session, SessionError};
use crate::store::{
    AccountKey, DraftRow, EmailFacts, EmailId, MailboxId, MailboxRow, Personal, StoreError,
};
use crate::sync::refresh::Given;
use crate::sync::{Cache, FolderSync};

/// The UIDs read at the top of a folder beyond one per draft, for mail that
/// arrived beside them.
const ROOM_BESIDE_DRAFTS: u32 = 8;

/// Why a draft did not reach the wire, beside whether the session still
/// stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Failure {
    /// Every blob id no part of the account answers to.
    BlobNotFound(Vec<String>),
    TooLarge,
    ServerFail,
    ServerUnavailable,
}

/// A draft that stopped before its command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Stopped {
    pub failure: Failure,
    pub stands: bool,
}

/// A message ready for the wire and what its row holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Built {
    pub message: Vec<u8>,
    pub flags: Vec<String>,
    pub unsealed: Unsealed,
}

/// What a row of the draft holds before the host seals it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Unsealed {
    pub id: EmailId,
    keywords: BTreeMap<String, bool>,
    size: u32,
    date: i64,
    has_attachment: bool,
    personal: Personal,
    /// The keyed hash of its Message-ID, which a pass lands the row by.
    pub hash: Vec<u8>,
    named: Vec<Vec<u8>>,
}

/// What the row of a copy holds otherwise than the message's: the
/// keywords the copy is stored with, its size and the moment of the send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CopyFacts {
    pub keywords: BTreeMap<String, bool>,
    pub size: u32,
    pub date: i64,
}

impl Unsealed {
    /// The row of a copy of a stored message under an id of its own: the
    /// fields the message's row holds, with the facts of the copy. It
    /// lands by the Message-ID it keeps.
    pub(crate) fn copied(
        personal: Personal,
        has_attachment: bool,
        facts: CopyFacts,
        sealer: &dyn Sealer,
    ) -> Self {
        let first = personal.message_id.as_deref().and_then(<[String]>::first);
        let hash = first.map_or_else(Vec::new, |id| sealer.keyed_hash(id.as_bytes()));
        let named = named_hashes(&personal, sealer);
        Self {
            id: EmailId::generate(),
            keywords: facts.keywords,
            size: facts.size,
            date: facts.date,
            has_attachment,
            personal,
            hash,
            named,
        }
    }

    /// The row of the draft under what `APPENDUID` named.
    ///
    /// # Errors
    ///
    /// Returns the host's failure to seal or the encoding's.
    pub(crate) fn sealed(
        self,
        (folder, appended): (MailboxId, Appended),
        sealer: &dyn Sealer,
        key: &AccountKey,
    ) -> Result<DraftRow, StoreError> {
        let plain = serde_json::to_vec(&self.personal)?;
        let blob = sealer.seal(key, &self.id, &plain)?;
        Ok(DraftRow {
            id: self.id,
            folder,
            uid_validity: appended.uid_validity,
            facts: EmailFacts {
                uid: appended.uid,
                keywords: self.keywords,
                size: self.size,
                received_at: self.date,
                sent_at: Some(self.date),
                has_attachment: self.has_attachment,
                gmail: None,
                personal: self.personal,
            },
            hash: self.hash,
            named: self.named,
            sealed: blob,
        })
    }
}

/// The message of a draft within what the bridge and the server take. Past
/// either, nothing is sent. The blobs are read before the first write of
/// the draft, so one of a message it replaces still stands.
///
/// # Errors
///
/// Returns why the draft stopped.
pub(crate) async fn built<S: Session>(
    session: &mut S,
    cache: &Cache,
    draft: &Draft,
    server_limit: Option<u64>,
) -> Result<Built, Stopped> {
    let limit = bound(server_limit);
    let blobs = read_blobs(session, cache, draft, limit).await?;
    let (owned, sealer) = (draft.clone(), Arc::clone(&cache.sealer));
    let built =
        tokio::task::spawn_blocking(move || assembled(&owned, &blobs, sealer.as_ref(), limit));
    let stopped = |failure| Stopped {
        failure,
        stands: true,
    };
    built
        .await
        .map_err(|_| stopped(Failure::ServerFail))?
        .map_err(stopped)
}

/// What a message may weigh: the bridge's bound, or the server's where it
/// names less (RFC 7889 section 3).
fn bound(server_limit: Option<u64>) -> u64 {
    let most = u64::try_from(MAX_MESSAGE_BYTES).unwrap_or(u64::MAX);
    server_limit.map_or(most, |limit| limit.min(most))
}

/// Every blob the tree names, each read whole; the ids the account does
/// not hold are named together. What the blobs may weigh shrinks as they
/// come in, so a draft past the bound fetches no more.
async fn read_blobs<S: Session>(
    session: &mut S,
    cache: &Cache,
    draft: &Draft,
    limit: u64,
) -> Result<Blobs, Stopped> {
    let mut blobs = Blobs::new();
    let mut missing = Vec::new();
    let mut left = limit;
    for id in draft.blob_ids() {
        match blob::whole(session, cache, id, left).await {
            Ok(bytes) => {
                let weight = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
                left = left.saturating_sub(weight);
                blobs.insert(id.to_owned(), bytes);
            }
            Err(stop) => {
                if matches!(stop.error, BlobError::NotFound) && stop.stands {
                    missing.push(id.to_owned());
                    continue;
                }
                let failure = match stop.error {
                    BlobError::NotFound => Failure::BlobNotFound(vec![id.to_owned()]),
                    BlobError::TooLarge => Failure::TooLarge,
                    BlobError::Unavailable => Failure::ServerUnavailable,
                    BlobError::Undecodable | BlobError::Store(_) | BlobError::Task => {
                        Failure::ServerFail
                    }
                };
                let stands = stop.stands;
                return Err(Stopped { failure, stands });
            }
        }
    }
    if missing.is_empty() {
        Ok(blobs)
    } else {
        Err(Stopped {
            failure: Failure::BlobNotFound(missing),
            stands: true,
        })
    }
}

/// The message and its row off the runtime. A tree past the limit is not
/// built at all, since the writer reserves what the tree asks up front; the
/// message is measured once more, since its encoding adds to it. A writer
/// that fails is the server's failure, which a `Vec` never gives.
fn assembled(
    draft: &Draft,
    blobs: &Blobs,
    sealer: &dyn Sealer,
    limit: u64,
) -> Result<Built, Failure> {
    if weight(&draft.body, blobs) > limit {
        return Err(Failure::TooLarge);
    }
    let stamp = Stamp::now(draft);
    let hash = sealer.keyed_hash(stamp.message_id.as_bytes());
    let message = build(draft, &stamp, blobs).map_err(|_| Failure::ServerFail)?;
    let size = u32::try_from(message.len()).map_err(|_| Failure::TooLarge)?;
    if u64::from(size) > limit {
        return Err(Failure::TooLarge);
    }
    let personal = draft.stamped(&stamp);
    let named = named_hashes(&personal, sealer);
    Ok(Built {
        message,
        flags: draft.flags(),
        unsealed: Unsealed {
            id: EmailId::generate(),
            keywords: draft.keyword_map(),
            size,
            date: stamp.date,
            has_attachment: draft.has_attachment(),
            personal,
            hash,
            named,
        },
    })
}

/// The keyed hash of every message id the header names, each once, which
/// a row threads by.
fn named_hashes(personal: &Personal, sealer: &dyn Sealer) -> Vec<Vec<u8>> {
    let mut named: Vec<Vec<u8>> = Vec::new();
    for id in personal.named_ids() {
        let id_hash = sealer.keyed_hash(id.as_bytes());
        if !named.contains(&id_hash) {
            named.push(id_hash);
        }
    }
    named
}

/// One pass of the account within what is left of the deadline, so the
/// rows hold what the server took. Of the stores it leaves to their first
/// sync, those that hold the drafts have their newest mail read as well. A
/// pass that cannot run or breaks off leaves the rows as far as it came; the
/// session goes with it.
///
/// # Errors
///
/// Returns the store's failure or `Task`.
pub(crate) async fn pass<C: Connector>(
    wire: &mut Wire<C>,
    cache: &Cache,
    (until, targets): (Option<Instant>, &[MailboxId]),
) -> Result<(), SyncError> {
    let left = until.map_or(wire.deadline(), |until| {
        until.saturating_duration_since(Instant::now())
    });
    let gmail = wire.gmail;
    let run = async {
        if wire.session(cache).await.is_err() {
            return Ok(false);
        }
        let given = Given {
            condstore: wire.condstore,
            gmail: wire.gmail,
            viewed: None,
            first_pass: std::mem::take(&mut wire.unpassed),
        };
        let Wire {
            session, refresher, ..
        } = &mut *wire;
        let Some(session) = session.as_mut() else {
            return Ok(false);
        };
        let refreshed = refresher.refresh(session, cache, given).await?;
        if !refreshed.stands {
            return Ok(false);
        }
        newest(session, cache, &refreshed.waiting, (targets, gmail)).await
    };
    match timeout(left, run).await {
        Ok(Ok(true)) => Ok(()),
        Ok(Ok(false)) | Err(_) => {
            wire.drop_session();
            Ok(())
        }
        Ok(Err(error)) => Err(error),
    }
}

/// The newest mail of the waiting stores that hold a draft: the mailbox it
/// went to and on Gmail All Mail, where a label's mail lies. A read moves
/// no progress, so a first sync that ends on another connection meanwhile
/// keeps what it recorded. That sync skips a row that stands. Whether the
/// session still stands.
async fn newest<S: Session>(
    session: &mut S,
    cache: &Cache,
    waiting: &[MailboxRow],
    (targets, gmail): (&[MailboxId], bool),
) -> Result<bool, SyncError> {
    let holds_draft = |row: &&MailboxRow| {
        targets.contains(&row.id) || (gmail && row.facts.role.as_deref() == Some(ALL_MAIL_ROLE))
    };
    let room = u32::try_from(targets.len()).unwrap_or(u32::MAX);
    let room = room.saturating_add(ROOM_BESIDE_DRAFTS);
    for row in waiting.iter().filter(holds_draft) {
        match newest_of(session, cache, row, (room, gmail)).await {
            Ok(()) | Err(SyncError::Session(SessionError::Refused)) => {}
            Err(SyncError::Session(_)) => return Ok(false),
            Err(other) => return Err(other),
        }
    }
    Ok(true)
}

/// The UIDs at the top of one folder.
async fn newest_of<S: Session>(
    session: &mut S,
    cache: &Cache,
    row: &MailboxRow,
    (room, gmail): (u32, bool),
) -> Result<(), SyncError> {
    let selected = session.examine(&row.facts.imap_name).await?;
    let unchanged = Some(selected.uid_validity) == row.facts.uid_validity;
    let Some(next) = selected.uid_next.filter(|_| unchanged) else {
        return Ok(());
    };
    let range = (next.saturating_sub(room), next.saturating_sub(1));
    if let Some(sync) = FolderSync::above(session, row, range, gmail).await? {
        sync.beside().finish(session, cache).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
