// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! One Request object run against an account: every call in order off
//! the runtime, a refresh ahead of a `/changes` call and a later pass
//! over the calls from the first `Email/get` whose previews or bodies
//! were fetched in between, the first `Email/set` or `Mailbox/set` whose
//! writes were done there or the first `Email/query` the server was
//! searched for.

#[cfg(test)]
mod tests;

use std::cell::RefCell;

use serde_json::Value;
use tokio::time::Instant;

use super::bodies::{self, Bodies, BodyAsk};
use super::previews::{self, PREVIEW_BATCH};
use super::set::{Outcome, SetAsk};
use super::values::MAX_BODY_WINDOWS;
use super::{
    Context, Invocation, MAIL_CAPABILITY, MAX_CALLS_IN_REQUEST, MAX_SIZE_REQUEST, QUOTA_CAPABILITY,
    Request, RequestError, Response, Using, references, run, session, using, write,
};
use crate::mailboxes::SyncError;
use crate::runtime::folders::{self, Ask, CreatedIds, Round};
use crate::runtime::search::{Failure, Hits, Plan};
use crate::runtime::{Connector, Link};
use crate::store::{EmailId, MailboxId, StoreError};
use crate::sync::Cache;

/// The passes one request may take: the first, one for what its calls
/// fetched or stored and one for a call whose reference into such an
/// answer resolved only then or whose write waited behind another call.
/// A need left after the last pass answers `serverUnavailable`, a write
/// among them with nothing stored.
const MAX_PASSES: usize = 3;

/// A sync failure inside a request: the store's and the task's count,
/// the session's never, since the cache answers as it stands.
fn sync_failed(error: SyncError) -> Result<(), RequestError> {
    match error {
        SyncError::Store(error) => Err(RequestError::Store(error)),
        SyncError::Task => Err(RequestError::Task),
        SyncError::Session(_) => Ok(()),
    }
}

/// What a request carries across its passes: the body windows it may
/// still spend and the creation ids it resolved so far, seeded from its
/// `createdIds` (RFC 8620 section 3.3).
struct Carried {
    windows: usize,
    created: CreatedIds,
}

/// Runs one Request object against the account and answers the Response
/// object as JSON, its `sessionState` the session object's state as the
/// request leaves it: the value the host derived, marked while the
/// server advertises QUOTA. A `/changes` call has the account refreshed
/// first when a refresh is due; an `Email/get` that asks for previews
/// the rows lack or for bodies has them fetched and runs again, with
/// every call after it, at most `MAX_PASSES` passes in all; a write and
/// a search answer on the next pass the same way. None waits on a server
/// that is down: the cache answers as it stands; a body it does not hold
/// and a search past the deadline of its round answer
/// `serverUnavailable`; an update that could not be stored says so. A
/// request that came with `createdIds` gets them back with every
/// creation id its calls resolved (RFC 8620 section 3.4).
///
/// # Errors
///
/// Returns [`RequestError`] when the request cannot run at all; a
/// method that fails answers inside the Response object instead.
pub async fn handle<C: Connector>(
    cache: &Cache,
    link: &Link<C>,
    body: &[u8],
    session_state: &str,
) -> Result<Vec<u8>, RequestError> {
    if body.len() > MAX_SIZE_REQUEST {
        return Err(RequestError::Limit("maxSizeRequest"));
    }
    let value: Value = serde_json::from_slice(body).map_err(|_| RequestError::NotJson)?;
    let request: Request = serde_json::from_value(value).map_err(|_| RequestError::NotRequest)?;
    let using = using(&request.using)?;
    if request.method_calls.len() > MAX_CALLS_IN_REQUEST {
        return Err(RequestError::Limit("maxCallsInRequest"));
    }
    let asks_changes = request
        .method_calls
        .iter()
        .any(|Invocation(name, ..)| name.ends_with("/changes"));
    if (using.has(MAIL_CAPABILITY) || using.has(QUOTA_CAPABILITY))
        && asks_changes
        && let Err(error) = link.refresh(cache).await
    {
        sync_failed(error)?;
    }
    let calls = request.method_calls;
    let mut answered = off_runtime(cache, using, &calls, None).await?;
    if let Some(viewed) = answered.viewed.take() {
        link.view(viewed);
    }
    let mut carried = Carried {
        windows: MAX_BODY_WINDOWS,
        created: request.created_ids.clone().unwrap_or_default(),
    };
    for _ in 1..MAX_PASSES {
        if answered.needs.is_empty() {
            break;
        }
        let needs = std::mem::take(&mut answered.needs);
        let reruns = fetched(cache, link, needs, &mut carried).await?;
        let first = reruns.iter().map(|(index, _)| *index).min().unwrap_or(0);
        let mut brought = std::mem::take(&mut answered.brought);
        brought.extend(reruns);
        let again = Again {
            responses: answered.responses,
            first,
            brought,
        };
        answered = off_runtime(cache, using, &calls, Some(again)).await?;
    }
    let quota = cache.store.quota(&cache.key).advertised;
    let response = Response {
        method_responses: answered.responses,
        created_ids: request.created_ids.is_some().then_some(carried.created),
        session_state: session::session_state(session_state, quota),
    };
    serde_json::to_vec(&response).map_err(|error| RequestError::Store(StoreError::Encoding(error)))
}

