// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! One Request object run against an account: every call in order off
//! the runtime, a refresh ahead of a `/changes` call and a second pass
//! over the `Email/get` calls whose previews were fetched in between.

use std::cell::RefCell;

use serde_json::Value;

use super::previews::{self, PREVIEW_BATCH};
use super::{
    Context, Invocation, MAX_CALLS_IN_REQUEST, MAX_SIZE_REQUEST, Request, RequestError, Response,
    Using, references, run, using,
};
use crate::mailboxes::SyncError;
use crate::runtime::{Connector, Link};
use crate::store::{EmailId, MailboxId, StoreError};
use crate::sync::Cache;

/// A sync failure inside a request: the store's and the task's count,
/// the session's never, since the cache answers as it stands.
fn sync_failed(error: SyncError) -> Result<(), RequestError> {
    match error {
        SyncError::Store(error) => Err(RequestError::Store(error)),
        SyncError::Task => Err(RequestError::Task),
        SyncError::Session(_) => Ok(()),
    }
}

/// Runs one Request object against the account and answers the Response
/// object as JSON, its `sessionState` the value the host derived for the
/// session object. A `/changes` call has the account refreshed first
/// when a refresh is due; an `Email/get` that asks for previews the rows
/// lack has them fetched and runs again. Neither waits on a server that
/// is down: the cache answers as it stands.
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
    if using.mail
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
    if !answered.missing.is_empty() {
        let mut ids: Vec<EmailId> = Vec::new();
        for (_, found) in &answered.missing {
            for id in found {
                if !ids.contains(id) && ids.len() < PREVIEW_BATCH {
                    ids.push(id.clone());
                }
            }
        }
        if let Err(error) = previews::fill(cache, link, ids).await {
            sync_failed(error)?;
        }
        let indexes: Vec<usize> = answered.missing.iter().map(|(index, _)| *index).collect();
        let again = Some((answered.responses, indexes));
        answered = off_runtime(cache, using, &calls, again).await?;
    }
    let response = Response {
        method_responses: answered.responses,
        created_ids: request.created_ids,
        session_state: session_state.to_owned(),
    };
    serde_json::to_vec(&response).map_err(|error| RequestError::Store(StoreError::Encoding(error)))
}

/// What one pass over the calls produced.
struct Answered {
    responses: Vec<Invocation>,
    viewed: Option<MailboxId>,
    /// Per call that asked, the emails whose preview the rows lack.
    missing: Vec<(usize, Vec<EmailId>)>,
}

/// The responses to run again in place, by index.
type Again = Option<(Vec<Invocation>, Vec<usize>)>;

/// Runs the calls on a blocking thread, since every method reads SQLite
/// and opens blobs.
async fn off_runtime(
    cache: &Cache,
    using: Using,
    calls: &[Invocation],
    again: Again,
) -> Result<Answered, RequestError> {
    let (cache, calls) = (cache.clone(), calls.to_vec());
    tokio::task::spawn_blocking(move || answer(&cache, using, &calls, again))
        .await
        .map_err(|_join| RequestError::Task)
}

