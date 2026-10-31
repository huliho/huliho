// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The searches of one account: a connection of their own, one search at
//! a time, the folders of a plan in turn under one deadline and the hits
//! of the last few kept in memory until the bridge moves mail, so the
//! pages after the first cost no SEARCH. A search that ends early, at the
//! deadline or because its request went away, drops the connection with
//! it.

use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use time::Date;
use tokio::time::{Instant, timeout_at};

use super::{Connector, Timing, Wire};
use crate::session::{GmailSearch, SearchAsk, Searching, Session, SessionError};
use crate::store::{FolderHits, MailboxId};
use crate::sync::Cache;

/// How long the hits of one search answer the pages after it, counted
/// from the end of the search that read them.
pub const SEARCH_CACHE_TTL: Duration = Duration::from_mins(5);

/// The searches whose hits one account keeps; the oldest leaves first.
pub const SEARCH_CACHE_ENTRIES: usize = 4;

/// One folder a plan searches, as the store held it when the request
/// came: its wire name, the UIDVALIDITY its rows stand under and, on a
/// Gmail account, the label a mailbox scope adds.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Folder {
    pub id: MailboxId,
    pub imap_name: String,
    pub uid_validity: u32,
    pub label: Option<String>,
}

/// What one search asks of the server: the text and the sender as
/// typed, the coarse keys and the folders in the order they run. Two
/// equal plans find the same hits, so a plan is what the hits are kept
/// under; a folder renumbered since makes another plan.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Plan {
    pub text: String,
    pub from: String,
    pub unseen: bool,
    pub flagged: bool,
    pub since: Option<Date>,
    pub before: Option<Date>,
    /// The account is a Gmail account, as its rows tell: the text and
    /// the sender go as one X-GM-RAW string.
    pub gmail: bool,
    pub folders: Vec<Folder>,
}

/// Prints the shape of the plan and neither the text, the sender nor a
/// mailbox name, so a plan in a log line carries nothing personal.
impl fmt::Debug for Plan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let labeled = self.folders.iter().filter(|folder| folder.label.is_some());
        f.debug_struct("Plan")
            .field("text", &!self.text.is_empty())
            .field("from", &!self.from.is_empty())
            .field("unseen", &self.unseen)
            .field("flagged", &self.flagged)
            .field("since", &self.since)
            .field("before", &self.before)
            .field("gmail", &self.gmail)
            .field("folders", &self.folders.len())
            .field("labeled", &labeled.count())
            .finish()
    }
}

/// The UIDs one search found, per folder it ran in.
pub(crate) type Hits = Arc<Vec<FolderHits>>;

/// Why a search has no hits to answer with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Failure {
    /// The deadline passed, the server answered NO or it could not be
    /// reached.
    Unavailable,
    /// The server cannot search in UTF-8 (RFC 3501 section 7.1).
    Charset,
    /// The server answered in a way the bridge cannot read.
    Unreadable,
}

/// One search kept: its plan, its hits and when the search ended.
struct Entry {
    plan: Plan,
    hits: Hits,
    read_at: Instant,
}

/// The searches kept and how often they were forgotten, so a search
/// that ran across a forget keeps nothing.
#[derive(Default)]
struct Kept {
    entries: Vec<Entry>,
    forgets: u64,
}

/// The connection the searches of one account run on, one at a time,
/// and the hits of the last ones.
pub(crate) struct Searches<C: Connector> {
    wire: tokio::sync::Mutex<Wire<C>>,
    kept: Mutex<Kept>,
    deadline: Duration,
}

impl<C: Connector> Searches<C> {
    pub(super) fn new(connector: C, timing: Timing) -> Self {
        Self {
            wire: tokio::sync::Mutex::new(Wire::new(connector, timing)),
            kept: Mutex::new(Kept::default()),
            deadline: timing.search,
        }
    }

    /// Drops every search kept. A move gives the mail it moved other
    /// UIDs, which the kept hits do not name.
    pub(crate) fn forget(&self) {
        let mut kept = self.kept();
        kept.entries.clear();
        kept.forgets += 1;
    }

