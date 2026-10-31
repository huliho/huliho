// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What a `Mailbox/set` did on the server, written to the rows as one
//! state, and what the next pass reads back of the rows: the role the
//! user set and which mailboxes hold letters.

use std::collections::HashMap;

use rusqlite::{Connection, Transaction, params};

use super::{MailboxRow, rights, upsert};
use crate::store::changes::{ChangeKind, ObjectType, next_sequence, prune, state_of};
use crate::store::folders::{self, FolderWrite};
use crate::store::ledger::Ledger;
use crate::store::{AccountKey, MailboxId, Store, StoreError};

/// How the column spells an override of no role.
const NO_ROLE: &str = "";

/// The role the user set on a mailbox, read ahead of what the server's
/// attribute or the name table gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoleOverride {
    Role(String),
    /// No role, whatever the server names.
    NoRole,
}

impl RoleOverride {
    /// The role the mailbox carries under the override.
    #[must_use]
    pub fn role(&self) -> Option<&str> {
        match self {
            Self::Role(role) => Some(role),
            Self::NoRole => None,
        }
    }

    fn column(&self) -> &str {
        self.role().unwrap_or(NO_ROLE)
    }

    fn from_column(value: String) -> Self {
        if value == NO_ROLE {
            Self::NoRole
        } else {
            Self::Role(value)
        }
    }
}

/// What a mailbox pass needs of the rows beyond the listing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PassInputs {
    /// The role the user set, by wire name.
    pub overrides: HashMap<String, RoleOverride>,
    /// The letters MYRIGHTS named on each row that holds some, by wire
    /// name.
    pub rights: HashMap<String, String>,
}

/// One change a `Mailbox/set` made on the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MailboxChange {
    /// A folder CREATE made, under the id the bridge gave it, with the
    /// role the user set.
    Created {
        row: MailboxRow,
        role: Option<RoleOverride>,
    },
    /// A RENAME: the row's new wire name, leaf and parent. Every row
    /// under it takes the new wire name in front of its own leaves.
    Renamed {
        id: MailboxId,
        imap_name: String,
        name: String,
        parent_id: Option<MailboxId>,
    },
    Subscribed {
        id: MailboxId,
        subscribed: bool,
    },
    /// The role the user set, with the role and sort order the row
    /// carries under it.
    Role {
        id: MailboxId,
        role: RoleOverride,
        sort_order: u32,
    },
    /// A DELETE: the row leaves with its emails, as a mailbox missing
    /// from a listing does.
    Destroyed {
        id: MailboxId,
    },
}

/// The state before the write and after it; the same when nothing was
/// written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MailboxesWritten {
    pub before: u64,
    pub after: u64,
}

impl Store {
    /// What the next pass needs of the rows: the overrides and the
    /// letters on record.
    ///
    /// # Errors
    ///
    /// Returns the database error.
    pub fn pass_inputs(&self, key: &AccountKey) -> Result<PassInputs, StoreError> {
        self.read(|connection| {
            Ok(PassInputs {
                overrides: overrides(connection, key)?,
                rights: rights::named(connection, key)?,
            })
        })
    }

    /// Writes what one `Mailbox/set` did, in its order, as one state.
    /// Each mailbox it touched is logged once; a destroyed one takes its
    /// emails and their threads along.
    ///
    /// # Errors
    ///
    /// Returns the database error.
    pub fn write_mailboxes(
        &self,
        key: &AccountKey,
        changes: &[MailboxChange],
    ) -> Result<MailboxesWritten, StoreError> {
        self.write(key, |transaction| {
            let before = state_of(transaction, key)?;
            let mut ledger = Ledger::default();
            let mut gone = Vec::new();
            for change in changes {
                written(transaction, key, change, &mut ledger)?;
                if let MailboxChange::Destroyed { id } = change {
                    gone.push(id);
                }
            }
            if ledger.is_empty() {
                return Ok(MailboxesWritten {
                    before,
                    after: before,
                });
            }
            let sequence = next_sequence(transaction, key)?;
            // First, since the rows a vanish logs step aside for these.
            ledger.write(transaction, key, sequence)?;
            for folder in gone {
                folders::vanish(
                    transaction,
                    &FolderWrite {
                        key,
                        folder,
                        sequence,
                    },
                )?;
            }
            prune(transaction, key)?;
            Ok(MailboxesWritten {
                before,
                after: sequence,
            })
        })
    }
}

