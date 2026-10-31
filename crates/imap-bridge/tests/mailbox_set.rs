// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! `Mailbox/set` against the scripted server (RFC 8621 section 2.5): a
//! name the user typed on the wire or refused before anything is sent,
//! a create that lands at once, a rename that keeps every id and the
//! roles the user set, the subscriptions, a destroy that takes its mail
//! and the refusals the rows answer without a command.

mod folders_rig;
mod mailbox_set_faults;
mod mailbox_set_gmail;
mod mailbox_set_order;
mod mailbox_set_roles;
mod sync_rig;

use folders_rig::{
    dovecot, id_of, mailbox, mailboxes_changed, refused, rows, sent, set, started, state,
};
use huliho_imap_bridge::store::ChangeKind;
use huliho_imap_bridge::testing::{Extension, Folder};
use serde_json::json;
use sync_rig::{ACCOUNT, Rig, inbox};

/// Abuse cases first: every name either reaches the server as one
/// escaped quoted string in modified UTF-7 under its parent or is
/// refused with nothing sent (RFC 3501 sections 4.3 and 5.1.3).
#[tokio::test]
async fn a_typed_name_reaches_the_wire_escaped_or_is_refused_before_anything_is_sent_rfc3501_5_1_3()
{
    let rig = started(dovecot(), false).await;
    let inbox = id_of(&rig, "INBOX");
    let long = "x".repeat(256);
    let refused_names = [
        "a\r\nb",
        "a/b",
        "tab\there",
        " edge",
        "edge ",
        "\u{3000}wide",
        "",
        long.as_str(),
    ];
    for name in refused_names {
        let answer = set(&rig, json!({ "create": { "k": { "name": name } } })).await;
        assert_eq!(refused(&answer, "notCreated", "k"), "invalidProperties");
        assert_eq!(answer["notCreated"]["k"]["properties"], json!(["name"]));
    }
    assert_eq!(sent(&rig), Vec::<String>::new());
    let made = [
        (
            "Quote \" and \\ back",
            "CREATE \"INBOX/Quote \\\" and \\\\ back\"",
        ),
        (
            "Caf\u{e9} \u{53f0}\u{5317}",
            "CREATE \"INBOX/Caf&AOk- &U,BTFw-\"",
        ),
    ];
    for (name, command) in made {
        let create = json!({ "k": { "name": name, "parentId": inbox } });
        let answer = set(&rig, json!({ "create": create })).await;
        assert!(answer["created"]["k"]["id"].is_string(), "{answer}");
        assert!(sent(&rig).contains(&command.to_owned()), "{:?}", sent(&rig));
    }
    let names: Vec<String> = rig
        .fake
        .script()
        .mailboxes
        .folders()
        .into_iter()
        .map(|folder| folder.name)
        .collect();
    assert!(names.contains(&"INBOX/Quote \" and \\ back".to_owned()));
    assert!(names.contains(&"INBOX/Caf&AOk- &U,BTFw-".to_owned()));
}

/// What another client does reaches the rows through the pass: a folder
/// it made lands as created, one it renamed as destroyed plus created,
/// since the pass keys the rows by wire name.
#[tokio::test]
async fn a_folder_another_client_made_or_renamed_lands_as_the_pass_has_it() {
    let rig = Rig::start(inbox(Vec::new())).await;
    rig.fake.script().mailboxes.push(Folder::new("Theirs"));
    let before = state(&rig);
    rig.pass().await.unwrap();
    let theirs = id_of(&rig, "Theirs");
    assert_eq!(
        mailboxes_changed(&rig, before),
        [(theirs.clone(), ChangeKind::Created)]
    );
    let model = &rig.fake.script().mailboxes;
    model.remove("Theirs");
    model.push(Folder::new("Renamed"));
    let before = state(&rig);
    rig.pass().await.unwrap();
    let renamed = id_of(&rig, "Renamed");
    let logged = mailboxes_changed(&rig, before);
    assert!(
        logged.contains(&(theirs, ChangeKind::Destroyed)),
        "{logged:?}"
    );
    assert!(
        logged.contains(&(renamed, ChangeKind::Created)),
        "{logged:?}"
    );
}