    /// The hits of the plan: those of an equal plan read inside the TTL,
    /// else those of a fresh search, kept for the pages after it. The
    /// searches of one round of a request share `until`, so a search
    /// that waits behind another of the account spends its own time
    /// waiting and one that finds the deadline spent connects nowhere.
    /// A plan without folders finds nothing and connects nowhere.
    pub(crate) async fn found(
        &self,
        cache: &Cache,
        plan: &Plan,
        until: Instant,
    ) -> Result<Hits, Failure> {
        if let Some(hits) = self.kept_for(plan) {
            return Ok(hits);
        }
        if plan.folders.is_empty() {
            return Ok(Hits::default());
        }
        // A free lock resolves before the timer looks at the clock.
        if Instant::now() >= until {
            return Err(Failure::Unavailable);
        }
        let Ok(mut wire) = timeout_at(until, self.wire.lock()).await else {
            return Err(Failure::Unavailable);
        };
        // The search ahead may have read the same plan.
        if let Some(hits) = self.kept_for(plan) {
            return Ok(hits);
        }
        let Ok(Ok(mut session)) = timeout_at(until, wire.take(cache)).await else {
            return Err(Failure::Unavailable);
        };
        let searching = wire.searching;
        let forgets = self.kept().forgets;
        let outcome = timeout_at(
            until,
            searched(&mut session, cache, plan, (searching, until)),
        )
        .await;
        let found = match outcome {
            Ok(Ok(found)) => found,
            Ok(Err(error)) => {
                let (failure, stands) = failure(&error);
                if stands {
                    wire.put(session);
                }
                tracing::debug!(account = cache.key.as_str(), %error, "a search failed");
                return Err(failure);
            }
            // The stream may hold the rest of an answer, so the session goes.
            Err(_elapsed) => return Err(Failure::Unavailable),
        };
        wire.put(session);
        let hits = Arc::new(found);
        self.keep(plan, &hits, forgets);
        Ok(hits)
    }

    /// What the searches of one round of a request may take together.
    pub(crate) fn deadline(&self) -> Duration {
        self.deadline
    }

    /// Logs the search connection out once nothing used it for `bound`;
    /// one a search holds is in use.
    pub(super) fn close_idle(&self, bound: Duration) -> Duration {
        match self.wire.try_lock() {
            Ok(mut wire) => wire.close_idle(bound),
            Err(_held) => bound,
        }
    }

    /// The hits an equal plan read inside the TTL.
    fn kept_for(&self, plan: &Plan) -> Option<Hits> {
        self.kept_at(plan, Instant::now())
    }

    /// The same at `now`; the entries past the TTL leave on the way.
    fn kept_at(&self, plan: &Plan, now: Instant) -> Option<Hits> {
        let mut kept = self.kept();
        kept.entries
            .retain(|entry| now.saturating_duration_since(entry.read_at) < SEARCH_CACHE_TTL);
        let entry = kept.entries.iter().find(|entry| entry.plan == *plan)?;
        Some(Arc::clone(&entry.hits))
    }

    /// Keeps the hits of a fresh search in place of an equal plan's,
    /// the oldest entry leaving past `SEARCH_CACHE_ENTRIES`; nothing when
    /// a forget came since the search began, as `forgets` counted then.
    fn keep(&self, plan: &Plan, hits: &Hits, forgets: u64) {
        let mut kept = self.kept();
        if kept.forgets != forgets {
            return;
        }
        kept.entries.retain(|entry| entry.plan != *plan);
        if kept.entries.len() >= SEARCH_CACHE_ENTRIES {
            kept.entries.remove(0);
        }
        kept.entries.push(Entry {
            plan: plan.clone(),
            hits: Arc::clone(hits),
            read_at: Instant::now(),
        });
    }

