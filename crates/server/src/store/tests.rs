// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The migrations walked with rows in them and the busy timeout.

use super::*;

#[test]
fn migrations_are_valid() {
    migrations().validate().unwrap();
}

#[test]
fn every_bridge_migration_is_one_of_the_sources() {
    for sql in huliho_imap_bridge::store::MIGRATIONS {
        assert!(MIGRATION_SOURCES.contains(sql));
    }
}

#[test]
fn now_is_after_the_epoch() {
    assert!(now_ms() > 0);
}

/// An organization with its owner, valid from the first schema on.
fn insert_owner(connection: &Connection) {
    connection
        .execute(
            "INSERT INTO organizations (id, name, created_at) VALUES ('o', 'o', 0)",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO users (id, organization_id, login, role, created_at)
             VALUES ('u', 'o', 'mira', 'owner', 0)",
            [],
        )
        .unwrap();
}

/// A row from the 0002 schema, with a made-up token hash and blob.
fn insert_pre_0003_session(connection: &Connection) {
    connection
        .execute(
            "INSERT INTO sessions (token_hash, user_id, sealed, created_at, last_seen_at)
             VALUES (X'0102', 'u', X'0304', 7, 9)",
            [],
        )
        .unwrap();
}

/// A row from the 0003 schema, with a made-up sealed blob.
fn insert_pre_0004_account(connection: &Connection) {
    connection
        .execute(
            "INSERT INTO accounts
             (id, organization_id, user_id, kind, auth_method, credentials, created_at)
             VALUES ('a', 'o', 'u', 'jmap', 'bearer', X'0304', 5)",
            [],
        )
        .unwrap();
}

/// An account row as the 0005 schema holds it.
fn insert_pre_0006_account(connection: &Connection) {
    connection
        .execute(
            "INSERT INTO accounts
             (id, organization_id, user_id, address, name, provider, kind, auth_method,
              settings, credentials, created_at)
             VALUES ('a', 'o', 'u', 'sanne@example.test', 'Sanne', 'generic', 'imap',
                     'password', '{}', X'0304', 5)",
            [],
        )
        .unwrap();
}

struct MigratedSession {
    token_hash: Vec<u8>,
    id: String,
    sealed: Vec<u8>,
    device: String,
    address: Option<String>,
    created_at: i64,
    last_seen_at: i64,
}

#[test]
fn the_device_migration_keeps_sessions_and_names_users() {
    let mut connection = Connection::open_in_memory().unwrap();
    connection
        .pragma_update(None, "foreign_keys", true)
        .unwrap();
    migrations().to_version(&mut connection, 2).unwrap();
    insert_owner(&connection);
    insert_pre_0003_session(&connection);
    migrations().to_latest(&mut connection).unwrap();
    let session = connection
        .query_row(
            "SELECT token_hash, id, sealed, device, address, created_at, last_seen_at
             FROM sessions",
            [],
            |row| {
                Ok(MigratedSession {
                    token_hash: row.get(0)?,
                    id: row.get(1)?,
                    sealed: row.get(2)?,
                    device: row.get(3)?,
                    address: row.get(4)?,
                    created_at: row.get(5)?,
                    last_seen_at: row.get(6)?,
                })
            },
        )
        .unwrap();
    assert_eq!(session.token_hash, vec![1, 2]);
    assert_eq!(session.id.len(), 32);
    assert_eq!(session.sealed, vec![3, 4]);
    assert_eq!(session.device, "{}");
    assert_eq!(session.address, None);
    assert_eq!((session.created_at, session.last_seen_at), (7, 9));
    let name: String = connection
        .query_row("SELECT name FROM users WHERE id = 'u'", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(name, "mira");
}

#[test]
fn the_settings_migration_keeps_accounts_and_they_list() {
    use crate::accounts::{self, AccountKind, AuthMethod, Provider};
    use crate::ids::UserId;
    let mut connection = Connection::open_in_memory().unwrap();
    connection
        .pragma_update(None, "foreign_keys", true)
        .unwrap();
    migrations().to_version(&mut connection, 3).unwrap();
    insert_owner(&connection);
    insert_pre_0004_account(&connection);
    let store = Store::initialize(connection).unwrap();
    let scope = crate::scope::resolve(&store, &UserId::from("u".to_owned()), None).unwrap();
    let listed = accounts::list(&store, &scope).unwrap();
    assert_eq!(listed.len(), 1);
    let account = &listed[0];
    assert_eq!(account.id.as_str(), "a");
    assert_eq!(account.kind, AccountKind::Jmap);
    assert_eq!(account.auth_method, AuthMethod::Bearer);
    assert_eq!(account.provider, Provider::Generic);
    assert_eq!((account.address.as_str(), account.name.as_str()), ("", ""));
    assert_eq!(account.created_at, 5);
    let (settings, credentials): (String, Vec<u8>) = store
        .read(|connection| {
            connection
                .query_row("SELECT settings, credentials FROM accounts", [], |row| {
                    Ok((row.get(0)?, row.get(1)?))
                })
                .map_err(StoreError::from)
        })
        .unwrap();
    assert_eq!(settings, "{}");
    assert_eq!(credentials, vec![3, 4]);
}

#[test]
fn the_instance_admin_migration_keeps_users_without_the_flag() {
    use crate::ids::{Role, UserId};
    let mut connection = Connection::open_in_memory().unwrap();
    connection
        .pragma_update(None, "foreign_keys", true)
        .unwrap();
    migrations().to_version(&mut connection, 4).unwrap();
    insert_owner(&connection);
    let store = Store::initialize(connection).unwrap();
    let scope = crate::scope::resolve(&store, &UserId::from("u".to_owned()), None).unwrap();
    assert!(!scope.instance_admin());
    assert_eq!(scope.role(), Role::Owner);
}

#[test]
fn the_bridge_migration_keeps_accounts_and_creates_the_bridge_tables() {
    use crate::accounts;
    use crate::ids::UserId;
    let mut connection = Connection::open_in_memory().unwrap();
    connection
        .pragma_update(None, "foreign_keys", true)
        .unwrap();
    migrations().to_version(&mut connection, 5).unwrap();
    insert_owner(&connection);
    insert_pre_0006_account(&connection);
    migrations().to_latest(&mut connection).unwrap();
    let tables: Vec<String> = connection
        .prepare(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name LIKE 'bridge%'
             ORDER BY name",
        )
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        tables,
        [
            "bridge_changes",
            "bridge_emails",
            "bridge_mailboxes",
            "bridge_memberships",
            "bridge_message_ids",
            "bridge_state",
            "bridge_sync"
        ]
    );
    let progress: Vec<String> = connection
        .prepare(
            "SELECT name FROM pragma_table_info('bridge_sync')
             WHERE name IN ('uid_next', 'highest_modseq', 'messages') ORDER BY name",
        )
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(progress, ["highest_modseq", "messages", "uid_next"]);
    let indexed: bool = connection
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master
                            WHERE type = 'index' AND name = 'bridge_message_ids_thread')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(indexed);
    let store = Store::initialize(connection).unwrap();
    let scope = crate::scope::resolve(&store, &UserId::from("u".to_owned()), None).unwrap();
    let listed = accounts::list(&store, &scope).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].address, "sanne@example.test");
}

