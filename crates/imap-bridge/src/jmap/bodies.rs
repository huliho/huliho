// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The bodies an `Email/get` asked for, fetched from the server between
//! two passes over the request: per message the structure with the
//! header fields named, then every wanted text part in windows, on the
//! account's conversation under the lock and the deadline; the values
//! decode off the runtime once the conversation is given back. Nothing
//! is stored; the next call asks again.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Arc;

use tokio::time::timeout;

use super::body;
use super::values::{self, BodyValue, ValueAsk};
use crate::mailboxes::SyncError;
use crate::runtime::{Connector, Link};
use crate::session::{
    BODY_WINDOW_BYTES, BodyPart, Leaf, PartAsk, PartWindow, Session, SessionError,
};
use crate::store::{EmailId, MailboxId, MailboxRow, StoreError};
use crate::sync::{Cache, blocking};

/// The fresh sessions one fill may open after a message the server
/// cannot describe ended the one before, so one such message never
/// costs its neighbors their bodies.
const BODY_RECONNECTS: usize = 2;

/// What one call wants of its messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct BodyAsk {
    pub ids: Vec<EmailId>,
    /// The header fields to fetch, as the FETCH names them.
    pub fields: Vec<String>,
    pub values: ValueAsk,
    /// Whether the structure is wanted at all.
    pub structure: bool,
}

/// What the server holds of one message; `V` is a value as fetched or
/// as decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Body<V = BodyValue> {
    /// `None` for a message the server could not describe within the
    /// bounds; also for one whose structure was not asked.
    pub tree: Option<BodyPart>,
    pub header: Vec<u8>,
    /// The values by part number.
    pub values: BTreeMap<String, V>,
}

/// One message's outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Fetched<V = BodyValue> {
    Body(Box<Body<V>>),
    /// The server holds no such message.
    Gone,
}

/// Per email of one call; an email absent here could not be fetched.
pub(super) type Bodies = HashMap<EmailId, Fetched>;

/// The bytes of one value as the windows brought them, decoded once the
/// conversation is given back.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RawValue {
    leaf: Leaf,
    bytes: Vec<u8>,
    cap: u32,
    whole: bool,
}

impl RawValue {
    fn decode(&self) -> BodyValue {
        values::decode(&self.leaf, &self.bytes, self.cap, self.whole)
    }
}

/// The maps of one fill before the decode.
type Raw = HashMap<EmailId, Fetched<RawValue>>;

/// One message to fetch for one call.
struct Job<'a> {
    ask: usize,
    id: EmailId,
    folder: MailboxId,
    uid: u32,
    spec: &'a BodyAsk,
}

/// What one fill works through: the jobs in order, the folder rows it
/// selects by, the maps it fills and the windows it may still spend.
struct Fill<'a> {
    jobs: VecDeque<Job<'a>>,
    rows: HashMap<MailboxId, MailboxRow>,
    results: Vec<Raw>,
    budget: usize,
}

impl Fill<'_> {
    /// The job at the front cannot be read now, its folder gone from
    /// the store or renumbered; its message stays absent.
    fn unreachable(&mut self) {
        self.jobs.pop_front();
    }
}

/// One text part whose value is wanted: where it lies and how much of
/// it.
struct Wanted<'a> {
    uid: u32,
    part_id: String,
    /// The section a fetch names.
    section: String,
    leaf: &'a Leaf,
    cap: u32,
}

/// Why one job did not end in a body.
enum Failure {
    /// The structure fetch failed on what the server sent; the session
    /// must be dropped and the message answers as one part.
    TooComplex,
    /// A NO on the message's own fetch; the job goes, the session stays.
    Refused,
    /// A window answer the bridge cannot read; the job and the session
    /// go, and the message itself is not tried again.
    Unreadable,
    /// The session failed; the job stays.
    Session,
}

/// Fetches the bodies of every ask, one map per ask in order, within
/// the deadline of the conversation and the windows the request may
/// still spend. A message the server does not hold is `Gone`; one it
/// cannot describe answers without a tree; one the fill could not
/// read is absent.
///
/// # Errors
///
/// Returns the store's failure or `Task`.
pub(super) async fn fill<C: Connector>(
    cache: &Cache,
    link: &Link<C>,
    asks: &[BodyAsk],
    windows: &mut usize,
) -> Result<Vec<Bodies>, SyncError> {
    let mut fill = located(cache, asks, *windows).await?;
    if fill.jobs.is_empty() {
        return Ok(decoded(fill.results));
    }
    let mut wire = link.wire.lock().await;
    let deadline = wire.deadline();
    let run = async {
        for _ in 0..=BODY_RECONNECTS {
            let Ok(session) = wire.session(cache).await else {
                return false;
            };
            if fetch(session, &mut fill).await {
                return true;
            }
            wire.drop_session();
            if fill.jobs.is_empty() {
                return true;
            }
        }
        false
    };
    // A deadline that fires reads as a session that does not stand.
    let stands = timeout(deadline, run).await.unwrap_or(false);
    if !stands {
        wire.drop_session();
    }
    drop(wire);
    *windows = fill.budget;
    let raw = fill.results;
    blocking(move || Ok::<_, StoreError>(decoded(raw))).await
}

