// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! `Email/query` (RFC 8621 section 4.4) over one `FilterCondition`: the
//! scope as `inMailbox`, `inMailboxOtherThan` or neither, which is the
//! whole account; the keywords, the dates and the attachment; a text and
//! a sender. The sort is `receivedAt` either way, the window by position
//! or by anchor, the total and one email per thread on request. A
//! mailbox with its keywords alone is the window index's. A text or a
//! sender goes to the server's SEARCH between two passes over the
//! request and the rest is SQL. Any other filter or sort is refused as
//! such.

mod plan;
#[cfg(test)]
mod tests;

use std::fmt;
use std::sync::Arc;

use serde::Deserialize;
use serde_json::{Map, Value};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::{Context, MethodError, arguments};
use crate::runtime::search::Failure;
use crate::session::{SearchAsk, Searching, SessionError};
use crate::store::{MailboxId, Queried, Query, Search, Start, Window};
use crate::sync::mapping::canonical_keyword;

/// The most ids one `Email/query` answers; a larger or absent `limit`
/// is lowered to it and the answer says so (RFC 8620 section 5.5).
pub const QUERY_LIMIT: u32 = 200;

/// The one property a comparator may name.
const RECEIVED_AT: &str = "receivedAt";

/// The keywords the coarse UNSEEN and FLAGGED keys stand for.
const SEEN: &str = "$seen";
const FLAGGED: &str = "$flagged";

/// The fixed words of a filter the bridge refuses.
const UNSERVED: &str = "the filter names an operator or a property the bridge does not serve";
const NO_KEYWORD: &str = "a keyword of the filter is no keyword";
const NO_DATE: &str = "a date of the filter is no UTCDate";
const NO_CHARSET: &str = "the server cannot search in UTF-8";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QueryArguments {
    account_id: String,
    filter: Option<Value>,
    sort: Option<Vec<Comparator>>,
    #[serde(default)]
    position: i64,
    anchor: Option<String>,
    #[serde(default)]
    anchor_offset: i64,
    limit: Option<u64>,
    #[serde(default)]
    calculate_total: bool,
    #[serde(default)]
    collapse_threads: bool,
}

/// One comparator (RFC 8620 section 5.5); a collation is refused, since
/// a date has none.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Comparator {
    property: String,
    #[serde(default = "ascending_by_default")]
    is_ascending: bool,
    collation: Option<String>,
}

fn ascending_by_default() -> bool {
    true
}

/// The one filter shape served, as the wire carries it: a
/// `FilterCondition` with these properties of RFC 8621 section 4.4.1.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Condition {
    in_mailbox: Option<String>,
    in_mailbox_other_than: Option<Vec<String>>,
    has_keyword: Option<String>,
    not_keyword: Option<String>,
    text: Option<String>,
    from: Option<String>,
    after: Option<String>,
    before: Option<String>,
    has_attachment: Option<bool>,
}

/// The condition as the call reads it: the keywords in lower case, the
/// dates as whole seconds, the text and the sender as typed.
#[derive(Clone, Default, PartialEq, Eq)]
pub(super) struct Filter {
    pub in_mailbox: Option<String>,
    pub other_than: Option<Vec<String>>,
    pub has_keyword: Option<String>,
    pub not_keyword: Option<String>,
    pub text: String,
    pub from: String,
    /// The earliest second of `receivedAt` that matches.
    pub after: Option<i64>,
    /// Every matching `receivedAt` lies before this second.
    pub before: Option<i64>,
    pub has_attachment: Option<bool>,
}

/// Says whether a text and a sender are set and prints neither, so a
/// filter in a log line carries no text of the client.
impl fmt::Debug for Filter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Filter")
            .field("in_mailbox", &self.in_mailbox)
            .field("other_than", &self.other_than)
            .field("has_keyword", &self.has_keyword)
            .field("not_keyword", &self.not_keyword)
            .field("text", &!self.text.is_empty())
            .field("from", &!self.from.is_empty())
            .field("after", &self.after)
            .field("before", &self.before)
            .field("has_attachment", &self.has_attachment)
            .finish()
    }
}

