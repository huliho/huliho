// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! How a `Mailbox/set` call reads its objects and patches and what a
//! create or an update answers (RFC 8620 section 5.3, RFC 8621 section
//! 2.5).

use super::*;

#[test]
fn a_create_reads_the_four_settable_properties_and_names_every_other_one_rfc8620_5_3() {
    let read = create(
        "k",
        &json!({ "name": "Work", "parentId": "m1", "isSubscribed": false }),
    );
    assert_eq!(
        read,
        Ok(Create {
            creation_id: "k".to_owned(),
            name: "Work".to_owned(),
            parent: Some("m1".to_owned()),
            role: None,
            subscribed: Some(false),
        })
    );
    let role_of = |object: Value| create("k", &object).map(|create| create.role);
    assert_eq!(
        role_of(json!({ "name": "Work", "role": "archive" })),
        Ok(Some(Nullable::Value("archive".to_owned())))
    );
    assert_eq!(
        role_of(json!({ "name": "Work", "role": null })),
        Ok(Some(Nullable::Null))
    );
    let refused = create(
        "k",
        &json!({ "name": 7, "role": 1, "myRights": {}, "sortOrder": 2, "isSubscribed": "yes" }),
    );
    let mut bad = refused.unwrap_err();
    bad.sort();
    assert_eq!(
        bad,
        ["isSubscribed", "myRights", "name", "role", "sortOrder"]
    );
    assert_eq!(create("k", &json!("Work")), Err(Vec::new()));
}

#[test]
fn a_patch_reads_its_properties_and_a_path_into_one_is_not_settable_rfc8620_5_3() {
    let read = update("m1", &json!({ "parentId": null, "role": "archive" }));
    assert_eq!(
        read,
        Ok(Update {
            id: "m1".to_owned(),
            name: None,
            parent: Some(Nullable::Null),
            role: Some(Nullable::Value("archive".to_owned())),
            subscribed: None,
        })
    );
    let refused = update("m1", &json!({ "name": null, "myRights/mayRename": true }));
    let mut bad = refused.unwrap_err();
    bad.sort();
    assert_eq!(bad, ["myRights/mayRename", "name"]);
}

#[test]
fn a_create_answers_what_it_did_not_send_and_an_update_what_differs_rfc8620_5_3() {
    let rendered: Map<String, Value> = [
        ("id", json!("m9")),
        ("name", json!("Caf\u{e9}")),
        ("parentId", json!("m1")),
        ("isSubscribed", json!(false)),
        ("sortOrder", json!(6)),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_owned(), value))
    .collect();
    let sent = json!({ "name": "Cafe\u{301}", "parentId": "#a", "isSubscribed": true });
    let created = beyond(rendered.clone(), sent.as_object(), None);
    assert_eq!(
        created,
        json!({
            "id": "m9",
            "name": "Caf\u{e9}",
            "parentId": "m1",
            "isSubscribed": false,
            "sortOrder": 6,
        })
    );
    let same = json!({ "name": "Caf\u{e9}", "parentId": "m1" });
    let created = beyond(rendered.clone(), same.as_object(), None);
    assert_eq!(created.as_object().unwrap().len(), 3);
    assert_eq!(
        beyond(rendered.clone(), same.as_object(), Some(&rendered)),
        Value::Null
    );
    let mut before = rendered.clone();
    before.insert("sortOrder".to_owned(), json!(3));
    assert_eq!(
        beyond(rendered.clone(), same.as_object(), Some(&before)),
        json!({ "sortOrder": 6 })
    );
    let made = Map::new();
    assert_eq!(beyond(rendered, same.as_object(), Some(&made)), Value::Null);
}

#[test]
fn every_refusal_answers_its_set_error_type_rfc8621_2_5() {
    for (refusal, kind) in [
        (Refusal::Forbidden, "forbidden"),
        (Refusal::NotFound, "notFound"),
        (Refusal::MailboxHasChild, "mailboxHasChild"),
        (Refusal::MailboxHasEmail, "mailboxHasEmail"),
        (Refusal::WillDestroy, "willDestroy"),
        (Refusal::ServerFail, "serverFail"),
        (Refusal::ServerUnavailable, "serverUnavailable"),
    ] {
        assert_eq!(set_error(&refusal), json!({ "type": kind }));
    }
    let invalid = set_error(&Refusal::InvalidProperties(vec!["name"]));
    assert_eq!(
        invalid,
        json!({ "type": "invalidProperties", "properties": ["name"] })
    );
}
