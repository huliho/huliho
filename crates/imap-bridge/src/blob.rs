// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A blob of an account (RFC 8620 section 6.2): the whole message under
//! its email id, one part under the email id, a hyphen and the part
//! number with underscores for its dots. The bytes come from the server
//! in windows, one window per turn on the account's conversation: a
//! message as it lies there, a part with its transfer encoding undone.
//! Nothing is stored.

mod address;
mod decode;
#[cfg(feature = "test-support")]
pub mod fuzzing;

use std::sync::{Arc, Weak};

use thiserror::Error;
use tokio::sync::mpsc;
use tokio::time::timeout;

use address::Address;
pub(crate) use address::part_blob_id;
use decode::Decoder;

use crate::jmap::leaf_of;
use crate::mailboxes::SyncError;
use crate::runtime::{Connector, Link};
use crate::session::{BODY_WINDOW_BYTES, PartAsk, PartWindow, Session, SessionError};
use crate::store::StoreError;
use crate::sync::{Cache, blocking};

/// The windows a blob stream reads ahead of whoever reads it: the last
/// of them waits in the stream's hand. A slow reader holds these beside
/// the first window, which waits in the blob. It never holds the
/// conversation.
pub const BLOB_BUFFER_WINDOWS: usize = 4;

/// What a stream hands on: bytes, `None` once the blob ended whole or
/// the error that broke it off.
pub type Step = Result<Option<Vec<u8>>, BlobError>;

/// Why a blob did not come or stopped coming.
#[derive(Debug, Error)]
pub enum BlobError {
    /// An id the bridge never handed out, a message that left or a part
    /// its message lacks.
    #[error("the account holds no such blob")]
    NotFound,
    /// The server states more octets than the caller carries or sent
    /// more than that.
    #[error("the blob is larger than the caller carries")]
    TooLarge,
    /// No session, an answer that failed or ran past the deadline, a
    /// folder renumbered since the sync or an account the runtime let
    /// go of.
    #[error("the server could not be read")]
    Unavailable,
    /// The decoder gave the bytes of a part up. The way they are handed
    /// to it leaves it no cause for that, so this names a fault of the
    /// bridge, never one of the mail.
    #[error("the part could not be decoded")]
    Undecodable,
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("the task reading the blob ended early")]
    Task,
}

impl From<SyncError> for BlobError {
    fn from(error: SyncError) -> Self {
        match error {
            SyncError::Store(error) => Self::Store(error),
            SyncError::Task => Self::Task,
            SyncError::Session(_) => Self::Unavailable,
        }
    }
}

/// A blob on its way, in the order of its bytes. It has no `Debug`
/// form, since its bytes are a sender's.
pub struct Blob {
    first: Option<Vec<u8>>,
    rest: Option<mpsc::Receiver<Step>>,
}

impl Blob {
    /// The next bytes; `None` at the end of the blob. An error is the
    /// last thing a blob answers: the bytes ahead of it are not the
    /// whole blob.
    pub async fn next(&mut self) -> Option<Result<Vec<u8>, BlobError>> {
        if let Some(first) = self.first.take() {
            return Some(Ok(first));
        }
        // A stream that stops without its closing step was cut short.
        let step = self.rest.as_mut()?.recv().await;
        let step = step.unwrap_or(Err(BlobError::Task));
        if !matches!(step, Ok(Some(_))) {
            self.rest = None;
        }
        step.transpose()
    }

    /// A blob that answers what the channel carries, for a host that
    /// tests its own reader of a blob.
    #[cfg(feature = "test-support")]
    #[must_use]
    pub fn scripted(rest: mpsc::Receiver<Step>) -> Self {
        Self {
            first: None,
            rest: Some(rest),
        }
    }
}