/// A create sends CREATE then SUBSCRIBE, writes its row at once under
/// one new state and answers the id with every property the client did
/// not send; the next pass finds the row as written (RFC 8621 section
/// 2.5).
#[tokio::test]
async fn a_create_lands_at_once_subscribed_and_the_next_pass_finds_it_equal_rfc8621_2_5() {
    let rig = started(dovecot(), false).await;
    let before = state(&rig);
    let work = id_of(&rig, "Work");
    let create = json!({ "k": { "name": "Plan \u{fc}", "parentId": work } });
    let answer = set(&rig, json!({ "create": create })).await;
    let id = answer["created"]["k"]["id"].as_str().unwrap().to_owned();
    assert_eq!(answer["oldState"], before.to_string());
    assert_eq!(answer["newState"], (before + 1).to_string());
    assert_eq!(answer["created"]["k"]["isSubscribed"], true);
    assert_eq!(answer["created"]["k"].get("name"), None);
    assert_eq!(answer["created"]["k"]["myRights"]["mayRename"], true);
    assert_eq!(
        sent(&rig),
        [
            "CREATE \"Work/Plan &APw-\"",
            "SUBSCRIBE \"Work/Plan &APw-\""
        ]
    );
    assert_eq!(
        mailboxes_changed(&rig, before),
        [(id.clone(), ChangeKind::Created)]
    );
    let shown = mailbox(&rig, &id).await;
    assert_eq!(shown["name"], "Plan \u{fc}");
    assert_eq!(shown["parentId"], json!(work));
    let after = state(&rig);
    rig.pass().await.unwrap();
    let logged = mailboxes_changed(&rig, after);
    assert!(
        logged
            .iter()
            .all(|(changed, kind)| *changed == id && *kind == ChangeKind::Updated),
        "{logged:?}"
    );
    assert_eq!(id_of(&rig, "Work/Plan &APw-"), id);
}

/// A move into a mailbox a `Mailbox/set` made before any pass lands and
/// the row takes the UIDVALIDITY the move ran under; a pass that then
/// finds another one treats the folder as renumbered (RFC 3501 section
/// 2.3.1.1).
#[tokio::test]
async fn a_move_into_a_mailbox_made_before_any_pass_lands_under_its_uidvalidity_rfc3501_2_3_1_1() {
    let rig = started(dovecot(), false).await;
    rig.sync("Work").await;
    let email = rig.created(0).remove(0);
    let work = id_of(&rig, "Work");
    let answer = set(&rig, json!({ "create": { "k": { "name": "Plan" } } })).await;
    let plan = answer["created"]["k"]["id"].as_str().unwrap().to_owned();
    assert_eq!(rig.folder("Plan").facts.uid_validity, None);
    let patch = json!({
        (format!("mailboxIds/{work}")): null,
        (format!("mailboxIds/{plan}")): true,
    });
    let arguments = json!({ "accountId": ACCOUNT, "update": { (&email): patch } });
    let moved = rig.call(&json!(["Email/set", arguments, "c1"])).await;
    assert_eq!(moved["updated"], json!({ (&email): null }), "{moved}");
    let folders = rig.fake.script().mailboxes.folders();
    let folder = folders.iter().find(|folder| folder.name == "Plan").unwrap();
    let uid_validity = folder.uid_validity;
    assert_eq!(rig.folder("Plan").facts.uid_validity, Some(uid_validity));
    let row = |rig: &Rig| {
        let (_, mut rows) = rig
            .cache
            .store
            .keyword_rows(&rig.cache.key, &[&email])
            .unwrap();
        rows.remove(0)
    };
    assert_eq!(row(&rig).folder.to_string(), plan);
    assert_eq!(row(&rig).uid, Some(1));
    let model = &rig.fake.script().mailboxes;
    let mut folders = model.folders();
    for folder in folders.iter_mut().filter(|folder| folder.name == "Plan") {
        folder.uid_validity += 1;
    }
    model.set(folders);
    rig.pass().await.unwrap();
    assert_eq!(
        rig.folder("Plan").facts.uid_validity,
        Some(uid_validity + 1)
    );
    assert_eq!(row(&rig).uid, None, "the moved row waits for a match");
}

/// A rename rewrites the wire name of the folder and of every folder
/// under it and keeps both ids. The subscriptions follow the new names,
/// since the server leaves them on the old ones, so the next pass finds
/// the rows equal (RFC 3501 section 6.3.5, RFC 9051 section 6.3.6).
#[tokio::test]
async fn a_rename_keeps_the_id_of_the_folder_and_of_every_folder_under_it_rfc3501_6_3_5() {
    let rig = started(dovecot(), false).await;
    let (work, q3) = (id_of(&rig, "Work"), id_of(&rig, "Work/Q3"));
    let before = state(&rig);
    let answer = set(&rig, json!({ "update": { (&work): { "name": "Job" } } })).await;
    assert_eq!(answer["updated"][&work], json!(null), "{answer}");
    assert_eq!(
        sent(&rig),
        [
            "RENAME \"Work\" \"Job\"",
            "SUBSCRIBE \"Job\"",
            "UNSUBSCRIBE \"Work\"",
            "SUBSCRIBE \"Job/Q3\"",
            "UNSUBSCRIBE \"Work/Q3\""
        ]
    );
    assert_eq!(
        (id_of(&rig, "Job"), id_of(&rig, "Job/Q3")),
        (work.clone(), q3.clone())
    );
    let logged = mailboxes_changed(&rig, before);
    assert_eq!(logged.len(), 2, "{logged:?}");
    for id in [work, q3] {
        assert!(logged.contains(&(id, ChangeKind::Updated)), "{logged:?}");
    }
    let after = state(&rig);
    rig.pass().await.unwrap();
    assert_eq!(mailboxes_changed(&rig, after), []);
}

