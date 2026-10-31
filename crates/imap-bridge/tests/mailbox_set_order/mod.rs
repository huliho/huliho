// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The objects of one call and the calls of one request see what ran
//! before them (RFC 8620 section 5.3): a later step names a mailbox as
//! the RENAMEs before it left it, and a creation id resolves in its own
//! call, in the calls after it and from the `createdIds` the request
//! came with.

use huliho_imap_bridge::store::ChangeKind;
use serde_json::{Value, json};

use crate::folders_rig::{
    dovecot, id_of, mailbox, mailboxes_changed, refused, request, rows, sent, set, started, state,
};
use crate::sync_rig::{ACCOUNT, Rig};

/// A `Mailbox/set` call with these arguments beside the account.
fn mailbox_set(arguments: Value, call_id: &str) -> Value {
    let mut arguments = arguments;
    arguments["accountId"] = json!(ACCOUNT);
    json!(["Mailbox/set", arguments, call_id])
}

/// The mailbox ids the one email of `Work` is a member of.
async fn memberships(rig: &Rig, email: &str) -> Value {
    let arguments = json!({ "accountId": ACCOUNT, "ids": [email], "properties": ["mailboxIds"] });
    let got = rig.call(&json!(["Email/get", arguments, "c1"])).await;
    got["list"][0]["mailboxIds"].clone()
}

/// Two mailboxes made in the call and renamed in it, the parent first:
/// the child's RENAME names the parent as the first RENAME left it, and
/// the ids stay.
#[tokio::test]
async fn a_rename_moves_the_names_the_later_steps_of_the_call_send_rfc3501_6_3_5() {
    let rig = started(dovecot(), false).await;
    let create = json!({ "a": { "name": "Top" }, "b": { "name": "Below", "parentId": "#a" } });
    let update = json!({ "#a": { "name": "Up" }, "#b": { "name": "Down" } });
    let answer = set(&rig, json!({ "create": create, "update": update })).await;
    let top = answer["created"]["a"]["id"].as_str().unwrap().to_owned();
    let below = answer["created"]["b"]["id"].as_str().unwrap().to_owned();
    assert_eq!(answer["created"]["b"]["parentId"], json!(top));
    assert_eq!(
        answer["updated"],
        json!({ (&top): null, (&below): null }),
        "{answer}"
    );
    assert_eq!(
        sent(&rig),
        [
            "CREATE \"Top\"",
            "SUBSCRIBE \"Top\"",
            "CREATE \"Top/Below\"",
            "SUBSCRIBE \"Top/Below\"",
            "RENAME \"Top\" \"Up\"",
            "SUBSCRIBE \"Up\"",
            "UNSUBSCRIBE \"Top\"",
            "SUBSCRIBE \"Up/Below\"",
            "UNSUBSCRIBE \"Top/Below\"",
            "RENAME \"Up/Below\" \"Up/Down\"",
            "SUBSCRIBE \"Up/Down\"",
            "UNSUBSCRIBE \"Up/Below\""
        ]
    );
    assert_eq!(
        (id_of(&rig, "Up"), id_of(&rig, "Up/Down")),
        (top.clone(), below.clone())
    );
    let after = state(&rig);
    rig.pass().await.unwrap();
    let logged = mailboxes_changed(&rig, after);
    assert!(
        logged
            .iter()
            .all(|(id, kind)| (*id == top || *id == below) && *kind == ChangeKind::Updated),
        "{logged:?}"
    );
    assert_eq!((id_of(&rig, "Up"), id_of(&rig, "Up/Down")), (top, below));
}