/// Opens a blob of the account: its first window read, the rest read
/// ahead of the caller on a task of its own, `BLOB_BUFFER_WINDOWS` at
/// most. `limit` bounds the octets read from the server; a blob the
/// server states larger is refused before a window is read.
///
/// # Errors
///
/// Returns `NotFound` for an id that names no blob of the account,
/// `TooLarge` past the limit, `Unavailable` when the server cannot be
/// read, `Undecodable` for a window its decoder gives up and the
/// store's failure or `Task`.
pub async fn open<C: Connector + 'static>(
    cache: &Cache,
    link: &Arc<Link<C>>,
    blob_id: &str,
    limit: u64,
) -> Result<Blob, BlobError> {
    let address = Address::parse(blob_id).ok_or(BlobError::NotFound)?;
    let mut reader = Reader::placed(cache, &address, limit).await?;
    let (first, last) = reader.chunk(link, address.part.as_deref()).await?;
    let rest = if last {
        None
    } else {
        let (sender, receiver) = mpsc::channel(BLOB_BUFFER_WINDOWS - 1);
        tokio::spawn(stream(reader, Arc::downgrade(link), sender));
        Some(receiver)
    };
    Ok(Blob {
        first: Some(first).filter(|content| !content.is_empty()),
        rest,
    })
}

/// Why a turn ended without its window and whether the session still
/// stands.
struct Stop {
    error: BlobError,
    stands: bool,
}

impl Stop {
    /// The session answered whole; the next turn may use it.
    fn kept(error: BlobError) -> Self {
        Self {
            error,
            stands: true,
        }
    }

    /// The stream may hold unread lines, so the session goes.
    fn dropped(error: BlobError) -> Self {
        Self {
            error,
            stands: false,
        }
    }
}

/// A NO leaves the session standing; any other failure may leave lines
/// unread.
impl From<SessionError> for Stop {
    fn from(error: SessionError) -> Self {
        Self {
            error: BlobError::Unavailable,
            stands: matches!(error, SessionError::Refused),
        }
    }
}

/// One blob while its windows are read: where its message lies, which
/// section, how far the read stands and what undoes its encoding.
struct Reader {
    cache: Cache,
    /// The folder by its wire name.
    folder: String,
    /// The UIDVALIDITY the sync stored the UID under.
    uid_validity: Option<u32>,
    uid: u32,
    /// Empty for the whole message.
    section: String,
    offset: u32,
    limit: u64,
    decoder: Decoder,
}

impl Reader {
    /// The reader of a blob the store can place: an email the account
    /// does not hold is not found, a whole message past the limit too
    /// large and a row or a folder without a place unavailable.
    async fn placed(cache: &Cache, address: &Address, limit: u64) -> Result<Self, BlobError> {
        let (store, key) = (Arc::clone(&cache.store), cache.key.clone());
        let email = address.email.clone();
        let found = blocking(move || {
            let Some(row) = store.emails(&key, &[&email])?.rows.pop() else {
                return Ok(None);
            };
            let located = store.locate(&key, &[&email])?.pop();
            let folders = store.mailbox_snapshot(&key)?.rows;
            let place = located.and_then(|located| {
                let folder = folders
                    .into_iter()
                    .find(|folder| folder.id == located.folder)?;
                Some((
                    folder.facts.imap_name,
                    folder.facts.uid_validity,
                    located.uid,
                ))
            });
            Ok(Some((row.size, place)))
        })
        .await?;
        let (size, place) = found.ok_or(BlobError::NotFound)?;
        // The row states the size of a whole message, so one past the
        // limit costs no command.
        if address.part.is_none() && u64::from(size) > limit {
            return Err(BlobError::TooLarge);
        }
        let (folder, uid_validity, uid) = place.ok_or(BlobError::Unavailable)?;
        Ok(Self {
            cache: cache.clone(),
            folder,
            uid_validity,
            uid,
            section: String::new(),
            offset: 0,
            limit,
            decoder: Decoder::identity(),
        })
    }

    /// The next window, read on one turn and decoded off the runtime:
    /// its content and whether it ended the blob. `part` names the part
    /// the first window of a part blob still has to find.
    async fn chunk<C: Connector>(
        &mut self,
        link: &Link<C>,
        part: Option<&str>,
    ) -> Result<(Vec<u8>, bool), BlobError> {
        let window = self.turn(link, part).await?;
        let read = u32::try_from(window.len()).unwrap_or(u32::MAX);
        // RFC 3501 section 6.4.5: a fetch past the end is cut short.
        let last = read < BODY_WINDOW_BYTES;
        self.offset = self
            .offset
            .checked_add(read)
            .filter(|offset| u64::from(*offset) <= self.limit)
            .ok_or(BlobError::TooLarge)?;
        if self.decoder.is_identity() {
            return Ok((window, last));
        }
        let mut decoder = std::mem::replace(&mut self.decoder, Decoder::identity());
        let (decoder, content) = tokio::task::spawn_blocking(move || {
            let content = decoder.push(window, last);
            (decoder, content)
        })
        .await
        .map_err(|_join| BlobError::Task)?;
        self.decoder = decoder;
        Ok((content.ok_or(BlobError::Undecodable)?, last))
    }

