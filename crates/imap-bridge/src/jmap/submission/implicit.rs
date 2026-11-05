// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The implicit `Email/set` after a send (RFC 8621 section 7.5), run as
//! any `Email/set` runs. A patch that files the email from one of its
//! mailboxes into another puts the Sent form into the new one on a folder
//! account, then the email leaves once the copy landed; Gmail keeps a
//! copy of what it sent, so there the email goes to Trash as a replaced
//! draft does. Every other patch runs as the update it reads as and a
//! destroy as a destroy. The email of a filing does not survive it, so
//! the answer names it updated with no property, as a move whose id does
//! not survive is answered.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use super::super::email_set::create::STORABLE_KEYWORDS;
use super::super::set::{self, Copied, Lists, Outcome, Patch, SetAsk, SetError, Source};
use super::super::{Context, write};
use super::OnSuccess;
use crate::mailboxes::SyncError;
use crate::runtime::drafts::{Built, CopyFacts, Unsealed};
use crate::runtime::folders::{CreatedIds, Round};
use crate::runtime::{Connector, Link};
use crate::store::{EmailRow, Personal};
use crate::sync::{Cache, mapping};

/// The creation id of the Sent copy inside the implicit call.
const COPY: &str = "sent";

/// The Sent form of a message that went out: the email's row, the bytes
/// the Sent mailbox gets and the moment of the send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SentForm {
    pub row: EmailRow,
    pub bytes: Vec<u8>,
    pub date: i64,
}

/// Where the call writes: the account's cache and link and the ids the
/// request made so far, which a patch may name and the call reads only.
pub(super) struct Account<'a, C: Connector> {
    pub cache: &'a Cache,
    pub link: &'a Link<C>,
    pub created: &'a CreatedIds,
}

/// The implicit call over every submission of one ask that went out: the
/// states of its writes and what it answers per email.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(in crate::jmap) struct Implicit {
    states: Option<(u64, u64)>,
    lists: Lists,
}

impl Implicit {
    /// What the call does to the email of one submission that went out.
    /// A failure on the way answers for that email alone, since the
    /// message is with the submission server by then.
    pub(super) async fn run<C: Connector>(
        &mut self,
        account: &Account<'_, C>,
        sent: SentForm,
        action: &OnSuccess,
    ) {
        let id = sent.row.id.to_string();
        if let Err(error) = self.tried(account, sent, action).await {
            let refusal = failed(&error).object();
            match action {
                OnSuccess::Destroy => self.lists.not_destroyed.insert(id, refusal),
                OnSuccess::Update(_) => self.lists.not_updated.insert(id, refusal),
            };
        }
    }

    async fn tried<C: Connector>(
        &mut self,
        account: &Account<'_, C>,
        sent: SentForm,
        action: &OnSuccess,
    ) -> Result<(), SyncError> {
        let id = sent.row.id.to_string();
        let patch = match action {
            OnSuccess::Destroy => {
                let outcome = applied(account, ask(vec![], vec![], &id)).await?;
                if let Outcome::Stored { destroyed, .. } = self.noted(outcome) {
                    for (id, result) in &destroyed {
                        self.lists.destroyed_as(id, result);
                    }
                }
                return Ok(());
            }
            OnSuccess::Update(value) => match set::read_patch(value) {
                Ok(patch) => patch,
                Err(error) => {
                    self.lists.not_updated.insert(id, error.object());
                    return Ok(());
                }
            },
        };
        let Some(target) = filing(&patch, &sent.row, account.created) else {
            let updates = vec![(id.clone(), patch)];
            let outcome = applied(account, ask(vec![], updates, "")).await?;
            if let Outcome::Stored { updated, .. } = self.noted(outcome) {
                for (id, result) in &updated {
                    self.lists.updated_as(id, result);
                }
            }
            return Ok(());
        };
        let filed = if account.cache.gmail {
            Vec::new()
        } else if let Some(built) = copy(account.cache, sent, &patch) {
            let sent_copy = Copied {
                mailbox: target,
                built,
            };
            vec![(COPY.to_owned(), Source::Copy(sent_copy))]
        } else {
            self.lists
                .not_updated
                .insert(id, SetError::ServerFail.object());
            return Ok(());
        };
        let appends = !filed.is_empty();
        let outcome = applied(account, ask(filed, vec![], &id)).await?;
        let Outcome::Stored {
            created: landed,
            destroyed,
            ..
        } = self.noted(outcome)
        else {
            return Ok(());
        };
        let result = if appends {
            first(landed).map(|_| None)
        } else {
            first(destroyed).map(|()| None)
        };
        self.lists.updated_as(&id, &result);
        Ok(())
    }