/// The role the user set stays with the mailbox across a rename, on the
/// renamed folder and on one under it, through the next pass (RFC 8621
/// section 2).
#[tokio::test]
async fn a_role_the_user_set_stays_across_a_rename_rfc8621_2() {
    let rig = started(dovecot(), false).await;
    let (archive, work, q3) = (
        id_of(&rig, "Archive"),
        id_of(&rig, "Work"),
        id_of(&rig, "Work/Q3"),
    );
    let roles = json!({
        (&archive): { "role": null },
        (&work): { "role": "archive" },
        (&q3): { "role": "important" },
    });
    let answer = set(&rig, json!({ "update": roles })).await;
    assert_eq!(answer["notUpdated"], json!(null), "{answer}");
    let renamed = set(&rig, json!({ "update": { (&work): { "name": "Job" } } })).await;
    assert_eq!(renamed["updated"][&work], json!(null), "{renamed}");
    assert_eq!(mailbox(&rig, &work).await["role"], "archive");
    assert_eq!(mailbox(&rig, &q3).await["role"], "important");
    rig.pass().await.unwrap();
    assert_eq!(rig.folder("Job").facts.role.as_deref(), Some("archive"));
    assert_eq!(
        rig.folder("Job/Q3").facts.role.as_deref(),
        Some("important")
    );
    assert_eq!(rig.folder("Archive").facts.role, None);
}

/// `isSubscribed` is SUBSCRIBE or UNSUBSCRIBE; INBOX and a role mailbox
/// never leave the tree (RFC 3501 sections 6.3.6 and 6.3.7).
#[tokio::test]
async fn the_subscription_follows_is_subscribed_and_inbox_and_a_role_stay_rfc3501_6_3_6() {
    let rig = started(dovecot(), false).await;
    let (work, inbox, sent_box) = (
        id_of(&rig, "Work"),
        id_of(&rig, "INBOX"),
        id_of(&rig, "Sent"),
    );
    let hidden = set(
        &rig,
        json!({ "update": { (&work): { "isSubscribed": false } } }),
    )
    .await;
    assert_eq!(hidden["updated"][&work], json!(null), "{hidden}");
    let shown = set(
        &rig,
        json!({ "update": { (&work): { "isSubscribed": true } } }),
    )
    .await;
    assert_eq!(shown["updated"][&work], json!(null), "{shown}");
    assert_eq!(sent(&rig), ["UNSUBSCRIBE \"Work\"", "SUBSCRIBE \"Work\""]);
    let update = json!({
        (&inbox): { "isSubscribed": false },
        (&sent_box): { "isSubscribed": false },
    });
    let answer = set(&rig, json!({ "update": update })).await;
    assert_eq!(refused(&answer, "notUpdated", &inbox), "forbidden");
    assert_eq!(refused(&answer, "notUpdated", &sent_box), "forbidden");
    assert_eq!(sent(&rig).len(), 2);
    assert_eq!(
        rows(&rig)
            .iter()
            .filter(|row| !row.facts.subscribed)
            .count(),
        0
    );
}

/// An update that asks for what the mailbox holds already, the same
/// subscription, name or parent or nothing at all, lands with nothing
/// sent, nothing written and null under its id (RFC 8620 section 5.3).
#[tokio::test]
async fn an_update_that_changes_nothing_lands_with_nothing_sent_or_written_rfc8620_5_3() {
    let rig = started(dovecot(), false).await;
    let work = id_of(&rig, "Work");
    let before = state(&rig);
    for patch in [
        json!({ "isSubscribed": true }),
        json!({ "name": "Work" }),
        json!({ "parentId": null }),
        json!({}),
    ] {
        let answer = set(&rig, json!({ "update": { (&work): patch } })).await;
        assert_eq!(answer["updated"], json!({ (&work): null }), "{answer}");
        assert_eq!(answer["notUpdated"], json!(null), "{answer}");
        assert_eq!(answer["oldState"], answer["newState"], "{answer}");
    }
    assert_eq!(sent(&rig), Vec::<String>::new());
    assert_eq!(state(&rig), before);
}

