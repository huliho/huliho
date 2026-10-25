// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The searches a client sends over the latency corpus: a text, a sender
//! and a date range alone, each over the default scope, every first page
//! checked against the messages the corpus says it holds. An indexed
//! target answers within the search bound. The Dovecot without an index
//! ends every search inside the bridge's deadline with the same messages
//! or the deadline's error, while a window and a refresh answer beside
//! it.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use huliho_imap_bridge::runtime::SEARCH_DEADLINE;
use serde_json::{Value, json};
use tokio::time::{sleep, timeout};

use super::corpus::{SEARCH_WORDS, SENDERS, Seed};
use super::{
    LIST_PROPERTIES, LOOK, Live, MEASURED_WINDOWS, Mail, Synced, Target, Window, corpus_size,
    listed, number_of,
};

/// The bound on one page of results on an indexed target: the query and
/// the get of its header properties.
const SEARCH_LATENCY_BOUND: Duration = Duration::from_secs(1);

/// How many results a page asks for, as many as a client's page holds.
const SEARCH_PAGE: usize = 100;

/// How many corpus messages each date range spans.
const DATE_SPAN: u32 = 40;

/// How long an answer may take past the bridge's deadline to come back
/// through the router before the search counts as hung.
const ANSWER_SLACK: Duration = Duration::from_secs(5);

/// How many rows the window beside a search asks for.
const BESIDE: usize = 5;

/// How long Cyrus's rolling squatter may take to index the corpus.
const INDEX_PATIENCE: Duration = Duration::from_secs(60);

/// The warm-up and the measured rounds. Every round searches something
/// new, so the bridge's kept hits never answer one.
const ROUNDS: usize = MEASURED_WINDOWS + 1;

const _: () = assert!(SEARCH_WORDS.len() >= ROUNDS && SENDERS.len() >= ROUNDS);

/// What a search asks for beside the scope.
#[derive(Debug, Clone, Copy)]
enum Kind {
    Text,
    Sender,
    Dates,
}

const KINDS: [Kind; 3] = [Kind::Text, Kind::Sender, Kind::Dates];

/// The first corpus number a round's date range holds and the first past
/// it.
fn dates(round: usize) -> (u32, u32) {
    let round = u32::try_from(round).unwrap();
    (round * DATE_SPAN + 1, (round + 1) * DATE_SPAN + 1)
}

impl Kind {
    /// The round's condition (RFC 8621 section 4.4.1).
    fn condition(self, round: usize) -> Value {
        match self {
            Self::Text => json!({ "text": SEARCH_WORDS[round] }),
            Self::Sender => json!({ "from": SENDERS[round].1 }),
            Self::Dates => {
                let (first, past) = dates(round);
                json!({ "after": Seed(first).received_at(), "before": Seed(past).received_at() })
            }
        }
    }

    /// Whether the round's search on the target finds the corpus message.
    /// Cyrus matches `before` at the instant itself as well (its
    /// `imap/jmap_mail.c` searches with less or equal), where RFC 8621
    /// section 4.4.1 asks for a time before it.
    fn finds(self, round: usize, number: u32, target: Target) -> bool {
        match self {
            Self::Text => Seed(number).word() == SEARCH_WORDS[round],
            Self::Sender => Seed(number).sender() == SENDERS[round],
            Self::Dates => {
                let (first, past) = dates(round);
                (first..past).contains(&number) || (target == Target::Cyrus && number == past)
            }
        }
    }

    /// The corpus messages the round's search answers on the target,
    /// newest first and one per thread (RFC 8620 section 5.5,
    /// `collapseThreads`).
    fn expected(self, round: usize, target: Target) -> Vec<u32> {
        let mut threads = HashSet::new();
        (1..=corpus_size())
            .rev()
            .filter(|&number| self.finds(round, number, target))
            .filter(|&number| threads.insert(Seed(number).root()))
            .collect()
    }
}

/// The mailboxes the default scope leaves out, Junk and Trash, as a
/// client names them in `inMailboxOtherThan`.
async fn excluded(live: &Live, mail: &Mail) -> Vec<String> {
    live.mailboxes(mail)
        .await
        .iter()
        .filter(|mailbox| mailbox["role"] == "junk" || mailbox["role"] == "trash")
        .map(|mailbox| mailbox["id"].as_str().unwrap().to_owned())
        .collect()
}

fn filter(kind: Kind, round: usize, excluded: &[String]) -> Value {
    let mut filter = kind.condition(round);
    filter["inMailboxOtherThan"] = json!(excluded);
    filter
}

/// A first page of results: the corpus number of each id in order and
/// the total.
struct Found {
    numbers: Vec<Option<u32>>,
    total: u64,
}