    fn kept(&self) -> MutexGuard<'_, Kept> {
        self.kept.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Every folder of the plan in turn: EXAMINE, then the search under
/// the deadline. A folder the server refuses or shows under another
/// UIDVALIDITY than its rows is passed over, since its hits would name
/// no row; the count goes to the debug log without a name.
async fn searched<S: Session>(
    session: &mut S,
    cache: &Cache,
    plan: &Plan,
    (searching, until): (Searching, Instant),
) -> Result<Vec<FolderHits>, SessionError> {
    let mut found = Vec::with_capacity(plan.folders.len());
    let mut passed = 0_usize;
    for folder in &plan.folders {
        match session.examine(&folder.imap_name).await {
            Ok(selected) if selected.uid_validity == folder.uid_validity => {}
            Ok(_) | Err(SessionError::Refused) => {
                passed += 1;
                continue;
            }
            Err(other) => return Err(other),
        }
        let ask = SearchAsk {
            text: &plan.text,
            from: &plan.from,
            unseen: plan.unseen,
            flagged: plan.flagged,
            since: plan.since,
            before: plan.before,
            gmail: plan.gmail.then_some(GmailSearch {
                label: folder.label.as_deref(),
            }),
            server: searching,
            until: Some(until),
        };
        let uids = session.uid_search(&ask).await?;
        found.push(FolderHits {
            folder: folder.id.clone(),
            uids,
        });
    }
    if passed > 0 {
        tracing::debug!(
            account = cache.key.as_str(),
            passed,
            "a search passed over folders it could not open"
        );
    }
    Ok(found)
}

/// What a failed search comes to and whether its session stands: a NO
/// keeps it, the charset's refusal with it; an answer that cannot be
/// read costs it, as does anything that left the server unreached.
fn failure(error: &SessionError) -> (Failure, bool) {
    match error {
        SessionError::BadCharset => (Failure::Charset, true),
        SessionError::Refused => (Failure::Unavailable, true),
        SessionError::Protocol(_) => (Failure::Unreadable, false),
        _ => (Failure::Unavailable, false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TestConnector;

    fn plan(text: &str) -> Plan {
        Plan {
            text: text.to_owned(),
            from: "anna.jansen@example.org".to_owned(),
            unseen: true,
            flagged: false,
            since: None,
            before: None,
            gmail: true,
            folders: vec![Folder {
                id: MailboxId::from("m1".to_owned()),
                imap_name: "Receipts".to_owned(),
                uid_validity: 7,
                label: Some("Receipts".to_owned()),
            }],
        }
    }

    #[test]
    fn debug_output_prints_the_shape_and_neither_a_term_nor_a_mailbox_name() {
        let printed = format!("{:?}", plan("xylophone"));
        for word in ["xylophone", "anna", "example", "Receipts"] {
            assert!(!printed.contains(word), "{printed}");
        }
        assert_eq!(
            printed,
            "Plan { text: true, from: true, unseen: true, flagged: false, since: None, \
             before: None, gmail: true, folders: 1, labeled: 1 }"
        );
    }

    fn hits() -> Hits {
        Arc::new(vec![FolderHits {
            folder: MailboxId::from("m1".to_owned()),
            uids: vec![3, 1],
        }])
    }

    #[test]
    fn the_hits_answer_an_equal_plan_inside_the_ttl_and_leave_past_it() {
        let searches = Searches::new(TestConnector::Refusing, Timing::default());
        let hits = hits();
        let read = Instant::now();
        searches.keep(&plan("plan"), &hits, 0);
        let inside = read + SEARCH_CACHE_TTL - Duration::from_secs(1);
        assert_eq!(
            searches.kept_at(&plan("plan"), inside),
            Some(Arc::clone(&hits))
        );
        assert_eq!(searches.kept_at(&plan("zoë"), inside), None);
        let past = read + SEARCH_CACHE_TTL + Duration::from_secs(1);
        assert_eq!(searches.kept_at(&plan("plan"), past), None);
        assert_eq!(
            searches.kept_at(&plan("plan"), inside),
            None,
            "gone for good"
        );
    }

    #[test]
    fn a_forget_drops_the_hits_and_a_search_begun_before_it_keeps_none() {
        let searches = Searches::new(TestConnector::Refusing, Timing::default());
        let hits = hits();
        searches.keep(&plan("plan"), &hits, 0);
        assert_eq!(searches.kept_for(&plan("plan")), Some(Arc::clone(&hits)));
        searches.forget();
        assert_eq!(searches.kept_for(&plan("plan")), None);
        searches.keep(&plan("plan"), &hits, 0);
        assert_eq!(
            searches.kept_for(&plan("plan")),
            None,
            "read before the forget"
        );
        searches.keep(&plan("plan"), &hits, 1);
        assert_eq!(searches.kept_for(&plan("plan")), Some(hits));
    }

    #[test]
    fn a_refusal_keeps_the_session_and_an_unreadable_answer_costs_it() {
        assert_eq!(failure(&SessionError::BadCharset), (Failure::Charset, true));
        assert_eq!(
            failure(&SessionError::Refused),
            (Failure::Unavailable, true)
        );
        assert_eq!(
            failure(&SessionError::Protocol("x")),
            (Failure::Unreadable, false)
        );
        for gone in [SessionError::Timeout, SessionError::Closed] {
            assert_eq!(failure(&gone), (Failure::Unavailable, false));
        }
    }
}
