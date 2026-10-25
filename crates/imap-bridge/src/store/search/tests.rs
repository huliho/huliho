// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The SQL of a search against a model of what RFC 8621 section 4.4
//! asks, over generated rows, hits and conditions, with the plan the
//! join takes.

use proptest::prelude::*;
use rusqlite::params;

use super::*;
use crate::store::Start;

const ACCOUNT: &str = "a1";

/// The folders, threads and mailboxes the generated rows draw from; the
/// last mailbox holds nothing.
const FOLDERS: [&str; 3] = ["f0", "f1", "f2"];
const THREADS: [&str; 4] = ["t0", "t1", "t2", "t3"];
const MAILBOXES: [&str; 5] = ["m0", "m1", "m2", "m3", "m4"];
const KEYWORDS: [&str; 2] = ["$seen", "$flagged"];

/// The seconds a generated `receivedAt` falls in, few so ties happen.
const SECONDS: i64 = 6;

/// One generated row with its memberships.
#[derive(Debug, Clone)]
struct Row {
    id: String,
    folder: &'static str,
    uid: u32,
    thread: &'static str,
    received_at: i64,
    has_attachment: bool,
    keywords: Vec<&'static str>,
    mailboxes: Vec<&'static str>,
}

/// One generated search in owned form.
#[derive(Debug, Clone)]
struct Asked {
    hits: Option<Vec<FolderHits>>,
    in_mailbox: Option<&'static str>,
    other_than: Option<Vec<String>>,
    has_keyword: Option<&'static str>,
    not_keyword: Option<&'static str>,
    after: Option<i64>,
    before: Option<i64>,
    has_attachment: Option<bool>,
    ascending: bool,
    collapse_threads: bool,
}

impl Asked {
    fn search(&self) -> Search<'_> {
        Search {
            hits: self.hits.as_deref(),
            in_mailbox: self.in_mailbox,
            other_than: self.other_than.as_deref(),
            has_keyword: self.has_keyword,
            not_keyword: self.not_keyword,
            after: self.after,
            before: self.before,
            has_attachment: self.has_attachment,
            ascending: self.ascending,
            collapse_threads: self.collapse_threads,
        }
    }

    /// The ids RFC 8621 section 4.4 answers for the rows, in order.
    fn model(&self, rows: &[Row]) -> Vec<String> {
        let hit = |row: &Row| {
            self.hits.as_ref().is_none_or(|hits| {
                hits.iter().any(|found| {
                    found.folder.as_str() == row.folder && found.uids.contains(&row.uid)
                })
            })
        };
        let in_scope = |row: &Row| {
            !row.mailboxes.is_empty()
                && self
                    .in_mailbox
                    .is_none_or(|named| row.mailboxes.contains(&named))
                && self.other_than.as_ref().is_none_or(|left_out| {
                    row.mailboxes
                        .iter()
                        .any(|mailbox| !left_out.iter().any(|left| left == mailbox))
                })
        };
        let passes = |row: &Row| {
            self.after.is_none_or(|after| row.received_at >= after)
                && self.before.is_none_or(|before| row.received_at < before)
                && self
                    .has_attachment
                    .is_none_or(|wanted| row.has_attachment == wanted)
                && self
                    .has_keyword
                    .is_none_or(|word| row.keywords.contains(&word))
                && self
                    .not_keyword
                    .is_none_or(|word| !row.keywords.contains(&word))
        };
        let mut matched: Vec<&Row> = rows
            .iter()
            .filter(|row| hit(row) && in_scope(row) && passes(row))
            .collect();
        matched.sort_by(|a, b| {
            let newest = b.received_at.cmp(&a.received_at).then(a.id.cmp(&b.id));
            if self.ascending {
                newest.reverse()
            } else {
                newest
            }
        });
        let mut seen = Vec::new();
        let mut ids = Vec::new();
        for row in matched {
            if self.collapse_threads && seen.contains(&row.thread) {
                continue;
            }
            seen.push(row.thread);
            ids.push(row.id.clone());
        }
        ids
    }
}

