// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What one call of a request is handed and what it leaves: the work
//! brought for it between two passes, what it asks of the server for the
//! next one and the answer that stands right after its own.

use serde_json::{Map, Value};

use super::super::{Context, Invocation};
use super::{Brought, Need};

/// The name of the one answer a call leaves after its own: the implicit
/// `Email/set` of a submission (RFC 8621 section 7.5).
const TRAILER: &str = "Email/set";

/// Hands one call what was brought for it and nothing it was not.
pub(super) fn hand<'a>(context: &Context<'a>, had: Option<&'a Brought>) {
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
    *context.submitted.borrow_mut() = match had {
        Some(Brought::Submitted(sent)) => Some(sent),
        _ => None,
    };
}

/// What one call left for the request to fetch or store, taken out.
pub(super) fn left(context: &Context<'_>) -> Vec<Need> {
    let previews = std::mem::take(&mut *context.missing_previews.borrow_mut());
    let previews = (!previews.is_empty()).then_some(Need::Previews(previews));
    [
        previews,
        context.body_ask.borrow_mut().take().map(Need::Bodies),
        context.set_ask.borrow_mut().take().map(Need::Write),
        context.folder_ask.borrow_mut().take().map(Need::Folders),
        context.search_ask.borrow_mut().take().map(Need::Search),
        context.submit_ask.borrow_mut().take().map(Need::Submit),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// The responses in the order of their calls, each call's trailer right
/// after its own under its call id. A reference finds the first response
/// under a call id (RFC 8620 section 3.7), so no trailer is ever one.
pub(super) fn trailed(
    responses: Vec<Invocation>,
    trailers: &[(usize, Map<String, Value>)],
    calls: &[Invocation],
) -> Vec<Invocation> {
    let mut all = Vec::with_capacity(responses.len() + trailers.len());
    for (index, response) in responses.into_iter().enumerate() {
        all.push(response);
        if let Some((_, trailer)) = trailers.iter().find(|(found, _)| *found == index) {
            let Invocation(_, _, call_id) = &calls[index];
            let name = TRAILER.to_owned();
            all.push(Invocation(name, trailer.clone(), call_id.clone()));
        }
    }
    all
}
