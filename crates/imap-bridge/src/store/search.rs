// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! One window of a search (RFC 8621 section 4.4): the emails the hits of
//! the server's SEARCH name, or every email of the account where no hits
//! are given, narrowed by the plaintext conditions and the memberships of
//! the scope, one per thread on request and ordered by `receivedAt` with
//! the id as the tiebreak. A row that left since the SEARCH drops out by
//! the join, so paging by position or by anchor costs no SEARCH.

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use rusqlite::types::ToSql;
use rusqlite::{CachedStatement, Connection};

use super::changes::state_of;
use super::{AccountKey, EmailId, MailboxId, Queried, Store, StoreError, Window};

/// The UIDs a SEARCH found in one folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderHits {
    pub folder: MailboxId,
    pub uids: Vec<u32>,
}

/// Which emails a search ranges over and in what order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Search<'a> {
    /// The hits of the server's SEARCH; `None` ranges over every email
    /// of the account.
    pub hits: Option<&'a [FolderHits]>,
    /// The mailbox every email of the result lies in.
    pub in_mailbox: Option<&'a str>,
    /// Every email of the result lies in one mailbox outside these at
    /// least (RFC 8621 section 4.4.1, `inMailboxOtherThan`).
    pub other_than: Option<&'a [String]>,
    /// The keyword every email of the result carries, in lower case.
    pub has_keyword: Option<&'a str>,
    /// The keyword no email of the result carries, in lower case.
    pub not_keyword: Option<&'a str>,
    /// The earliest second of `receivedAt` the result takes.
    pub after: Option<i64>,
    /// The result takes a `receivedAt` before this second alone.
    pub before: Option<i64>,
    pub has_attachment: Option<bool>,
    pub ascending: bool,
    /// Whether only the first email of each thread stays (RFC 8621
    /// section 4.4.3); a thread's emails outside the filter do not
    /// count.
    pub collapse_threads: bool,
}

/// The hits as a table of folder and UID, read from one JSON object of
/// a UID list per folder.
const HITS: &str = "hits (folder_id, uid) AS (
    SELECT f.key, u.value FROM json_each(:hits) f, json_each(f.value) u)";

/// The emails of the hits; CROSS JOIN fixes the join order, so every hit
/// is one lookup on the folder's UID key.
const FROM_HITS: &str = "hits h CROSS JOIN bridge_emails e
    ON e.account_key = :key AND e.folder_id = h.folder_id AND e.uid = h.uid";

/// What an email of the result passes: a membership in the scope, the
/// dates, the attachment and the keywords (RFC 8621 section 4.4.1). An
/// email without a membership is in no mailbox and stays out.
const CONDITIONS: &str = "e.account_key = :key
    AND EXISTS (SELECT 1 FROM bridge_memberships s
                WHERE s.account_key = e.account_key AND s.email_id = e.id
                  AND (:mailbox IS NULL OR s.mailbox_id = :mailbox))
    AND (:other_than IS NULL OR EXISTS (
        SELECT 1 FROM bridge_memberships o
        WHERE o.account_key = e.account_key AND o.email_id = e.id
          AND o.mailbox_id NOT IN (SELECT value FROM json_each(:other_than))))
    AND (:after IS NULL OR e.received_at >= :after)
    AND (:before IS NULL OR e.received_at < :before)
    AND (:attachment IS NULL OR e.has_attachment = :attachment)
    AND (:has_keyword IS NULL OR EXISTS (
        SELECT 1 FROM json_each(e.keywords) w WHERE w.key = :has_keyword))
    AND (:not_keyword IS NULL OR NOT EXISTS (
        SELECT 1 FROM json_each(e.keywords) w WHERE w.key = :not_keyword))";

/// Newest first, ties by id; oldest first is its exact reverse, so ties
/// fall the same way in every answer.
const NEWEST_FIRST: &str = "received_at DESC, id ASC";
const OLDEST_FIRST: &str = "received_at ASC, id DESC";

/// The rows of `kept` that stand in the result.
const KEPT: &str = "(:collapse = 0 OR rank = 1)";

impl Search<'_> {
    fn order(&self) -> &'static str {
        if self.ascending {
            OLDEST_FIRST
        } else {
            NEWEST_FIRST
        }
    }

    /// The statement that ends in `tail` over `kept`: every email that
    /// passes, ranked inside its thread in the order of the result.
    fn sql(&self, tail: &str) -> String {
        let (hits, from) = match self.hits {
            Some(_) => (format!("{HITS}, "), FROM_HITS),
            None => (String::new(), "bridge_emails e"),
        };
        let order = self.order();
        format!(
            "WITH {hits}matched AS (
                 SELECT e.id, e.thread_id, e.received_at FROM {from} WHERE {CONDITIONS}),
             kept AS (
                 SELECT id, received_at,
                        ROW_NUMBER() OVER (PARTITION BY thread_id ORDER BY {order}) AS rank
                 FROM matched)
             {tail}"
        )
    }
}