/// A parent renamed by an update and its child destroyed in the same
/// call: the DELETE names the child under the new name.
#[tokio::test]
async fn a_rename_moves_the_name_a_later_destroy_of_the_call_sends_rfc3501_6_3_5() {
    let rig = started(dovecot(), false).await;
    let (work, q3) = (id_of(&rig, "Work"), id_of(&rig, "Work/Q3"));
    let call = json!({ "update": { (&work): { "name": "Job" } }, "destroy": [&q3] });
    let answer = set(&rig, call).await;
    assert_eq!(answer["updated"][&work], json!(null), "{answer}");
    assert_eq!(answer["destroyed"], json!([q3]), "{answer}");
    assert_eq!(
        sent(&rig),
        [
            "RENAME \"Work\" \"Job\"",
            "SUBSCRIBE \"Job\"",
            "UNSUBSCRIBE \"Work\"",
            "SUBSCRIBE \"Job/Q3\"",
            "UNSUBSCRIBE \"Work/Q3\"",
            "DELETE \"Job/Q3\"",
            "UNSUBSCRIBE \"Job/Q3\""
        ]
    );
    assert_eq!(id_of(&rig, "Job"), work);
    assert!(rows(&rig).iter().all(|row| row.facts.imap_name != "Job/Q3"));
    let after = state(&rig);
    rig.pass().await.unwrap();
    assert_eq!(mailboxes_changed(&rig, after), []);
}

/// A mailbox made and destroyed in one call: `created` still carries its
/// id and `destroyed` names it (RFC 8620 section 5.3).
#[tokio::test]
async fn a_mailbox_made_and_destroyed_in_one_call_answers_its_id_in_both_lists_rfc8620_5_3() {
    let rig = started(dovecot(), false).await;
    let call = json!({ "create": { "b": { "name": "Brief" } }, "destroy": ["#b"] });
    let answer = set(&rig, call).await;
    let id = answer["created"]["b"]["id"].clone();
    assert!(id.is_string(), "{answer}");
    assert_eq!(answer["destroyed"], json!([id]), "{answer}");
    assert_eq!(
        sent(&rig),
        [
            "CREATE \"Brief\"",
            "SUBSCRIBE \"Brief\"",
            "DELETE \"Brief\"",
            "UNSUBSCRIBE \"Brief\""
        ]
    );
    assert!(rows(&rig).iter().all(|row| row.facts.imap_name != "Brief"));
}

/// A role an update of the call sets on a mailbox the call creates
/// lands on it: the create runs first, so the row holds the role,
/// `Mailbox/get` shows it, the log and `Mailbox/changes` list the
/// mailbox as created and the update answers null (RFC 8620 section
/// 5.3).
#[tokio::test]
async fn a_role_set_on_a_mailbox_the_same_call_creates_lands_on_it_rfc8620_5_3() {
    let rig = started(dovecot(), false).await;
    let before = state(&rig);
    let call = json!({
        "create": { "a": { "name": "New" } },
        "update": { "#a": { "role": "important" } },
    });
    let answer = set(&rig, call).await;
    let id = answer["created"]["a"]["id"].as_str().unwrap().to_owned();
    assert_eq!(answer["created"]["a"]["role"], "important", "{answer}");
    assert_eq!(answer["updated"], json!({ (&id): null }), "{answer}");
    assert_eq!(answer["notUpdated"], json!(null), "{answer}");
    assert_eq!(sent(&rig), ["CREATE \"New\"", "SUBSCRIBE \"New\""]);
    assert_eq!(mailbox(&rig, &id).await["role"], "important");
    assert_eq!(
        mailboxes_changed(&rig, before),
        [(id.clone(), ChangeKind::Created)]
    );
    let changes = json!({ "accountId": ACCOUNT, "sinceState": before.to_string() });
    let listed = rig.call(&json!(["Mailbox/changes", changes, "c1"])).await;
    assert_eq!(listed["created"], json!([id]), "{listed}");
    assert_eq!(listed["updated"], json!([]), "{listed}");
    assert_eq!(rig.folder("New").facts.role.as_deref(), Some("important"));
}

/// An update naming a creation id whose CREATE the server refused
/// names no mailbox: it answers `notFound`, nothing is written and the
/// state stands (RFC 8620 section 5.3).
#[tokio::test]
async fn an_update_of_a_mailbox_whose_create_the_server_refused_is_not_found_rfc8620_5_3() {
    let mut mailboxes = dovecot();
    mailboxes.behavior.folder_refusal = Some("ALREADYEXISTS");
    let rig = started(mailboxes, false).await;
    let before = state(&rig);
    let call = json!({
        "create": { "a": { "name": "New" } },
        "update": { "#a": { "role": "important" } },
    });
    let answer = set(&rig, call).await;
    assert_eq!(refused(&answer, "notCreated", "a"), "serverFail");
    assert_eq!(refused(&answer, "notUpdated", "#a"), "notFound");
    assert_eq!(answer["updated"], json!(null), "{answer}");
    assert_eq!(answer["oldState"], answer["newState"], "{answer}");
    assert_eq!(state(&rig), before);
    assert_eq!(sent(&rig), ["CREATE \"New\""]);
    assert!(rows(&rig).iter().all(|row| row.facts.imap_name != "New"));
}