impl Filter {
    /// The text and the sender as the session would search them; it
    /// counts their terms and checks the caps.
    fn ask(&self) -> SearchAsk<'_> {
        SearchAsk {
            text: &self.text,
            from: &self.from,
            unseen: false,
            flagged: false,
            since: None,
            before: None,
            gmail: None,
            server: Searching::default(),
            until: None,
        }
    }

    /// Whether the server is searched: the text or the sender holds a
    /// term. One of quotes or whitespace alone names nothing.
    fn searches(&self) -> bool {
        self.ask().terms() > 0
    }

    /// A mailbox with its keywords alone, which the window index of the
    /// mailbox answers as built.
    fn is_window(&self) -> bool {
        self.in_mailbox.is_some()
            && self.other_than.is_none()
            && self.after.is_none()
            && self.before.is_none()
            && self.has_attachment.is_none()
            && !self.searches()
    }

    /// Whether the call asks `$seen` away and `$flagged` present, which
    /// the coarse UNSEEN and FLAGGED keys stand for.
    fn unseen(&self) -> bool {
        self.not_keyword.as_deref() == Some(SEEN)
    }

    fn flagged(&self) -> bool {
        self.has_keyword.as_deref() == Some(FLAGGED)
    }
}

/// `Email/query` (RFC 8621 section 4.4).
pub(super) fn email(
    context: &Context<'_>,
    raw: &Map<String, Value>,
) -> Result<Map<String, Value>, MethodError> {
    let query_arguments: QueryArguments = arguments(raw)?;
    context.account(&query_arguments.account_id)?;
    let filter = filter_of(query_arguments.filter.as_ref())?;
    let ascending = ascending(query_arguments.sort.as_deref())?;
    let (limit, lowered) = limit(query_arguments.limit);
    let start = match &query_arguments.anchor {
        // RFC 8620 section 5.5: with an anchor the position is ignored.
        Some(id) => Start::Anchor {
            id,
            offset: query_arguments.anchor_offset,
        },
        None => Start::Position(query_arguments.position),
    };
    let window = Window {
        start,
        limit,
        calculate_total: query_arguments.calculate_total,
    };
    let collapse_threads = query_arguments.collapse_threads;
    let queried = if let (true, Some(mailbox)) = (filter.is_window(), &filter.in_mailbox) {
        let query = Query {
            mailbox,
            has_keyword: filter.has_keyword.as_deref(),
            not_keyword: filter.not_keyword.as_deref(),
            ascending,
            collapse_threads,
        };
        context.store.query(context.key, &query, &window)?
    } else {
        searched(context, &filter, (ascending, collapse_threads), &window)?
    };
    let queried = queried.ok_or(MethodError::AnchorNotFound)?;
    if let Some(mailbox) = &filter.in_mailbox {
        *context.viewed.borrow_mut() = Some(MailboxId::from(mailbox.clone()));
    }
    Ok(answer(context, &queried, lowered))
}

/// One window of a search: the SQL over the hits once the server was
/// searched, over the scope alone where no text and no sender is asked.
/// A term past the caps or holding a NUL is refused before anything
/// connects. The first pass of a search leaves its plan for the request
/// and answers `serverUnavailable`, which stands when the search could
/// not run.
fn searched(
    context: &Context<'_>,
    filter: &Filter,
    (ascending, collapse_threads): (bool, bool),
    window: &Window<'_>,
) -> Result<Option<Queried>, MethodError> {
    let hits = if filter.searches() {
        filter.ask().check().map_err(|error| refused(&error))?;
        match *context.searched.borrow() {
            Some(Ok(hits)) => Some(Arc::clone(hits)),
            Some(Err(failure)) => return Err(failed(*failure)),
            None => {
                let snapshot = context.store.mailbox_snapshot(context.key)?;
                *context.search_ask.borrow_mut() = Some(plan::plan(filter, &snapshot));
                return Err(MethodError::ServerUnavailable);
            }
        }
    } else {
        None
    };
    let search = Search {
        hits: hits.as_deref().map(Vec::as_slice),
        in_mailbox: filter.in_mailbox.as_deref(),
        other_than: filter.other_than.as_deref(),
        has_keyword: filter.has_keyword.as_deref(),
        not_keyword: filter.not_keyword.as_deref(),
        after: filter.after,
        before: filter.before,
        has_attachment: filter.has_attachment,
        ascending,
        collapse_threads,
    };
    Ok(context.store.search(context.key, &search, window)?)
}