/// A search as a client sends it, the query and then the header
/// properties of its ids, with how long the two took; a method error
/// answers as its arguments. A search that outlasts the bridge's
/// deadline fails the test.
async fn searched(live: &Live, mail: &Mail, filter: Value) -> (Result<Found, Value>, Duration) {
    let arguments = json!({
        "accountId": mail.account,
        "filter": filter,
        "sort": [{ "property": "receivedAt", "isAscending": false }],
        "position": 0,
        "limit": SEARCH_PAGE,
        "calculateTotal": true,
        "collapseThreads": true,
    });
    let started = Instant::now();
    let page = async {
        let (name, answer) = live.answer(mail, "Email/query", arguments).await;
        if name != "Email/query" {
            return Err(answer);
        }
        let ids = listed(&answer, "ids");
        let emails = live.emails(mail, &ids, &LIST_PROPERTIES).await;
        assert_eq!(emails.len(), ids.len(), "{answer}");
        Ok((ids, answer["total"].as_u64().unwrap()))
    };
    let answered = timeout(SEARCH_DEADLINE + ANSWER_SLACK, page)
        .await
        .expect("the search ends inside the deadline");
    let took = started.elapsed();
    let (ids, total) = match answered {
        Ok(page) => page,
        Err(error) => return (Err(error), took),
    };
    let numbers: HashMap<String, Option<u32>> = live
        .emails(mail, &ids, &["id", "messageId"])
        .await
        .iter()
        .map(|email| (email["id"].as_str().unwrap().to_owned(), number_of(email)))
        .collect();
    let numbers = ids.iter().map(|id| numbers[id]).collect();
    (Ok(Found { numbers, total }), took)
}

/// The page holds the corpus messages the search has to find on the
/// target in order, with a total that counts all of them.
fn check(kind: Kind, round: usize, target: Target, found: &Found) {
    let expected = kind.expected(round, target);
    let page: Vec<Option<u32>> = expected
        .iter()
        .take(SEARCH_PAGE)
        .copied()
        .map(Some)
        .collect();
    assert_eq!(found.numbers, page, "{kind:?} round {round}");
    assert_eq!(
        found.total,
        u64::try_from(expected.len()).unwrap(),
        "{kind:?} round {round}"
    );
}

/// Waits until a search finds the newest corpus message. Cyrus's JMAP
/// search reads its Xapian index alone and leaves out a message the
/// rolling squatter has not reached yet (`imap/search_xapian.c`), and the
/// squatter indexes a mailbox in the order of its UIDs.
async fn indexed(live: &Live, mail: &Mail) {
    let newest = corpus_size();
    let until = Instant::now() + INDEX_PATIENCE;
    loop {
        let filter = json!({ "text": Seed(newest).word() });
        let (answered, _) = searched(live, mail, filter).await;
        if answered.is_ok_and(|found| found.numbers.first() == Some(&Some(newest))) {
            return;
        }
        assert!(Instant::now() < until, "message {newest} is not indexed");
        sleep(LOOK).await;
    }
}

/// Every search against an indexed target answers its corpus messages,
/// each measured round within the bound.
pub async fn within_the_bound(target: Target) {
    let synced = Synced::start(target).await;
    if target == Target::Cyrus {
        indexed(&synced.live, &synced.mail).await;
    }
    let excluded = excluded(&synced.live, &synced.mail).await;
    for kind in KINDS {
        for round in 0..ROUNDS {
            let filter = filter(kind, round, &excluded);
            let (answered, took) = searched(&synced.live, &synced.mail, filter).await;
            let found = answered.unwrap_or_else(|error| panic!("{kind:?} round {round}: {error}"));
            check(kind, round, target, &found);
            assert!(
                round == 0 || took <= SEARCH_LATENCY_BOUND,
                "{kind:?} round {round} took {took:?}"
            );
        }
    }
    synced.leave().await;
}

/// Every search against the Dovecot without an index ends inside the
/// deadline with the same corpus messages or the deadline's error, while
/// a window of the inbox and a refresh answer beside it.
pub async fn inside_the_deadline() {
    let synced = Synced::start(Target::DovecotPlain).await;
    let (live, mail) = (&synced.live, &synced.mail);
    let excluded = excluded(live, mail).await;
    let window = Window {
        mailbox: &synced.inbox,
        position: 0,
        limit: BESIDE,
        collapse: true,
        total: false,
    };
    for kind in KINDS {
        for round in 0..ROUNDS {
            let since = live.state(mail).await;
            let beside = async {
                let rows = live.query(mail, window).await;
                let arguments = json!({ "accountId": mail.account, "sinceState": &since });
                (rows, live.call(mail, "Email/changes", arguments).await)
            };
            let search = searched(live, mail, filter(kind, round, &excluded));
            let ((answered, _), (rows, changes)) = tokio::join!(search, beside);
            match answered {
                Ok(found) => check(kind, round, Target::DovecotPlain, &found),
                Err(error) => assert_eq!(error["type"], "serverUnavailable", "{error}"),
            }
            assert_eq!(listed(&rows, "ids").len(), BESIDE, "{rows}");
            assert_eq!(changes["oldState"], since, "{changes}");
        }
    }
    synced.leave().await;
}