impl Store {
    /// One window of the search. `None` when the anchor is not part of
    /// the result.
    ///
    /// # Errors
    ///
    /// Returns the database error.
    pub fn search(
        &self,
        key: &AccountKey,
        search: &Search<'_>,
        window: &Window<'_>,
    ) -> Result<Option<Queried>, StoreError> {
        let hits = search.hits.map(hits_json).transpose()?;
        let other_than = search.other_than.map(serde_json::to_string).transpose()?;
        self.read(|connection| {
            let asked = Asked {
                connection,
                search,
                key: key.as_str(),
                hits: hits.as_deref(),
                other_than: other_than.as_deref(),
            };
            let total = if window.needs_total() {
                Some(asked.total()?)
            } else {
                None
            };
            let Some(position) = window.position(total, |id| asked.index_of(id))? else {
                return Ok(None);
            };
            Ok(Some(Queried {
                state: state_of(connection, key)?,
                position,
                ids: asked.ids(position, window.limit)?,
                total: total.filter(|_| window.calculate_total),
            }))
        })
    }
}

/// The hits as one JSON object: a UID list per folder id.
fn hits_json(hits: &[FolderHits]) -> Result<String, StoreError> {
    let folders: BTreeMap<&str, &[u32]> = hits
        .iter()
        .map(|found| (found.folder.as_str(), found.uids.as_slice()))
        .collect();
    Ok(serde_json::to_string(&folders)?)
}

/// One search against one connection, with the values its statements
/// bind.
struct Asked<'a> {
    connection: &'a Connection,
    search: &'a Search<'a>,
    key: &'a str,
    hits: Option<&'a str>,
    other_than: Option<&'a str>,
}

impl Asked<'_> {
    fn total(&self) -> Result<u64, StoreError> {
        let sql = self
            .search
            .sql(&format!("SELECT COUNT(*) FROM kept WHERE {KEPT}"));
        let mut statement = self.prepared(&sql, &[])?;
        let mut rows = statement.raw_query();
        let row = rows.next()?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        Ok(row.get(0)?)
    }

    /// The index of an email in the result; `None` when the result does
    /// not hold it.
    fn index_of(&self, anchor: &str) -> Result<Option<u64>, StoreError> {
        let order = self.search.order();
        let sql = self.search.sql(&format!(
            "SELECT position FROM (
                 SELECT id, ROW_NUMBER() OVER (ORDER BY {order}) - 1 AS position
                 FROM kept WHERE {KEPT})
             WHERE id = :anchor"
        ));
        let mut statement = self.prepared(&sql, &[(":anchor", &anchor)])?;
        let mut rows = statement.raw_query();
        Ok(rows.next()?.map(|row| row.get::<_, u64>(0)).transpose()?)
    }

    fn ids(&self, position: u64, limit: u32) -> Result<Vec<EmailId>, StoreError> {
        let order = self.search.order();
        let sql = self.search.sql(&format!(
            "SELECT id FROM kept WHERE {KEPT} ORDER BY {order} LIMIT :limit OFFSET :offset"
        ));
        let offset = i64::try_from(position).unwrap_or(i64::MAX);
        let mut statement = self.prepared(&sql, &[(":limit", &limit), (":offset", &offset)])?;
        let mut rows = statement.raw_query();
        let mut ids = Vec::new();
        while let Some(row) = rows.next()? {
            ids.push(row.get(0)?);
        }
        Ok(ids)
    }

    /// The statement with every value it names bound; `more` adds the
    /// values of its tail.
    fn prepared(
        &self,
        sql: &str,
        more: &[(&str, &dyn ToSql)],
    ) -> Result<CachedStatement<'_>, StoreError> {
        let search = self.search;
        let values: [(&str, &dyn ToSql); 10] = [
            (":key", &self.key),
            (":hits", &self.hits),
            (":mailbox", &search.in_mailbox),
            (":other_than", &self.other_than),
            (":after", &search.after),
            (":before", &search.before),
            (":attachment", &search.has_attachment),
            (":has_keyword", &search.has_keyword),
            (":not_keyword", &search.not_keyword),
            (":collapse", &search.collapse_threads),
        ];
        let mut statement = self.connection.prepare_cached(sql)?;
        for (name, value) in values.iter().chain(more) {
            if let Some(index) = statement.parameter_index(name)? {
                statement.raw_bind_parameter(index, value)?;
            }
        }
        Ok(statement)
    }
}