/// The Response arguments of one window.
fn answer(context: &Context<'_>, queried: &Queried, lowered: bool) -> Map<String, Value> {
    let mut answer = Map::new();
    answer.insert("accountId".to_owned(), Value::from(context.key.as_str()));
    answer.insert(
        "queryState".to_owned(),
        Value::from(queried.state.to_string()),
    );
    answer.insert("canCalculateChanges".to_owned(), Value::Bool(false));
    answer.insert("position".to_owned(), Value::from(queried.position));
    answer.insert(
        "ids".to_owned(),
        Value::Array(
            queried
                .ids
                .iter()
                .map(|id| Value::from(id.as_str()))
                .collect(),
        ),
    );
    if let Some(total) = queried.total {
        answer.insert("total".to_owned(), Value::from(total));
    }
    if lowered {
        answer.insert("limit".to_owned(), Value::from(QUERY_LIMIT));
    }
    answer
}

/// A search the session would refuse: the caps or a NUL, in its own
/// fixed words.
fn refused(error: &SessionError) -> MethodError {
    match error {
        SessionError::Protocol(words) => MethodError::InvalidArguments(words),
        _ => MethodError::ServerFail,
    }
}

/// What a search that found nothing to answer with comes to (RFC 8620
/// sections 3.6.2 and 5.5).
fn failed(failure: Failure) -> MethodError {
    match failure {
        Failure::Unavailable => MethodError::ServerUnavailable,
        Failure::Charset => MethodError::UnsupportedFilter(NO_CHARSET),
        Failure::Unreadable => MethodError::ServerFail,
    }
}

/// The condition the filter names, its keywords in lower case and its
/// dates in seconds. No filter is every email of the account (RFC 8620
/// section 5.5); a `FilterOperator`, an unknown property or a value of
/// the wrong type is `unsupportedFilter`, a keyword that is none or a
/// date that is no `UTCDate` `invalidArguments`.
fn filter_of(filter: Option<&Value>) -> Result<Filter, MethodError> {
    let Some(filter) = filter else {
        return Ok(Filter::default());
    };
    let condition: Condition = serde_json::from_value(filter.clone())
        .map_err(|_| MethodError::UnsupportedFilter(UNSERVED))?;
    let keyword = |named: Option<String>| match named {
        None => Ok(None),
        Some(named) => canonical_keyword(&named)
            .map(Some)
            .ok_or(MethodError::InvalidArguments(NO_KEYWORD)),
    };
    Ok(Filter {
        in_mailbox: condition.in_mailbox,
        other_than: condition.in_mailbox_other_than,
        has_keyword: keyword(condition.has_keyword)?,
        not_keyword: keyword(condition.not_keyword)?,
        text: condition.text.unwrap_or_default(),
        from: condition.from.unwrap_or_default(),
        after: seconds(condition.after.as_deref())?,
        before: seconds(condition.before.as_deref())?,
        has_attachment: condition.has_attachment,
    })
}

/// A `UTCDate` (RFC 8620 section 1.4), its offset written `Z` as the
/// section asks and never in digits, as the first whole second at or
/// past it, since `receivedAt` counts whole seconds: the same second
/// bounds `after` and `before` alike.
fn seconds(date: Option<&str>) -> Result<Option<i64>, MethodError> {
    let Some(date) = date else {
        return Ok(None);
    };
    // RFC 3339 section 5.6: the `Z` may be lower case.
    let at = OffsetDateTime::parse(date, &Rfc3339)
        .ok()
        .filter(|_| date.ends_with(['Z', 'z']))
        .ok_or(MethodError::InvalidArguments(NO_DATE))?;
    let whole = at.unix_timestamp();
    Ok(Some(if at.nanosecond() > 0 {
        whole.saturating_add(1)
    } else {
        whole
    }))
}

/// Whether the result runs oldest first: no sort or one comparator on
/// `receivedAt` without a collation; anything else is `unsupportedSort`.
fn ascending(sort: Option<&[Comparator]>) -> Result<bool, MethodError> {
    match sort {
        None | Some([]) => Ok(false),
        Some([one]) if one.property == RECEIVED_AT && one.collation.is_none() => {
            Ok(one.is_ascending)
        }
        Some(_) => Err(MethodError::UnsupportedSort),
    }
}

/// The limit the query runs with and whether it was lowered.
fn limit(asked: Option<u64>) -> (u32, bool) {
    match asked {
        Some(asked) if asked <= u64::from(QUERY_LIMIT) => {
            (u32::try_from(asked).unwrap_or(QUERY_LIMIT), false)
        }
        _ => (QUERY_LIMIT, true),
    }
}