fn stored(rows: &[Row]) -> Store {
    let store = Store::in_memory().unwrap();
    let key = AccountKey::new(ACCOUNT);
    store
        .write(&key, |transaction| {
            transaction.execute(
                "INSERT INTO bridge_state (account_key, sequence) VALUES (?1, 7)",
                [ACCOUNT],
            )?;
            for row in rows {
                let keywords: BTreeMap<&str, bool> =
                    row.keywords.iter().map(|word| (*word, true)).collect();
                transaction.execute(
                    "INSERT INTO bridge_emails (account_key, id, folder_id, uid, thread_id, keywords,
                         size, received_at, has_attachment, sealed)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?8, X'00')",
                    params![
                        ACCOUNT,
                        row.id,
                        row.folder,
                        row.uid,
                        row.thread,
                        serde_json::to_string(&keywords)?,
                        row.received_at,
                        row.has_attachment
                    ],
                )?;
                for mailbox in &row.mailboxes {
                    transaction.execute(
                        "INSERT INTO bridge_memberships (account_key, email_id, mailbox_id, received_at)
                         VALUES (?1, ?2, ?3, ?4)",
                        params![ACCOUNT, row.id, mailbox, row.received_at],
                    )?;
                }
            }
            Ok(())
        })
        .unwrap();
    store
}

