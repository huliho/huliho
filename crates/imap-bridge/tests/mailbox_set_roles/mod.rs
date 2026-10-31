// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The role a create of a `Mailbox/set` sets or leaves open, held
//! against the name table on the next pass (RFC 8621 section 2).

use huliho_imap_bridge::testing::Mailboxes;
use serde_json::{Value, json};

use crate::folders_rig::{mailbox, set, started};

/// A Dovecot without its Archive, so the name table has the archive
/// role to give to a folder of that name.
fn without_archive() -> Mailboxes {
    let mailboxes = Mailboxes::dovecot();
    mailboxes.remove("Archive");
    mailboxes
}

/// A create with `role: null` stores an override of no role, so a folder
/// named like a role of the name table holds none through the next
/// pass; a create that leaves the role out takes the name table's role
/// there (RFC 8621 section 2).
#[tokio::test]
async fn a_create_with_a_null_role_holds_none_against_the_name_table_rfc8621_2() {
    let cases = [
        (json!({ "name": "Archive", "role": null }), Value::Null),
        (json!({ "name": "Archive" }), json!("archive")),
    ];
    for (object, role_after_the_pass) in cases {
        let rig = started(without_archive(), false).await;
        let answer = set(&rig, json!({ "create": { "k": object } })).await;
        let id = answer["created"]["k"]["id"].as_str().unwrap().to_owned();
        assert_eq!(answer["notCreated"], json!(null), "{answer}");
        assert_eq!(mailbox(&rig, &id).await["role"], json!(null));
        rig.pass().await.unwrap();
        assert_eq!(mailbox(&rig, &id).await["role"], role_after_the_pass);
        assert_eq!(
            rig.folder("Archive").facts.role,
            role_after_the_pass.as_str().map(str::to_owned)
        );
    }
}