/// A mailbox a `Mailbox/set` creates is where an `Email/set` after it
/// in the same request moves a message by its creation id; an
/// `Email/set` before the `Mailbox/set` knows no such mailbox.
#[tokio::test]
async fn a_creation_id_of_a_mailbox_set_resolves_in_the_email_set_after_it_rfc8620_5_3() {
    let rig = started(dovecot(), false).await;
    rig.sync("Work").await;
    let email = rig.created(0).remove(0);
    let work = id_of(&rig, "Work");
    let early = json!({
        "accountId": ACCOUNT,
        "update": { (&email): { "mailboxIds/#new": true, (format!("mailboxIds/{work}")): null } },
    });
    let calls = [
        json!(["Email/set", early, "c0"]),
        mailbox_set(json!({ "create": { "new": { "name": "Plan" } } }), "c1"),
    ];
    let response = request(&rig, &calls, None).await;
    let answers = &response["methodResponses"];
    assert_eq!(
        answers[0][1]["notUpdated"][&email]["type"],
        "invalidProperties"
    );
    assert_eq!(
        answers[0][1]["notUpdated"][&email]["properties"],
        json!(["mailboxIds/#new"])
    );
    let plan = answers[1][1]["created"]["new"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(response.get("createdIds"), None);
    let late = json!({
        "accountId": ACCOUNT,
        "update": { (&email): { "mailboxIds/#later": true, (format!("mailboxIds/{work}")): null } },
    });
    let calls = [
        mailbox_set(json!({ "create": { "later": { "name": "Later" } } }), "c0"),
        json!(["Email/set", late, "c1"]),
    ];
    let response = request(&rig, &calls, None).await;
    let answers = &response["methodResponses"];
    let later = answers[0][1]["created"]["later"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        answers[1][1]["updated"],
        json!({ (&email): null }),
        "{response}"
    );
    assert_eq!(memberships(&rig, &email).await, json!({ (&later): true }));
    assert_ne!(plan, later);
}

/// A creation id of one `Mailbox/set` names the mailbox in the next
/// call, the `createdIds` the request came with resolve as well and
/// the answer carries the seed with every id the request made; a
/// creation id used twice names the later mailbox. An update or a
/// destroy that landed answers under the mailbox id, one that failed
/// under the reference as written.
#[tokio::test]
async fn created_ids_carry_across_the_calls_and_come_back_with_the_seed_rfc8620_3_3() {
    let rig = started(dovecot(), false).await;
    let work = id_of(&rig, "Work");
    let calls = [
        mailbox_set(json!({ "create": { "a": { "name": "One" } } }), "c0"),
        mailbox_set(
            json!({
                "create": { "b": { "name": "Two", "parentId": "#a" } },
                "update": { "#w": { "name": "Job" }, "#gone": { "name": "Nope" } },
                "destroy": ["#missing"],
            }),
            "c1",
        ),
        mailbox_set(json!({ "create": { "a": { "name": "Three" } } }), "c2"),
    ];
    let seed = json!({ "w": work });
    let response = request(&rig, &calls, Some(seed)).await;
    let answers = &response["methodResponses"];
    let one = answers[0][1]["created"]["a"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let two = answers[1][1]["created"]["b"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let three = answers[2][1]["created"]["a"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(answers[1][1]["created"]["b"]["parentId"], json!(one));
    assert_eq!(answers[1][1]["notUpdated"]["#gone"]["type"], "notFound");
    assert_eq!(
        answers[1][1]["notDestroyed"]["#missing"]["type"],
        "notFound"
    );
    assert_eq!(
        answers[1][1]["updated"],
        json!({ (&work): null }),
        "{response}"
    );
    assert_eq!(id_of(&rig, "Job"), work);
    assert_eq!(id_of(&rig, "One/Two"), two);
    assert_eq!(
        response["createdIds"],
        json!({ "w": work, "a": three, "b": two })
    );
}