fn window(start: Start<'_>, limit: u32) -> Window<'_> {
    Window {
        start,
        limit,
        calculate_total: true,
    }
}

fn rows() -> impl Strategy<Value = Vec<Row>> {
    let row = (
        0..FOLDERS.len(),
        0..THREADS.len(),
        0..SECONDS,
        any::<bool>(),
        prop::sample::subsequence(KEYWORDS.to_vec(), 0..=KEYWORDS.len()),
        prop::sample::subsequence(MAILBOXES[..4].to_vec(), 0..=2),
    );
    prop::collection::vec(row, 0..14).prop_map(|rows| {
        rows.into_iter()
            .enumerate()
            .map(
                |(index, (folder, thread, received_at, has_attachment, keywords, mailboxes))| Row {
                    id: format!("e{index:02}"),
                    folder: FOLDERS[folder],
                    uid: u32::try_from(index).unwrap() + 1,
                    thread: THREADS[thread],
                    received_at,
                    has_attachment,
                    keywords,
                    mailboxes,
                },
            )
            .collect()
    })
}

fn asked() -> impl Strategy<Value = Asked> {
    let hits = prop::option::of(prop::collection::vec(
        (0..FOLDERS.len(), prop::collection::vec(1..18_u32, 0..8)),
        0..3,
    ));
    let keyword = prop::option::of(prop::sample::select(KEYWORDS.to_vec()));
    let mailbox = prop::option::of(prop::sample::select(MAILBOXES.to_vec()));
    let other_than = prop::option::of(prop::sample::subsequence(
        MAILBOXES.to_vec(),
        0..=MAILBOXES.len(),
    ));
    let second = prop::option::of(0..=SECONDS);
    (
        (hits, mailbox, other_than),
        (keyword.clone(), keyword, second.clone(), second),
        (
            prop::option::of(any::<bool>()),
            any::<bool>(),
            any::<bool>(),
        ),
    )
        .prop_map(
            |(
                (hits, in_mailbox, other_than),
                (has_keyword, not_keyword, after, before),
                (has_attachment, ascending, collapse_threads),
            )| Asked {
                hits: hits.map(|folders| {
                    let mut by_folder: BTreeMap<usize, Vec<u32>> = BTreeMap::new();
                    for (folder, mut uids) in folders {
                        let held = by_folder.entry(folder).or_default();
                        held.append(&mut uids);
                        held.sort_unstable();
                        held.dedup();
                    }
                    by_folder
                        .into_iter()
                        .map(|(folder, uids)| FolderHits {
                            folder: MailboxId::from(FOLDERS[folder].to_owned()),
                            uids,
                        })
                        .collect()
                }),
                in_mailbox,
                other_than: other_than.map(|ids| ids.into_iter().map(str::to_owned).collect()),
                has_keyword,
                not_keyword,
                after,
                before,
                has_attachment,
                ascending,
                collapse_threads,
            },
        )
}

proptest! {
    #[test]
    fn a_search_answers_what_the_model_of_rfc8621_4_4_answers_by_position_and_anchor(
        rows in rows(),
        asked in asked(),
        start in 0..6_i64,
        limit in 1..6_u32,
    ) {
        let store = stored(&rows);
        let key = AccountKey::new(ACCOUNT);
        let expected = asked.model(&rows);
        let search = asked.search();
        let whole = store.search(&key, &search, &window(Start::Position(0), 100)).unwrap().unwrap();
        let ids: Vec<String> = whole.ids.iter().map(ToString::to_string).collect();
        prop_assert_eq!(&ids, &expected);
        prop_assert_eq!(whole.total, Some(u64::try_from(expected.len()).unwrap()));
        prop_assert_eq!(whole.state, 7);
        let paged = store.search(&key, &search, &window(Start::Position(start), limit)).unwrap().unwrap();
        let from = usize::try_from(start).unwrap().min(expected.len());
        let to = (from + usize::try_from(limit).unwrap()).min(expected.len());
        let page: Vec<String> = paged.ids.iter().map(ToString::to_string).collect();
        prop_assert_eq!(&page[..], &expected[from..to]);
        let from_end = store.search(&key, &search, &window(Start::Position(-2), limit)).unwrap().unwrap();
        prop_assert_eq!(from_end.position, u64::try_from(expected.len().saturating_sub(2)).unwrap());
        for (index, anchor) in expected.iter().enumerate() {
            let at = Start::Anchor { id: anchor, offset: -1 };
            let anchored = store.search(&key, &search, &window(at, limit)).unwrap().unwrap();
            prop_assert_eq!(anchored.position, u64::try_from(index.saturating_sub(1)).unwrap());
        }
        let outside = rows.iter().find(|row| !expected.contains(&row.id));
        if let Some(row) = outside {
            let at = Start::Anchor { id: &row.id, offset: 0 };
            prop_assert_eq!(store.search(&key, &search, &window(at, limit)).unwrap(), None);
        }
    }
}

/// The steps of the plan of a search over hits on a fresh database,
/// where the planner has no statistics to go by.
fn plan_of(search: &Search<'_>) -> Vec<String> {
    let store = Store::in_memory().unwrap();
    let sql = format!("EXPLAIN QUERY PLAN {}", search.sql("SELECT id FROM kept"));
    store
        .read(|connection| {
            let mut statement = connection.prepare(&sql)?;
            let mut steps = Vec::new();
            let mut rows = statement.raw_query();
            while let Some(row) = rows.next()? {
                steps.push(row.get::<_, String>(3)?);
            }
            Ok(steps)
        })
        .unwrap()
}

#[test]
fn every_hit_is_one_lookup_on_the_folder_s_uid_key_and_the_memberships_on_theirs() {
    let hits = [FolderHits {
        folder: MailboxId::from("f0".to_owned()),
        uids: vec![1],
    }];
    let search = Search {
        hits: Some(&hits),
        in_mailbox: Some("m0"),
        other_than: None,
        has_keyword: None,
        not_keyword: None,
        after: None,
        before: None,
        has_attachment: None,
        ascending: false,
        collapse_threads: true,
    };
    let steps = plan_of(&search);
    let step = |table: &str| {
        steps
            .iter()
            .find(|step| step.starts_with(&format!("SEARCH {table} ")))
            .unwrap_or_else(|| panic!("{steps:?}"))
    };
    assert!(
        step("e").contains("sqlite_autoindex_bridge_emails_2"),
        "{steps:?}"
    );
    assert!(
        step("s").contains("sqlite_autoindex_bridge_memberships_1"),
        "{steps:?}"
    );
}
