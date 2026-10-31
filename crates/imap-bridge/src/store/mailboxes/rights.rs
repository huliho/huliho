// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The letters MYRIGHTS named per mailbox, kept on its row: null where
//! the server advertises no ACL.

use std::collections::HashMap;

use rusqlite::{Connection, Transaction, params};

use crate::store::{AccountKey, ChangeKind, MailboxId, StoreError};

/// The letters of every mailbox that holds some.
pub(super) fn read(
    connection: &Connection,
    key: &AccountKey,
) -> Result<HashMap<MailboxId, String>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT id, rights FROM bridge_mailboxes WHERE account_key = ?1 AND rights IS NOT NULL",
    )?;
    let rights = statement
        .query_map([key.as_str()], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<HashMap<_, _>, _>>()?;
    Ok(rights)
}

/// The letters of every mailbox that holds some, by wire name.
pub(super) fn named(
    connection: &Connection,
    key: &AccountKey,
) -> Result<HashMap<String, String>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT imap_name, rights FROM bridge_mailboxes
         WHERE account_key = ?1 AND rights IS NOT NULL",
    )?;
    let names = statement
        .query_map([key.as_str()], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<HashMap<_, _>, _>>()?;
    Ok(names)
}

/// Brings the letters of the rows a pass found, by wire name, to what it
/// read: a name it read takes the new letters, a name it did not loses
/// them and with `None` every row clears them. Every row whose letters
/// moved, as updated.
pub(super) fn apply(
    transaction: &Transaction<'_>,
    key: &AccountKey,
    found: &HashMap<&str, MailboxId>,
    letters: Option<&HashMap<String, String>>,
) -> Result<Vec<(MailboxId, ChangeKind)>, StoreError> {
    let mut statement = transaction
        .prepare("SELECT imap_name, rights FROM bridge_mailboxes WHERE account_key = ?1")?;
    let current: HashMap<String, Option<String>> = statement
        .query_map([key.as_str()], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    drop(statement);
    let mut moved = Vec::new();
    for (name, id) in found {
        let held = current.get(*name).cloned().flatten();
        let wanted = letters.and_then(|read| read.get(*name).cloned());
        if wanted != held {
            transaction.execute(
                "UPDATE bridge_mailboxes SET rights = ?3 WHERE account_key = ?1 AND id = ?2",
                params![key.as_str(), id, wanted],
            )?;
            moved.push((id.clone(), ChangeKind::Updated));
        }
    }
    Ok(moved)
}