/// A sync row and a message id as the 0006 schema holds them.
fn insert_pre_0007_bridge_rows(connection: &Connection) {
    connection
        .execute(
            "INSERT INTO bridge_sync (account_key, folder_id, lowest_synced_uid, done)
             VALUES ('k', 'm', 3, 1)",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO bridge_message_ids (account_key, message_id_hash, thread_id)
             VALUES ('k', X'0A0B', 't')",
            [],
        )
        .unwrap();
}

struct MigratedProgress {
    lowest_synced_uid: Option<u32>,
    done: bool,
    uid_next: Option<u32>,
    highest_modseq: Option<u64>,
    messages: Option<u32>,
}

#[test]
fn the_refresh_migration_keeps_the_bridge_rows_and_indexes_the_threads() {
    let mut connection = Connection::open_in_memory().unwrap();
    connection
        .pragma_update(None, "foreign_keys", true)
        .unwrap();
    migrations().to_version(&mut connection, 6).unwrap();
    insert_pre_0007_bridge_rows(&connection);
    migrations().to_latest(&mut connection).unwrap();
    let progress = connection
        .query_row(
            "SELECT lowest_synced_uid, done, uid_next, highest_modseq, messages
             FROM bridge_sync",
            [],
            |row| {
                Ok(MigratedProgress {
                    lowest_synced_uid: row.get(0)?,
                    done: row.get(1)?,
                    uid_next: row.get(2)?,
                    highest_modseq: row.get(3)?,
                    messages: row.get(4)?,
                })
            },
        )
        .unwrap();
    assert_eq!((progress.lowest_synced_uid, progress.done), (Some(3), true));
    assert_eq!(
        (
            progress.uid_next,
            progress.highest_modseq,
            progress.messages
        ),
        (None, None, None)
    );
    let thread: String = connection
        .query_row(
            "SELECT thread_id FROM bridge_message_ids WHERE message_id_hash = X'0A0B'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(thread, "t");
    let indexed: Vec<String> = connection
        .prepare("SELECT name FROM pragma_index_info('bridge_message_ids_thread') ORDER BY seqno")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(indexed, ["account_key", "thread_id"]);
}

/// Long enough that the second writer arrives while the first holds
/// the lock, short enough for a test.
const HOLD: Duration = Duration::from_millis(300);
const ARRIVE_AFTER: Duration = Duration::from_millis(50);

#[test]
fn a_second_connection_waits_for_a_writer_instead_of_failing() {
    let dir = tempfile::tempdir().unwrap();
    let first = Store::open(dir.path()).unwrap();
    let second = Store::open(dir.path()).unwrap();
    let holder = std::thread::spawn(move || {
        first
            .write(|transaction| {
                transaction.execute(
                    "INSERT INTO organizations (id, name, created_at) VALUES ('a', 'a', 0)",
                    [],
                )?;
                std::thread::sleep(HOLD);
                Ok(())
            })
            .unwrap();
    });
    std::thread::sleep(ARRIVE_AFTER);
    second
        .write(|transaction| {
            transaction.execute(
                "INSERT INTO organizations (id, name, created_at) VALUES ('b', 'b', 0)",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    holder.join().unwrap();
    let rows: i64 = second
        .read(|connection| {
            connection
                .query_row("SELECT COUNT(*) FROM organizations", [], |row| row.get(0))
                .map_err(StoreError::from)
        })
        .unwrap();
    assert_eq!(rows, 2);
}