    /// One turn on the conversation within its deadline; a failure that
    /// may leave lines unread costs the session.
    async fn turn<C: Connector>(
        &mut self,
        link: &Link<C>,
        part: Option<&str>,
    ) -> Result<Vec<u8>, BlobError> {
        let mut wire = link.wire.lock().await;
        let deadline = wire.deadline();
        let work = async {
            let session = wire
                .session(&self.cache)
                .await
                .map_err(|_| Stop::kept(BlobError::Unavailable))?;
            self.read(session, part).await
        };
        let outcome = timeout(deadline, work)
            .await
            .unwrap_or_else(|_elapsed| Err(Stop::dropped(BlobError::Unavailable)));
        outcome.map_err(|stop| {
            if !stop.stands {
                wire.drop_session();
            }
            stop.error
        })
    }

    /// The folder examined afresh, since the conversation may have
    /// selected another one in between; then the part found where one
    /// is named and the window at the offset.
    async fn read<S: Session>(
        &mut self,
        session: &mut S,
        part: Option<&str>,
    ) -> Result<Vec<u8>, Stop> {
        let chosen = session.examine(&self.folder).await?;
        // Renumbered since the sync, the UID names another message.
        if Some(chosen.uid_validity) != self.uid_validity {
            return Err(Stop::kept(BlobError::Unavailable));
        }
        if let Some(part_id) = part {
            self.find(session, part_id).await?;
        }
        let ask = PartAsk {
            uid: self.uid,
            section: &self.section,
            window: PartWindow {
                offset: self.offset,
                bytes: BODY_WINDOW_BYTES,
            },
        };
        // No line for the UID: the message left since the sync.
        session
            .uid_part(&ask)
            .await?
            .ok_or(Stop::kept(BlobError::NotFound))
    }

    /// The section and the encoding of a part from the structure of its
    /// message; a part the server states past the limit is refused
    /// before a window is read.
    async fn find<S: Session>(&mut self, session: &mut S, part_id: &str) -> Result<(), Stop> {
        let structure = match session.uid_structure(self.uid, &[]).await {
            Ok(structure) => structure.ok_or(Stop::kept(BlobError::NotFound))?,
            // An answer the bridge cannot read is the message's own
            // shape: it has no part the bridge describes.
            Err(SessionError::Protocol(_)) => return Err(Stop::dropped(BlobError::NotFound)),
            Err(error) => return Err(error.into()),
        };
        let (leaf, section) = structure
            .tree
            .as_ref()
            .and_then(|tree| leaf_of(tree, part_id))
            .ok_or(Stop::kept(BlobError::NotFound))?;
        if u64::from(leaf.bytes) > self.limit {
            return Err(Stop::kept(BlobError::TooLarge));
        }
        self.decoder = Decoder::of(&leaf.encoding);
        self.section = section;
        Ok(())
    }
}

/// Reads the windows after the first, each on a turn of its own, until
/// the blob ends, nobody reads the stream or a read fails. The closing
/// step or an error is the last thing sent.
async fn stream<C: Connector>(mut reader: Reader, link: Weak<Link<C>>, sender: mpsc::Sender<Step>) {
    while !sender.is_closed() {
        // An account the runtime let go of has no conversation left.
        let read = match link.upgrade() {
            Some(link) => reader.chunk(&link, None).await,
            None => Err(BlobError::Unavailable),
        };
        let (content, last) = match read {
            Ok(read) => read,
            Err(error) => {
                // Nobody reading is how a stream ends as well.
                let _unread = sender.send(Err(error)).await;
                return;
            }
        };
        if !content.is_empty() && sender.send(Ok(Some(content))).await.is_err() {
            return;
        }
        if last {
            let _unread = sender.send(Ok(None)).await;
            return;
        }
    }
}
