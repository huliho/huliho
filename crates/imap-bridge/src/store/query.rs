// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! One window of the emails of a mailbox by `receivedAt` (RFC 8621
//! section 4.4), answered from the memberships. Newest first is the
//! order of the window index; oldest first is its exact reverse, so
//! both walk the index and ties fall the same way in every answer.

use rusqlite::{Connection, OptionalExtension, params};

use super::changes::state_of;
use super::{AccountKey, EmailId, Store, StoreError};

/// Which emails a query ranges over and in what order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Query<'a> {
    pub mailbox: &'a str,
    pub ascending: bool,
    /// Whether only the first email of each thread stays (RFC 8621
    /// section 4.4.3).
    pub collapse_threads: bool,
}

/// Where a window starts (RFC 8620 section 5.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start<'a> {
    /// An index into the result; a negative one counts from its end.
    Position(i64),
    /// The index of this email in the result plus the offset.
    Anchor { id: &'a str, offset: i64 },
}

/// The window asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window<'a> {
    pub start: Start<'a>,
    pub limit: u32,
    pub calculate_total: bool,
}

/// One window of a result, read under one lock so the state belongs to
/// it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Queried {
    pub state: u64,
    /// The index of the first id in the whole result.
    pub position: u64,
    pub ids: Vec<EmailId>,
    pub total: Option<u64>,
}

/// The SQL of one direction.
struct Direction {
    /// Whether the membership `n` sorts ahead of `m`.
    ahead: &'static str,
    /// Whether `m` sorts ahead of the anchor at `?4` and `?5`.
    before_anchor: &'static str,
    order: &'static str,
}

const NEWEST_FIRST: Direction = Direction {
    ahead: "(n.received_at > m.received_at
             OR (n.received_at = m.received_at AND n.email_id < m.email_id))",
    before_anchor: "(m.received_at > ?4 OR (m.received_at = ?4 AND m.email_id < ?5))",
    order: "m.received_at DESC, m.email_id ASC",
};

const OLDEST_FIRST: Direction = Direction {
    ahead: "(n.received_at < m.received_at
             OR (n.received_at = m.received_at AND n.email_id > m.email_id))",
    before_anchor: "(m.received_at < ?4 OR (m.received_at = ?4 AND m.email_id > ?5))",
    order: "m.received_at ASC, m.email_id DESC",
};

impl Direction {
    /// The rows of the result: the memberships of the mailbox at `?2`
    /// and, with `?3` set, only those no email of the same thread in
    /// that mailbox sorts ahead of.
    fn rows(&self) -> String {
        format!(
            "FROM bridge_memberships m
             WHERE m.account_key = ?1 AND m.mailbox_id = ?2
               AND (?3 = 0 OR NOT EXISTS (
                   SELECT 1 FROM bridge_emails e
                   JOIN bridge_emails o
                     ON o.account_key = e.account_key AND o.thread_id = e.thread_id
                   JOIN bridge_memberships n
                     ON n.account_key = o.account_key AND n.email_id = o.id
                    AND n.mailbox_id = m.mailbox_id
                   WHERE e.account_key = m.account_key AND e.id = m.email_id
                     AND {}))",
            self.ahead
        )
    }
}

impl Store {
    /// One window of the query. `None` when the anchor is not part of
    /// the result.
    ///
    /// # Errors
    ///
    /// Returns the database error.
    pub fn query(
        &self,
        key: &AccountKey,
        query: &Query<'_>,
        window: &Window<'_>,
    ) -> Result<Option<Queried>, StoreError> {
        self.read(|connection| {
            let direction = if query.ascending {
                &OLDEST_FIRST
            } else {
                &NEWEST_FIRST
            };
            let scope = Scope {
                connection,
                key,
                query,
                direction,
            };
            let needs_total = window.calculate_total
                || matches!(window.start, Start::Position(index) if index < 0);
            let total = if needs_total {
                Some(scope.total()?)
            } else {
                None
            };
            let position = match window.start {
                Start::Position(index) => match u64::try_from(index) {
                    Ok(index) => index,
                    Err(_) => total.unwrap_or(0).saturating_sub(index.unsigned_abs()),
                },
                Start::Anchor { id, offset } => {
                    let Some(index) = scope.index_of(id)? else {
                        return Ok(None);
                    };
                    if offset < 0 {
                        index.saturating_sub(offset.unsigned_abs())
                    } else {
                        index.saturating_add(offset.unsigned_abs())
                    }
                }
            };
            Ok(Some(Queried {
                state: state_of(connection, key)?,
                position,
                ids: scope.ids(position, window.limit)?,
                total: total.filter(|_| window.calculate_total),
            }))
        })
    }
}

/// One query against one connection.
struct Scope<'a> {
    connection: &'a Connection,
    key: &'a AccountKey,
    query: &'a Query<'a>,
    direction: &'a Direction,
}

impl Scope<'_> {
    fn total(&self) -> Result<u64, StoreError> {
        let sql = format!("SELECT COUNT(*) {}", self.direction.rows());
        Ok(self.connection.query_row(
            &sql,
            params![
                self.key.as_str(),
                self.query.mailbox,
                self.query.collapse_threads
            ],
            |row| row.get(0),
        )?)
    }

    /// The index of an email in the result; `None` when the result does
    /// not hold it.
    fn index_of(&self, anchor: &str) -> Result<Option<u64>, StoreError> {
        let rows = self.direction.rows();
        let received_at: Option<i64> = self
            .connection
            .query_row(
                &format!("SELECT m.received_at {rows} AND m.email_id = ?4"),
                params![
                    self.key.as_str(),
                    self.query.mailbox,
                    self.query.collapse_threads,
                    anchor
                ],
                |row| row.get(0),
            )
            .optional()?;
        let Some(received_at) = received_at else {
            return Ok(None);
        };
        let sql = format!(
            "SELECT COUNT(*) {rows} AND {}",
            self.direction.before_anchor
        );
        let index = self.connection.query_row(
            &sql,
            params![
                self.key.as_str(),
                self.query.mailbox,
                self.query.collapse_threads,
                received_at,
                anchor
            ],
            |row| row.get(0),
        )?;
        Ok(Some(index))
    }

    fn ids(&self, position: u64, limit: u32) -> Result<Vec<EmailId>, StoreError> {
        let sql = format!(
            "SELECT m.email_id {} ORDER BY {} LIMIT ?4 OFFSET ?5",
            self.direction.rows(),
            self.direction.order
        );
        let mut statement = self.connection.prepare_cached(&sql)?;
        let ids = statement
            .query_map(
                params![
                    self.key.as_str(),
                    self.query.mailbox,
                    self.query.collapse_threads,
                    limit,
                    i64::try_from(position).unwrap_or(i64::MAX)
                ],
                |row| row.get(0),
            )?
            .collect::<Result<_, _>>()?;
        Ok(ids)
    }
}