/// What a call left for the request to fetch or store before it runs
/// again.
enum Need {
    /// The emails whose preview the rows lack.
    Previews(Vec<EmailId>),
    Bodies(BodyAsk),
    Write(SetAsk),
    Folders(Ask),
    Search(Plan),
}

/// What the request brought one call between two passes.
enum Brought {
    /// Previews, which the rows hold from then on.
    Rows,
    Bodies(Bodies),
    Written(Outcome),
    Folders(folders::Outcome),
    Searched(Result<Hits, Failure>),
}

/// The calls to run again, by index, with what was brought for each.
type Reruns = Vec<(usize, Brought)>;

/// Hands a call what was brought for it.
fn bring(reruns: &mut Reruns, index: usize, brought: Brought) {
    if let Some((_, slot)) = reruns.iter_mut().find(|(found, _)| *found == index) {
        *slot = brought;
    }
}

/// Brings what the needs name: the writes of the `Email/set` and
/// `Mailbox/set` calls that stand in a row from the first call with a
/// need, in their order, the previews of every call at once, then the
/// bodies per call within the windows the request may still spend, then
/// the searches in the order of their calls under one deadline, as the
/// writes run under theirs. Which calls run again, with what each was
/// brought. A call answers what the calls before it left and nothing of
/// the calls behind it (RFC 8620 section 3.10): the next pass runs every
/// call from that first one again, so a write behind a call that is no
/// write of this round is left for the pass after it, where its call
/// notes it again; a write sees the creation ids of the writes before
/// it alone.
async fn fetched<C: Connector>(
    cache: &Cache,
    link: &Link<C>,
    needs: Vec<(usize, Need)>,
    carried: &mut Carried,
) -> Result<Reruns, RequestError> {
    let mut ids: Vec<EmailId> = Vec::new();
    let mut asks: Vec<(usize, BodyAsk)> = Vec::new();
    let mut plans: Vec<(usize, Plan)> = Vec::new();
    let mut reruns: Reruns = Vec::new();
    // Where the next write of the row stands; the needs come in the
    // order of their calls.
    let mut row = needs.first().map(|(index, _)| *index);
    let Carried { windows, created } = carried;
    let mut round = Round {
        until: None,
        created,
    };
    for (index, need) in needs {
        let write = matches!(need, Need::Write(_) | Need::Folders(_));
        let in_row = write && row == Some(index);
        row = in_row.then_some(index + 1);
        if write && !in_row {
            continue;
        }
        if !reruns.iter().any(|(found, _)| *found == index) {
            reruns.push((index, Brought::Rows));
        }
        match need {
            Need::Previews(found) => {
                for id in found {
                    if !ids.contains(&id) && ids.len() < PREVIEW_BATCH {
                        ids.push(id);
                    }
                }
            }
            Need::Bodies(ask) => asks.push((index, ask)),
            Need::Search(plan) => plans.push((index, plan)),
            Need::Write(ask) => match write::apply(cache, link, &ask, &mut round).await {
                Ok(outcome) => bring(&mut reruns, index, Brought::Written(outcome)),
                Err(error) => sync_failed(error)?,
            },
            Need::Folders(ask) => match folders::apply(cache, link, &ask, &mut round).await {
                Ok(outcome) => bring(&mut reruns, index, Brought::Folders(outcome)),
                Err(error) => sync_failed(error)?,
            },
        }
    }
    if !ids.is_empty()
        && let Err(error) = previews::fill(cache, link, ids).await
    {
        sync_failed(error)?;
    }
    if !asks.is_empty() {
        let specs: Vec<BodyAsk> = asks.iter().map(|(_, ask)| ask.clone()).collect();
        let bodies = match bodies::fill(cache, link, &specs, windows).await {
            Ok(bodies) => bodies,
            Err(error) => {
                sync_failed(error)?;
                specs.iter().map(|_| Bodies::new()).collect()
            }
        };
        for ((index, _), fetched) in asks.into_iter().zip(bodies) {
            bring(&mut reruns, index, Brought::Bodies(fetched));
        }
    }
    if !plans.is_empty() {
        // The deadline the searches of this round share.
        let until = Instant::now() + link.search.deadline();
        for (index, plan) in plans {
            let found = link.search.found(cache, &plan, until).await;
            bring(&mut reruns, index, Brought::Searched(found));
        }
    }
    Ok(reruns)
}

