// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What the memberships say about each mailbox, and which mailboxes take
//! their counts from them.

use std::collections::{HashMap, HashSet};

use rusqlite::{Connection, params};

use super::Counts;
use crate::store::{AccountKey, MailboxId, StoreError};

/// The counts of RFC 8621 section 2: a thread is unread while one of its
/// emails carries neither `$seen` nor `$draft`.
pub(super) fn read_counts(
    connection: &Connection,
    key: &AccountKey,
) -> Result<HashMap<MailboxId, Counts>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT m.mailbox_id, COUNT(*), COUNT(DISTINCT e.thread_id),
                COUNT(DISTINCT CASE
                    WHEN json_extract(e.keywords, '$.\"$seen\"') IS NULL
                     AND json_extract(e.keywords, '$.\"$draft\"') IS NULL
                    THEN e.thread_id END),
                COUNT(CASE
                    WHEN json_extract(e.keywords, '$.\"$seen\"') IS NULL
                     AND json_extract(e.keywords, '$.\"$draft\"') IS NULL
                    THEN 1 END)
         FROM bridge_memberships m
         JOIN bridge_emails e ON e.account_key = m.account_key AND e.id = m.email_id
         WHERE m.account_key = ?1
         GROUP BY m.mailbox_id",
    )?;
    let counts = statement
        .query_map([key.as_str()], |row| {
            Ok((
                row.get::<_, MailboxId>(0)?,
                Counts {
                    synced_emails: row.get(1)?,
                    total_threads: row.get(2)?,
                    unread_threads: row.get(3)?,
                    unread_emails: row.get(4)?,
                },
            ))
        })?
        .collect::<Result<HashMap<_, _>, _>>()?;
    Ok(counts)
}

/// The folders that are done and, once the store with the archive role
/// is among them, every label mailbox, whose rows live in that store.
pub(super) fn read_done(
    connection: &Connection,
    key: &AccountKey,
) -> Result<HashSet<MailboxId>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT folder_id FROM bridge_sync WHERE account_key = ?1 AND done = 1
         UNION
         SELECT l.id FROM bridge_mailboxes l
         WHERE l.account_key = ?1 AND l.store = 0 AND l.gmail_label IS NOT NULL
           AND EXISTS (
               SELECT 1 FROM bridge_mailboxes a
               JOIN bridge_sync s ON s.account_key = a.account_key AND s.folder_id = a.id
               WHERE a.account_key = ?1 AND a.store = 1 AND a.role = ?2 AND s.done = 1)",
    )?;
    let done = statement
        .query_map(params![key.as_str(), crate::gmail::ALL_MAIL_ROLE], |row| {
            row.get(0)
        })?
        .collect::<Result<HashSet<_>, _>>()?;
    Ok(done)
}