/// Every value decoded, the maps as they were.
fn decoded(raw: Vec<Raw>) -> Vec<Bodies> {
    raw.into_iter()
        .map(|bodies| {
            bodies
                .into_iter()
                .map(|(id, fetched)| (id, decoded_one(fetched)))
                .collect()
        })
        .collect()
}

fn decoded_one(fetched: Fetched<RawValue>) -> Fetched {
    match fetched {
        Fetched::Gone => Fetched::Gone,
        Fetched::Body(body) => Fetched::Body(Box::new(Body {
            tree: body.tree,
            header: body.header,
            values: body
                .values
                .iter()
                .map(|(part_id, raw)| (part_id.clone(), raw.decode()))
                .collect(),
        })),
    }
}

/// Where the messages lie, as jobs in the order of the asks, with the
/// folder rows the fetch selects by and the windows to spend.
async fn located<'a>(
    cache: &Cache,
    asks: &'a [BodyAsk],
    windows: usize,
) -> Result<Fill<'a>, SyncError> {
    let (store, key) = (Arc::clone(&cache.store), cache.key.clone());
    let ids: Vec<EmailId> = asks.iter().flat_map(|ask| ask.ids.clone()).collect();
    let (located, rows) = blocking(move || {
        let names: Vec<&str> = ids.iter().map(EmailId::as_str).collect();
        let located: HashMap<EmailId, (MailboxId, u32)> = store
            .locate(&key, &names)?
            .into_iter()
            .map(|row| (row.id, (row.folder, row.uid)))
            .collect();
        let rows: HashMap<MailboxId, MailboxRow> = store
            .mailbox_snapshot(&key)?
            .rows
            .into_iter()
            .map(|row| (row.id.clone(), row))
            .collect();
        Ok((located, rows))
    })
    .await?;
    let mut fill = Fill {
        jobs: VecDeque::new(),
        rows,
        results: asks.iter().map(|_| Raw::new()).collect(),
        budget: windows,
    };
    for (index, ask) in asks.iter().enumerate() {
        for id in &ask.ids {
            // A row without a place waits for the next pass and stays absent.
            if let Some((folder, uid)) = located.get(id) {
                fill.jobs.push_back(Job {
                    ask: index,
                    id: id.clone(),
                    folder: folder.clone(),
                    uid: *uid,
                    spec: ask,
                });
            }
        }
    }
    Ok(fill)
}

/// Every job in order on one session: EXAMINE the folder where the job
/// before lay elsewhere, then the message. Whether the session still
/// stands; a job that ended the session stays at the front unless the
/// message itself was the cause.
async fn fetch<S: Session>(session: &mut S, fill: &mut Fill<'_>) -> bool {
    let mut selected: Option<MailboxId> = None;
    while let Some(job) = fill.jobs.front() {
        if selected.as_ref() != Some(&job.folder) {
            // EXAMINE replaces the selected mailbox whatever it answers.
            selected = None;
            let Some(row) = fill.rows.get(&job.folder) else {
                fill.unreachable();
                continue;
            };
            match session.examine(&row.facts.imap_name).await {
                Ok(chosen) if Some(chosen.uid_validity) == row.facts.uid_validity => {
                    selected = Some(job.folder.clone());
                }
                Ok(_) | Err(SessionError::Refused) => {
                    fill.unreachable();
                    continue;
                }
                Err(_) => return false,
            }
        }
        match one(session, job, &mut fill.budget).await {
            Ok(fetched) => {
                fill.results[job.ask].insert(job.id.clone(), fetched);
                fill.jobs.pop_front();
            }
            Err(Failure::Refused) => {
                fill.jobs.pop_front();
            }
            Err(Failure::Unreadable) => {
                fill.jobs.pop_front();
                return false;
            }
            Err(Failure::TooComplex) => {
                let body = Body {
                    tree: None,
                    header: Vec::new(),
                    values: BTreeMap::new(),
                };
                fill.results[job.ask].insert(job.id.clone(), Fetched::Body(Box::new(body)));
                fill.jobs.pop_front();
                return false;
            }
            Err(Failure::Session) => return false,
        }
    }
    true
}

