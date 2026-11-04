// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

use super::*;
use crate::store::{DraftRow, SetWrite};

#[test]
fn a_draft_row_stands_only_where_appenduid_still_names_a_free_uid_rfc4315_3() {
    let (store, folder) = listed();
    fill(&store, &folder, 1);
    let hash = b"message id".to_vec();
    let draft = |uid: u32, uid_validity: u32| DraftRow {
        id: EmailId::generate(),
        folder: folder.clone(),
        uid_validity,
        facts: facts(uid, None),
        hash: hash.clone(),
        named: Vec::new(),
        sealed: vec![0],
    };
    let write = |row: DraftRow| {
        let drafts = vec![row];
        let set = SetWrite {
            drafts,
            ..SetWrite::default()
        };
        store.write_set(&key(), &set).unwrap().drafts
    };
    let other_validity = 9;
    assert_eq!(write(draft(2, other_validity)), []);
    assert_eq!(write(draft(1, 1)), []);
    assert_eq!(store.landed(&key(), &hash).unwrap(), None);
    let state = store.state(&key()).unwrap();
    let row = draft(2, 1);
    let id = row.id.clone();
    assert_eq!(write(row), std::slice::from_ref(&id));
    assert_eq!(store.landed(&key(), &hash).unwrap().unwrap().id, id);
    let email = logged(&store, ObjectType::Email, state);
    assert_eq!(email, [(id.to_string(), ChangeKind::Created)]);
    assert_eq!(logged(&store, ObjectType::Mailbox, state).len(), 1);
}

#[test]
fn a_row_that_left_for_a_pass_answers_no_draft() {
    let (store, folder) = listed();
    fill(&store, &folder, 1);
    let hash = b"message id".to_vec();
    store
        .write(&key(), |transaction| {
            transaction.execute(
                "UPDATE bridge_emails SET uid = 0, message_id_hash = ?2 WHERE account_key = ?1",
                params![key().as_str(), hash],
            )?;
            Ok(())
        })
        .unwrap();
    assert_eq!(store.landed(&key(), &hash).unwrap(), None);
}