fn overrides(
    connection: &Connection,
    key: &AccountKey,
) -> Result<HashMap<String, RoleOverride>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT imap_name, role_override FROM bridge_mailboxes
         WHERE account_key = ?1 AND role_override IS NOT NULL",
    )?;
    let found = statement
        .query_map([key.as_str()], |row| {
            Ok((row.get(0)?, RoleOverride::from_column(row.get(1)?)))
        })?
        .collect::<Result<HashMap<_, _>, _>>()?;
    Ok(found)
}

/// One change written to the rows and noted.
fn written(
    transaction: &Transaction<'_>,
    key: &AccountKey,
    change: &MailboxChange,
    ledger: &mut Ledger,
) -> Result<(), StoreError> {
    let (id, kind) = match change {
        MailboxChange::Created { row, role } => {
            upsert(transaction, key, row)?;
            if let Some(role) = role {
                transaction.execute(
                    "UPDATE bridge_mailboxes SET role_override = ?3
                     WHERE account_key = ?1 AND id = ?2",
                    params![key.as_str(), row.id, role.column()],
                )?;
            }
            (&row.id, ChangeKind::Created)
        }
        MailboxChange::Renamed {
            id,
            imap_name,
            name,
            parent_id,
        } => {
            renamed(transaction, key, (id, imap_name), ledger)?;
            transaction.execute(
                "UPDATE bridge_mailboxes SET name = ?3, parent_id = ?4
                 WHERE account_key = ?1 AND id = ?2",
                params![key.as_str(), id, name, parent_id],
            )?;
            (id, ChangeKind::Updated)
        }
        MailboxChange::Subscribed { id, subscribed } => {
            transaction.execute(
                "UPDATE bridge_mailboxes SET subscribed = ?3 WHERE account_key = ?1 AND id = ?2",
                params![key.as_str(), id, subscribed],
            )?;
            (id, ChangeKind::Updated)
        }
        MailboxChange::Role {
            id,
            role,
            sort_order,
        } => {
            transaction.execute(
                "UPDATE bridge_mailboxes SET role_override = ?3, role = ?4, sort_order = ?5
                 WHERE account_key = ?1 AND id = ?2",
                params![key.as_str(), id, role.column(), role.role(), sort_order],
            )?;
            (id, ChangeKind::Updated)
        }
        MailboxChange::Destroyed { id } => {
            transaction.execute(
                "DELETE FROM bridge_mailboxes WHERE account_key = ?1 AND id = ?2",
                params![key.as_str(), id],
            )?;
            (id, ChangeKind::Destroyed)
        }
    };
    ledger.note(ObjectType::Mailbox, id.as_str(), kind);
    Ok(())
}

/// The row and every row under it, found through the parents, take the
/// new wire name in place of the old one in front; a user label of a
/// Gmail account, which is its wire name, follows it.
fn renamed(
    transaction: &Transaction<'_>,
    key: &AccountKey,
    (id, imap_name): (&MailboxId, &str),
    ledger: &mut Ledger,
) -> Result<(), StoreError> {
    let mut statement = transaction.prepare(
        "WITH RECURSIVE tree(id) AS (
             SELECT ?2
             UNION
             SELECT m.id FROM bridge_mailboxes m JOIN tree t ON m.parent_id = t.id
             WHERE m.account_key = ?1)
         SELECT id, imap_name FROM bridge_mailboxes
         WHERE account_key = ?1 AND id IN tree",
    )?;
    let tree: Vec<(MailboxId, String)> = statement
        .query_map(params![key.as_str(), id], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?
        .collect::<Result<_, _>>()?;
    drop(statement);
    let Some(old) = tree
        .iter()
        .find(|(found, _)| found == id)
        .map(|(_, old)| old.clone())
    else {
        return Ok(());
    };
    for (member, wire) in tree {
        let Some(rest) = wire.strip_prefix(old.as_str()) else {
            continue;
        };
        let moved = format!("{imap_name}{rest}");
        transaction.execute(
            "UPDATE bridge_mailboxes SET imap_name = ?3,
                 gmail_label = CASE WHEN gmail_label = imap_name THEN ?3 ELSE gmail_label END
             WHERE account_key = ?1 AND id = ?2",
            params![key.as_str(), member, moved],
        )?;
        ledger.note(ObjectType::Mailbox, member.as_str(), ChangeKind::Updated);
    }
    Ok(())
}
