// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The bridge's third migration over a database the seventh left.

use super::*;

/// The schema version the bridge's second migration leaves.
const BEFORE_FOLDER_MIGRATION: usize = 7;

#[test]
fn the_folder_migration_keeps_the_mailbox_rows_and_adds_the_override_and_the_rights() {
    let mut connection = Connection::open_in_memory().unwrap();
    connection
        .pragma_update(None, "foreign_keys", true)
        .unwrap();
    migrations()
        .to_version(&mut connection, BEFORE_FOLDER_MIGRATION)
        .unwrap();
    connection
        .execute(
            "INSERT INTO bridge_mailboxes (account_key, id, name, imap_name, role, sort_order,
                 subscribed, selectable, store, uid_validity)
             VALUES ('k', 'm', 'Sent', 'Sent', 'sent', 2, 1, 1, 1, 9)",
            [],
        )
        .unwrap();
    migrations().to_latest(&mut connection).unwrap();
    let columns: Vec<String> = connection
        .prepare(
            "SELECT name FROM pragma_table_info('bridge_mailboxes')
             WHERE name IN ('role_override', 'rights') ORDER BY name",
        )
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(columns, ["rights", "role_override"]);
    let row: (String, Option<String>, Option<String>, Option<u32>) = connection
        .query_row(
            "SELECT role, role_override, rights, uid_validity FROM bridge_mailboxes
             WHERE account_key = 'k' AND id = 'm'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(row, ("sent".to_owned(), None, None, Some(9)));
}
