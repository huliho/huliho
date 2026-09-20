// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What happens to the emails of a folder that vanished or that the
//! server renumbered. Every statement works on the rows in place, so a
//! folder of any size costs no memory.

use std::collections::HashMap;

use rusqlite::{Connection, Transaction, params};

use super::{AccountKey, EmailId, MailboxId, StoreError};

/// The rows of one renumbered folder that wait for a match. A larger
/// folder is destroyed and synced afresh: every batch loads the waiting
/// rows, so the limit caps what one batch reads.
pub const REMATCH_LIMIT: u32 = 100_000;

/// What names a message across a renumbering where the server has no id
/// of its own: the keyed hash of its Message-ID, INTERNALDATE and size.
pub(super) type Identity = (Option<Vec<u8>>, i64, u32);

/// One folder inside a write, with the sequence its log rows go under.
pub(super) struct FolderWrite<'a> {
    pub key: &'a AccountKey,
    pub folder: &'a MailboxId,
    pub sequence: u64,
}

/// Which rows of a folder leave.
#[derive(Clone, Copy)]
pub(super) enum Leaving {
    /// Every row: the folder vanished.
    All,
    /// The rows a renumbering left without a match; they wait under a
    /// negative UID.
    Unmatched,
}

impl Leaving {
    /// The rows below this UID leave.
    fn below(self) -> i64 {
        match self {
            Self::All => i64::MAX,
            Self::Unmatched => 0,
        }
    }
}

/// Destroys the leaving rows with their memberships under the write's
/// sequence. A thread is destroyed when its last email leaves and
/// updated otherwise.
pub(super) fn leave(
    transaction: &Transaction<'_>,
    write: &FolderWrite<'_>,
    leaving: Leaving,
) -> Result<(), StoreError> {
    let scope = params![
        write.key.as_str(),
        write.sequence,
        write.folder,
        leaving.below()
    ];
    transaction.execute(
        "INSERT INTO bridge_changes (account_key, sequence, type, id, kind)
         SELECT e.account_key, ?2, 'Thread', e.thread_id,
                CASE WHEN EXISTS (
                    SELECT 1 FROM bridge_emails o
                    WHERE o.account_key = e.account_key AND o.thread_id = e.thread_id
                      AND NOT (o.folder_id = ?3 AND o.uid < ?4))
                THEN 'updated' ELSE 'destroyed' END
         FROM bridge_emails e
         WHERE e.account_key = ?1 AND e.folder_id = ?3 AND e.uid < ?4
         GROUP BY e.thread_id
         ON CONFLICT (account_key, sequence, type, id) DO UPDATE SET kind = excluded.kind",
        scope,
    )?;
    transaction.execute(
        "INSERT INTO bridge_changes (account_key, sequence, type, id, kind)
         SELECT account_key, ?2, 'Email', id, 'destroyed' FROM bridge_emails
         WHERE account_key = ?1 AND folder_id = ?3 AND uid < ?4",
        scope,
    )?;
    let rows = params![write.key.as_str(), write.folder, leaving.below()];
    transaction.execute(
        "DELETE FROM bridge_memberships WHERE account_key = ?1 AND email_id IN (
             SELECT id FROM bridge_emails
             WHERE account_key = ?1 AND folder_id = ?2 AND uid < ?3)",
        rows,
    )?;
    transaction.execute(
        "DELETE FROM bridge_emails WHERE account_key = ?1 AND folder_id = ?2 AND uid < ?3",
        rows,
    )?;
    Ok(())
}

/// The folder vanished: its rows leave and its progress with them.
pub(super) fn vanish(
    transaction: &Transaction<'_>,
    write: &FolderWrite<'_>,
) -> Result<(), StoreError> {
    leave(transaction, write, Leaving::All)?;
    forget_progress(transaction, write)
}

/// The server renumbered the folder (RFC 3501 section 2.3.1.1): its
/// rows wait under negative UIDs for the fresh sync to match them and
/// the progress starts over. What an earlier renumbering left waiting
/// leaves first, so no two rows share a UID; a folder past
/// `REMATCH_LIMIT` leaves whole.
pub(super) fn renumber(
    transaction: &Transaction<'_>,
    write: &FolderWrite<'_>,
) -> Result<(), StoreError> {
    let rows = params![write.key.as_str(), write.folder];
    let held: u32 = transaction.query_row(
        "SELECT COUNT(*) FROM bridge_emails
         WHERE account_key = ?1 AND folder_id = ?2 AND uid > 0",
        rows,
        |row| row.get(0),
    )?;
    let leaving = if held > REMATCH_LIMIT {
        Leaving::All
    } else {
        Leaving::Unmatched
    };
    leave(transaction, write, leaving)?;
    transaction.execute(
        "UPDATE bridge_emails SET uid = -uid
         WHERE account_key = ?1 AND folder_id = ?2 AND uid > 0",
        rows,
    )?;
    forget_progress(transaction, write)
}

fn forget_progress(
    transaction: &Transaction<'_>,
    write: &FolderWrite<'_>,
) -> Result<(), StoreError> {
    transaction.execute(
        "DELETE FROM bridge_sync WHERE account_key = ?1 AND folder_id = ?2",
        params![write.key.as_str(), write.folder],
    )?;
    Ok(())
}

/// The waiting rows of a folder by what names them; two messages that
/// share all three wait under one entry.
pub(super) fn unmatched(
    connection: &Connection,
    key: &AccountKey,
    folder: &MailboxId,
) -> Result<HashMap<Identity, Vec<EmailId>>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT id, message_id_hash, received_at, size FROM bridge_emails
         WHERE account_key = ?1 AND folder_id = ?2 AND uid < 0",
    )?;
    let mut rows = statement.query(params![key.as_str(), folder])?;
    let mut waiting: HashMap<Identity, Vec<EmailId>> = HashMap::new();
    while let Some(row) = rows.next()? {
        let identity = (row.get(1)?, row.get(2)?, row.get(3)?);
        waiting.entry(identity).or_default().push(row.get(0)?);
    }
    Ok(waiting)
}