/// A destroy with `onDestroyRemoveEmails` sends DELETE and the row
/// leaves with its emails and their threads under one state; without
/// the argument, with a child or on INBOX or a role nothing is sent
/// (RFC 8621 section 2.5).
#[tokio::test]
async fn a_destroy_takes_its_mail_under_one_state_and_the_rows_refuse_the_rest_rfc8621_2_5() {
    let rig = started(dovecot(), false).await;
    rig.sync("Work").await;
    let (work, q3, inbox, trash) = (
        id_of(&rig, "Work"),
        id_of(&rig, "Work/Q3"),
        id_of(&rig, "INBOX"),
        id_of(&rig, "Trash"),
    );
    let answer = set(&rig, json!({ "destroy": [&q3, &work, &inbox, &trash] })).await;
    assert_eq!(answer["destroyed"], json!([q3]), "{answer}");
    assert_eq!(refused(&answer, "notDestroyed", &work), "mailboxHasEmail");
    assert_eq!(refused(&answer, "notDestroyed", &inbox), "forbidden");
    assert_eq!(refused(&answer, "notDestroyed", &trash), "forbidden");
    assert_eq!(
        sent(&rig),
        ["DELETE \"Work/Q3\"", "UNSUBSCRIBE \"Work/Q3\""]
    );
    let before = state(&rig);
    let email = rig.created(0).remove(0);
    let destroy = json!({ "destroy": [&work], "onDestroyRemoveEmails": true });
    let answer = set(&rig, destroy).await;
    assert_eq!(answer["destroyed"], json!([work]), "{answer}");
    assert_eq!(state(&rig), before + 1);
    let gone = rig.changes(huliho_imap_bridge::store::ObjectType::Email, before);
    assert_eq!(gone, [(email, ChangeKind::Destroyed)]);
    assert!(rows(&rig).iter().all(|row| row.facts.imap_name != "Work"));
}

/// Objects of one call see what the objects before them did: a parent
/// made in the call by its creation id, a child destroyed before its
/// parent, a role moved in one call, each side answering the sort order
/// and the rights that moved with it (RFC 8620 section 5.3).
#[tokio::test]
async fn one_call_runs_its_objects_in_order_against_what_the_earlier_ones_did_rfc8620_5_3() {
    let rig = started(dovecot(), false).await;
    let create = json!({
        "a": { "name": "Top" },
        "b": { "name": "Below", "parentId": "#a" },
        "c": { "name": "Lost", "parentId": "#nope" },
    });
    let answer = set(&rig, json!({ "create": create })).await;
    let top = answer["created"]["a"]["id"].as_str().unwrap().to_owned();
    assert_eq!(answer["created"]["b"]["parentId"], json!(top));
    assert_eq!(answer["notCreated"]["c"]["properties"], json!(["parentId"]));
    let below = id_of(&rig, "Top/Below");
    let both = set(&rig, json!({ "destroy": [&below, &top] })).await;
    assert_eq!(both["destroyed"], json!([below, top]), "{both}");
    let (archive, work) = (id_of(&rig, "Archive"), id_of(&rig, "Work"));
    let alone = set(
        &rig,
        json!({ "update": { (&work): { "role": "archive" } } }),
    )
    .await;
    assert_eq!(refused(&alone, "notUpdated", &work), "invalidProperties");
    let moved = json!({ (&archive): { "role": null }, (&work): { "role": "archive" } });
    let answer = set(&rig, json!({ "update": moved })).await;
    assert_eq!(answer["notUpdated"], json!(null), "{answer}");
    let changed = &answer["updated"];
    assert_eq!(changed[&work]["sortOrder"], 3, "{answer}");
    assert_eq!(changed[&work]["myRights"]["mayDelete"], false);
    assert_eq!(changed[&work].as_object().unwrap().len(), 2);
    assert_eq!(changed[&archive]["sortOrder"], 6);
    assert_eq!(changed[&archive]["myRights"]["mayDelete"], true);
    assert_eq!(mailbox(&rig, &work).await["role"], "archive");
    assert_eq!(mailbox(&rig, &archive).await["role"], json!(null));
    rig.pass().await.unwrap();
    assert_eq!(rig.folder("Work").facts.role.as_deref(), Some("archive"));
    assert_eq!(rig.folder("Archive").facts.role, None);
    assert_eq!(rig.folder("Work").facts.sort_order, 3);
    let extensions = rig.fake.script().mailboxes.extensions.clone();
    assert!(!extensions.contains(&Extension::Acl));
}