    /// Keeps the states of a write the call made: the first one's before,
    /// the last one's after.
    fn noted(&mut self, outcome: Outcome) -> Outcome {
        if let Outcome::Stored {
            old_state,
            new_state,
            ..
        } = &outcome
        {
            let before = self.states.map_or(*old_state, |(before, _)| before);
            self.states = Some((before, *new_state));
        }
        outcome
    }

    /// The answer of the call: between the states of its writes, or at
    /// the account's own state where no write ran; none where that state
    /// cannot be read, so the submission's own answer still stands.
    pub(super) fn answer(self, context: &Context<'_>) -> Option<Map<String, Value>> {
        let current = || {
            context
                .store
                .state(context.key)
                .ok()
                .map(|state| (state, state))
        };
        let states = self.states.or_else(current)?;
        Some(set::answer(context.key, states, self.lists))
    }
}

/// What a write that did not come about answers: the store's failure or
/// a task ended early as the server's, a session gone as unavailable.
fn failed(error: &SyncError) -> SetError {
    match error {
        SyncError::Session(_) => SetError::ServerUnavailable,
        SyncError::Store(_) | SyncError::Task => SetError::ServerFail,
    }
}

/// How the one create or destroy of a write ended.
fn first<T>(results: Vec<(String, Result<T, SetError>)>) -> Result<T, SetError> {
    results
        .into_iter()
        .next()
        .map_or(Err(SetError::ServerFail), |(_, result)| result)
}

fn ask(creates: Vec<(String, Source)>, updates: Vec<(String, Patch)>, destroy: &str) -> SetAsk {
    SetAsk {
        if_in_state: None,
        creates,
        updates,
        destroy: if destroy.is_empty() {
            Vec::new()
        } else {
            vec![destroy.to_owned()]
        },
    }
}

/// One write on the account's conversation with a deadline of its own,
/// since the send before it may have spent the request's. It runs over
/// a copy of the request's creation ids: the write resolves against them
/// and lands the Sent copy's id there, never in the request's own map.
async fn applied<C: Connector>(
    account: &Account<'_, C>,
    ask: SetAsk,
) -> Result<Outcome, SyncError> {
    let mut scratch = account.created.clone();
    let mut round = Round {
        until: None,
        created: &mut scratch,
    };
    write::apply(account.cache, account.link, &ask, &mut round).await
}

/// The mailbox the patch files the email into: one mailbox it adds and
/// one of the email's own it removes, and nothing more of either.
fn filing(patch: &Patch, row: &EmailRow, created: &CreatedIds) -> Option<String> {
    let mut edit = patch.mailboxes.clone()?;
    edit.resolve(created);
    let ([added], [removed]) = (edit.add.as_slice(), edit.remove.as_slice()) else {
        return None;
    };
    let member = row
        .mailbox_ids
        .iter()
        .any(|mailbox| mailbox.as_str() == removed);
    (!edit.whole && member && added != removed).then(|| added.clone())
}

/// The Sent copy: the email's own fields under an id of its own, the
/// keywords the patch leaves of the email's that a server stores on an
/// APPEND, the size of the Sent form and the moment of the send. None
/// where the host does not open the email's row.
fn copy(cache: &Cache, sent: SentForm, patch: &Patch) -> Option<Built> {
    let row = &sent.row;
    let plain = cache.sealer.open(&cache.key, &row.id, &row.sealed)?;
    let personal: Personal = serde_json::from_slice(&plain).ok()?;
    let edit = &patch.keywords;
    let mut keywords: BTreeMap<String, bool> = if edit.whole {
        BTreeMap::new()
    } else {
        row.keywords.clone()
    };
    for keyword in &edit.clear {
        keywords.remove(keyword);
    }
    for keyword in &edit.set {
        keywords.insert(keyword.clone(), true);
    }
    keywords.retain(|keyword, _| STORABLE_KEYWORDS.contains(&keyword.as_str()));
    let flags = keywords
        .keys()
        .map(|keyword| mapping::flag(keyword))
        .collect();
    let facts = CopyFacts {
        keywords,
        size: u32::try_from(sent.bytes.len()).unwrap_or(u32::MAX),
        date: sent.date,
    };
    let unsealed = Unsealed::copied(personal, row.has_attachment, facts, cache.sealer.as_ref());
    Some(Built {
        message: sent.bytes,
        flags,
        unsealed,
    })
}