/// What one pass over the calls produced.
struct Answered {
    responses: Vec<Invocation>,
    viewed: Option<MailboxId>,
    /// Per call that asked, what it left to fetch or store.
    needs: Vec<(usize, Need)>,
    /// What was brought so far, for the next pass.
    brought: Reruns,
}

/// A later pass: the responses so far, the first call to run again and
/// what was brought for every call that asked so far.
struct Again {
    responses: Vec<Invocation>,
    first: usize,
    brought: Reruns,
}

/// Runs the calls on a blocking thread, since every method reads SQLite
/// and opens blobs.
async fn off_runtime(
    cache: &Cache,
    using: Using,
    calls: &[Invocation],
    again: Option<Again>,
) -> Result<Answered, RequestError> {
    let (cache, calls) = (cache.clone(), calls.to_vec());
    tokio::task::spawn_blocking(move || answer(&cache, using, &calls, again))
        .await
        .map_err(|_join| RequestError::Task)
}

/// Every call in order, or every call from the first one that fetched
/// again with the earlier responses standing: a call after it may
/// reference its answer; every method answers from the cache as it
/// stands, so a call before the first fetched one keeps its response.
/// A call something was brought for asks no more, so nothing is stored
/// twice; one run again for its reference may.
fn answer(cache: &Cache, using: Using, calls: &[Invocation], again: Option<Again>) -> Answered {
    let context = Context {
        store: &cache.store,
        sealer: cache.sealer.as_ref(),
        key: &cache.key,
        using,
        budget: references::Budget::full(),
        viewed: RefCell::new(None),
        missing_previews: RefCell::new(Vec::new()),
        body_ask: RefCell::new(None),
        bodies: RefCell::new(None),
        set_ask: RefCell::new(None),
        written: RefCell::new(None),
        folder_ask: RefCell::new(None),
        folders_done: RefCell::new(None),
        search_ask: RefCell::new(None),
        searched: RefCell::new(None),
    };
    let (mut responses, first, brought) = match again {
        Some(again) => (again.responses, Some(again.first), again.brought),
        None => (Vec::with_capacity(calls.len()), None, Vec::new()),
    };
    let mut needs = Vec::new();
    for (index, call) in calls.iter().enumerate() {
        if first.is_some_and(|first| index < first) {
            continue;
        }
        let had = brought.iter().find(|(found, _)| *found == index);
        let had = had.map(|(_, brought)| brought);
        *context.bodies.borrow_mut() = match had {
            Some(Brought::Bodies(bodies)) => Some(bodies),
            _ => None,
        };
        *context.written.borrow_mut() = match had {
            Some(Brought::Written(outcome)) => Some(outcome),
            _ => None,
        };
        *context.folders_done.borrow_mut() = match had {
            Some(Brought::Folders(outcome)) => Some(outcome),
            _ => None,
        };
        *context.searched.borrow_mut() = match had {
            Some(Brought::Searched(found)) => Some(found),
            _ => None,
        };
        let response = run(&context, call, &responses[..index]);
        let previews = std::mem::take(&mut *context.missing_previews.borrow_mut());
        let ask = context.body_ask.borrow_mut().take();
        let set = context.set_ask.borrow_mut().take();
        let folder = context.folder_ask.borrow_mut().take();
        let search = context.search_ask.borrow_mut().take();
        if had.is_none() {
            if !previews.is_empty() {
                needs.push((index, Need::Previews(previews)));
            }
            if let Some(ask) = ask {
                needs.push((index, Need::Bodies(ask)));
            }
            if let Some(ask) = set {
                needs.push((index, Need::Write(ask)));
            }
            if let Some(ask) = folder {
                needs.push((index, Need::Folders(ask)));
            }
            if let Some(plan) = search {
                needs.push((index, Need::Search(plan)));
            }
        }
        if first.is_some() {
            responses[index] = response;
        } else {
            responses.push(response);
        }
    }
    let viewed = context.viewed.take();
    drop(context);
    Answered {
        responses,
        viewed,
        needs,
        brought,
    }
}