/// Every call in order, or the named ones again with the earlier
/// responses standing, so a reference into one still resolves.
fn answer(cache: &Cache, using: Using, calls: &[Invocation], again: Again) -> Answered {
    let context = Context {
        store: &cache.store,
        sealer: cache.sealer.as_ref(),
        key: &cache.key,
        using,
        budget: references::Budget::full(),
        viewed: RefCell::new(None),
        missing_previews: RefCell::new(Vec::new()),
    };
    let (mut responses, rerun) = match again {
        Some((responses, indexes)) => (responses, Some(indexes)),
        None => (Vec::with_capacity(calls.len()), None),
    };
    let mut missing = Vec::new();
    for (index, call) in calls.iter().enumerate() {
        match &rerun {
            Some(indexes) if !indexes.contains(&index) => continue,
            _ => {}
        }
        let response = run(&context, call, &responses[..index]);
        let found = std::mem::take(&mut *context.missing_previews.borrow_mut());
        if !found.is_empty() && rerun.is_none() {
            missing.push((index, found));
        }
        if rerun.is_some() {
            responses[index] = response;
        } else {
            responses.push(response);
        }
    }
    Answered {
        responses,
        viewed: context.viewed.take(),
        missing,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;

    use super::super::CORE_CAPABILITY;
    use super::*;
    use crate::store::{AccountKey, Store};
    use crate::testing::TestConnector;
    use crate::testing::seal::TestSealer;

    /// `Core/echo` calls: the first echoes 64 KiB of text, every later
    /// one echoes the call before it twice through the path `""`.
    fn doubling(calls: usize) -> Vec<Value> {
        let mut out = vec![json!(["Core/echo", { "text": "x".repeat(64 * 1024) }, "c0"])];
        for call in 1..calls {
            let before = json!({
                "resultOf": format!("c{}", call - 1),
                "name": "Core/echo",
                "path": ""
            });
            let twice = json!({ "#a": before, "#b": before });
            out.push(json!(["Core/echo", twice, format!("c{call}")]));
        }
        out
    }

    fn cache() -> Cache {
        Cache {
            store: Arc::new(Store::in_memory().unwrap()),
            sealer: Arc::new(TestSealer::default()),
            key: AccountKey::new("a1"),
            gmail: false,
        }
    }

    async fn responses(cache: &Cache, calls: &[Value]) -> Vec<Invocation> {
        let body = json!({ "using": [CORE_CAPABILITY], "methodCalls": calls });
        let body = serde_json::to_vec(&body).unwrap();
        let link = Link::new(TestConnector::Refusing);
        let answer = handle(cache, &link, &body, "s1").await.unwrap();
        let mut answer: Value = serde_json::from_slice(&answer).unwrap();
        assert_eq!(answer["sessionState"], "s1");
        serde_json::from_value(answer["methodResponses"].take()).unwrap()
    }

    #[tokio::test]
    async fn references_past_the_budget_fail_their_call_alone_rfc8620_3_6_2() {
        let cache = cache();
        let mut calls = doubling(5);
        calls.push(json!(["Core/echo", { "after": true }, "c5"]));
        let responses = responses(&cache, &calls).await;
        for Invocation(name, _, _) in &responses[..4] {
            assert_eq!(name, "Core/echo");
        }
        let Invocation(name, error, id) = &responses[4];
        assert_eq!((name.as_str(), id.as_str()), ("error", "c4"));
        assert_eq!(error["type"], "invalidResultReference");
        assert!(error["description"].is_string());
        let Invocation(name, after, id) = &responses[5];
        assert_eq!((name.as_str(), id.as_str()), ("Core/echo", "c5"));
        assert_eq!(after["after"], true);
    }

    #[tokio::test]
    async fn a_chain_of_references_inside_the_budget_resolves_rfc8620_3_7() {
        let cache = cache();
        let ids = |call: &str| json!({ "resultOf": call, "name": "Core/echo", "path": "/ids" });
        let calls = [
            json!(["Core/echo", { "ids": ["e1", "e2"] }, "c0"]),
            json!(["Core/echo", { "#ids": ids("c0") }, "c1"]),
            json!(["Core/echo", { "#ids": ids("c1") }, "c2"]),
        ];
        for Invocation(name, arguments, _) in responses(&cache, &calls).await {
            assert_eq!(name, "Core/echo");
            assert_eq!(arguments["ids"], json!(["e1", "e2"]));
        }
    }

    #[tokio::test]
    async fn every_request_starts_with_a_full_budget() {
        let cache = cache();
        for _ in 0..2 {
            for Invocation(name, _, _) in responses(&cache, &doubling(4)).await {
                assert_eq!(name, "Core/echo");
            }
        }
    }
}