/// One message: the structure or the header fields, then every wanted
/// text part in windows.
async fn one<S: Session>(
    session: &mut S,
    job: &Job<'_>,
    budget: &mut usize,
) -> Result<Fetched<RawValue>, Failure> {
    let spec = job.spec;
    // An answer the bridge cannot read is the message's own shape, as the
    // header sync has it; a NO is the message's alone; anything else ends
    // the session.
    let (tree, header) = if spec.structure {
        match session.uid_structure(job.uid, &spec.fields).await {
            Ok(Some(structure)) => (structure.tree, structure.header),
            Ok(None) => return Ok(Fetched::Gone),
            Err(SessionError::Protocol(_)) => return Err(Failure::TooComplex),
            Err(SessionError::Refused) => return Err(Failure::Refused),
            Err(_) => return Err(Failure::Session),
        }
    } else {
        match session.uid_header_fields(job.uid, &spec.fields).await {
            Ok(Some(header)) => (None, header),
            Ok(None) => return Ok(Fetched::Gone),
            Err(SessionError::Protocol(_)) => return Err(Failure::TooComplex),
            Err(SessionError::Refused) => return Err(Failure::Refused),
            Err(_) => return Err(Failure::Session),
        }
    };
    let mut values = BTreeMap::new();
    if let Some(root) = &tree {
        for wanted in wanted(job.uid, root, spec.values) {
            let Some(value) = value_of(session, &wanted, budget).await? else {
                return Ok(Fetched::Gone);
            };
            values.insert(wanted.part_id, value);
        }
    }
    Ok(Fetched::Body(Box::new(Body {
        tree,
        header,
        values,
    })))
}

/// The text parts whose values the call wants, in the order of the
/// message.
fn wanted(uid: u32, root: &BodyPart, ask: ValueAsk) -> Vec<Wanted<'_>> {
    if !ask.wants_any() {
        return Vec::new();
    }
    let lists = body::lists(root);
    let named =
        |list: &[body::Node<'_>], part_id: &str| list.iter().any(|node| node.part_id == part_id);
    body::leaves(root)
        .into_iter()
        .filter(|node| node.leaf.media_type == "text")
        .filter(|node| {
            ask.all
                || (ask.text && named(&lists.text, &node.part_id))
                || (ask.html && named(&lists.html, &node.part_id))
        })
        .map(|node| Wanted {
            uid,
            section: body::fetch_section(root, &node.part_id),
            part_id: node.part_id,
            leaf: node.leaf,
            cap: ask.cap,
        })
        .collect()
}

/// One value's bytes in windows up to the cap, the budget spent one
/// window at a time; `None` when the message left in between.
async fn value_of<S: Session>(
    session: &mut S,
    wanted: &Wanted<'_>,
    budget: &mut usize,
) -> Result<Option<RawValue>, Failure> {
    let want = wanted.leaf.bytes.min(wanted.cap);
    // One window's room up front; the size a server claims is not trusted.
    let mut buffer = Vec::with_capacity(usize::try_from(want.min(BODY_WINDOW_BYTES)).unwrap_or(0));
    let mut offset = 0;
    let mut ended = false;
    while offset < want && *budget > 0 {
        *budget -= 1;
        let bytes = (want - offset).min(BODY_WINDOW_BYTES);
        let ask = PartAsk {
            uid: wanted.uid,
            section: &wanted.section,
            window: PartWindow { offset, bytes },
        };
        match session.uid_part(&ask).await {
            Ok(Some(data)) => {
                let short = data.len() < usize::try_from(bytes).unwrap_or(usize::MAX);
                offset = offset.saturating_add(u32::try_from(data.len()).unwrap_or(u32::MAX));
                buffer.extend_from_slice(&data);
                if short {
                    ended = true;
                    break;
                }
            }
            Ok(None) => return Ok(None),
            Err(SessionError::Refused) => return Err(Failure::Refused),
            Err(SessionError::Protocol(_)) => return Err(Failure::Unreadable),
            Err(_) => return Err(Failure::Session),
        }
    }
    Ok(Some(RawValue {
        leaf: wanted.leaf.clone(),
        bytes: buffer,
        cap: wanted.cap,
        whole: ended || offset >= wanted.leaf.bytes,
    }))
}
