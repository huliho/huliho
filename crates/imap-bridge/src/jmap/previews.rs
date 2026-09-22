// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The previews an `Email/get` asked for that the rows lack: one fetch
//! per folder and part the messages share, decoded off the runtime and
//! kept in the sealed blobs as one state. A server that does not answer
//! costs nothing but the previews of this request.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use crate::mailboxes::SyncError;
use crate::runtime::{Connector, Link};
use crate::session::{MAX_PREVIEWS, PreviewAsk, PreviewBytes, Session, SessionError};
use crate::store::{EmailId, MailboxId, MailboxRow, Personal};
use crate::sync::{Cache, blocking, preview};

/// The previews one `Email/get` fetches at most.
pub const PREVIEW_BATCH: usize = MAX_PREVIEWS;

/// The messages of one folder that share a part and an ask.
#[derive(Default)]
struct Group {
    uids: Vec<u32>,
    ids: HashMap<u32, EmailId>,
}

/// One folder, one part number, one length of text.
type Shared = (MailboxId, String, u32);

/// Fetches and keeps the previews of `ids`, `PREVIEW_BATCH` at most. An
/// email without a text part or one that left is skipped; a failure of
/// the session ends the fetch and drops the session.
///
/// # Errors
///
/// Returns the store's failure or `Task`.
pub(super) async fn fill<C: Connector>(
    cache: &Cache,
    link: &Link<C>,
    ids: Vec<EmailId>,
) -> Result<(), SyncError> {
    let (groups, rows) = grouped(cache, ids).await?;
    if groups.is_empty() {
        return Ok(());
    }
    let mut wire = link.wire.lock().await;
    let Ok(session) = wire.session(cache).await else {
        return Ok(());
    };
    let (fetched, stands) = fetch(session, &groups, &rows).await;
    if !stands {
        wire.drop_session();
    }
    drop(wire);
    if fetched.is_empty() {
        return Ok(());
    }
    let (store, key, sealer) = (
        Arc::clone(&cache.store),
        cache.key.clone(),
        Arc::clone(&cache.sealer),
    );
    blocking(move || {
        let previews: Vec<(EmailId, String)> = fetched
            .into_iter()
            .map(|(id, bytes)| (id, preview::text(&bytes.header, &bytes.text)))
            .collect();
        store.save_previews(&key, sealer.as_ref(), &previews)
    })
    .await?;
    Ok(())
}

/// Where the messages lie and which part each needs, grouped by what
/// one command can ask, with the folder rows the fetch selects by.
async fn grouped(
    cache: &Cache,
    ids: Vec<EmailId>,
) -> Result<(BTreeMap<Shared, Group>, HashMap<MailboxId, MailboxRow>), SyncError> {
    let (store, key, sealer) = (
        Arc::clone(&cache.store),
        cache.key.clone(),
        Arc::clone(&cache.sealer),
    );
    blocking(move || {
        let names: Vec<&str> = ids.iter().map(EmailId::as_str).collect();
        let mut groups: BTreeMap<Shared, Group> = BTreeMap::new();
        for row in store.locate(&key, &names)? {
            let Some(plain) = sealer.open(&key, &row.id, &row.sealed) else {
                continue;
            };
            let personal: Personal = serde_json::from_slice(&plain)?;
            let Some(part) = personal.preview_part.filter(|_| personal.preview.is_none()) else {
                continue;
            };
            let shared = (row.folder, part.path.clone(), preview::fetch_bytes(&part));
            let group = groups.entry(shared).or_default();
            group.uids.push(row.uid);
            group.ids.insert(row.uid, row.id);
        }
        let rows = store
            .mailbox_snapshot(&key)?
            .rows
            .into_iter()
            .map(|row| (row.id.clone(), row))
            .collect();
        Ok((groups, rows))
    })
    .await
}

/// Every group on the one session: EXAMINE the folder where the group
/// before it lay elsewhere, then the ask in chunks of `MAX_PREVIEWS`.
/// Answers what arrived and whether the session still stands; a refused
/// command skips its group alone.
async fn fetch<S: Session>(
    session: &mut S,
    groups: &BTreeMap<Shared, Group>,
    rows: &HashMap<MailboxId, MailboxRow>,
) -> (Vec<(EmailId, PreviewBytes)>, bool) {
    let mut fetched = Vec::new();
    let mut selected: Option<&MailboxId> = None;
    for ((folder, path, text_bytes), group) in groups {
        if selected != Some(folder) {
            // EXAMINE replaces the selected mailbox whatever it answers.
            selected = None;
            let Some(row) = rows.get(folder) else {
                continue;
            };
            match session.examine(&row.facts.imap_name).await {
                Ok(chosen) if Some(chosen.uid_validity) == row.facts.uid_validity => {
                    selected = Some(folder);
                }
                Ok(_) | Err(SessionError::Refused) => continue,
                Err(_) => return (fetched, false),
            }
        }
        for uids in group.uids.chunks(MAX_PREVIEWS) {
            let ask = PreviewAsk {
                uids,
                path,
                text_bytes: *text_bytes,
            };
            match session.uid_previews(&ask).await {
                Ok(answered) => {
                    for bytes in answered {
                        if let Some(id) = group.ids.get(&bytes.uid) {
                            fetched.push((id.clone(), bytes));
                        }
                    }
                }
                Err(SessionError::Refused) => break,
                Err(_) => return (fetched, false),
            }
        }
    }
    (fetched, true)
}
