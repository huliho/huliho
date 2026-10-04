// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! `Email/set` against the scripted server: a keyword patch is one
//! STORE on the selected folder and one state with the email and its
//! mailbox updated; a whole `keywords` object, a batch over one set of
//! UIDs, `ifInState`, the objects that cannot be set and the patches
//! that never reach the server.

mod set_rig;
mod stable_ids;
mod sync_rig;

use huliho_imap_bridge::jmap::{CORE_CAPABILITY, MAIL_CAPABILITY, MAX_OBJECTS_IN_SET, handle};
use huliho_imap_bridge::session::MAX_STORE_UIDS;
use huliho_imap_bridge::store::{AccountKey, EmailId, ObjectType};
use huliho_imap_bridge::sync::Cache;
use huliho_imap_bridge::testing::Message;
use serde_json::{Map, Value, json};
use set_rig::{Set, refused, started};
use sync_rig::{ACCOUNT, inbox};

const INBOX: &str = "INBOX";

/// A second account in the same store.
const OTHER_ACCOUNT: &str = "a2";

/// Three messages: an unseen one, a seen one and one that carries a
/// keyword beside `\Seen`.
fn mail() -> Vec<Message> {
    vec![
        Message::new(1).flagged(&[]),
        Message::new(2),
        Message::new(3).flagged(&["\\Seen", "work"]),
    ]
}

async fn synced() -> Set {
    started(inbox(mail()), INBOX, false).await
}

/// The answer with every email id swapped for a name that holds across
/// runs.
async fn stable(set: &Set, answer: &Value) -> Value {
    let ids: Vec<&String> = set.ids.values().collect();
    let arguments = json!({ "accountId": ACCOUNT, "ids": ids, "properties": ["size", "threadId"] });
    let listed = set.rig.call(&json!(["Email/get", arguments, "c1"])).await;
    let names = stable_ids::names(&set.rig, listed["list"].as_array().unwrap());
    let mut answer = answer.clone();
    stable_ids::rename(&mut answer, &names);
    answer
}

#[tokio::test]
async fn seen_on_and_off_is_one_store_and_one_state_each_rfc8621_4_6() {
    let set = synced().await;
    let before = set.state();
    let on = set.update(&[(1, json!({ "keywords/$seen": true }))]).await;
    insta::assert_json_snapshot!(stable(&set, &on).await);
    assert_eq!(
        set.written(),
        ["SELECT \"INBOX\"", "UID STORE 1 +FLAGS.SILENT (\\Seen)"]
    );
    assert_eq!(set.flags(INBOX, 1), ["\\Seen"]);
    assert_eq!(set.keywords(1).await, json!({ "$seen": true }));
    assert_eq!(
        set.updated(ObjectType::Email, before),
        [set.ids[&1].clone()]
    );
    let inbox_id = set.rig.folder(INBOX).id.to_string();
    assert_eq!(set.updated(ObjectType::Mailbox, before), [inbox_id]);
    let off = set.update(&[(1, json!({ "keywords/$seen": null }))]).await;
    assert_eq!(off["newState"], (before + 2).to_string(), "{off}");
    assert_eq!(set.written()[3], "UID STORE 1 -FLAGS.SILENT (\\Seen)");
    assert!(set.flags(INBOX, 1).is_empty());
    assert_eq!(set.keywords(1).await, json!({}));
}

#[tokio::test]
async fn the_unread_count_of_the_mailbox_follows_the_keyword() {
    let set = synced().await;
    let unread = async || {
        let arguments = json!({ "accountId": ACCOUNT, "ids": null });
        let got = set.rig.call(&json!(["Mailbox/get", arguments, "c1"])).await;
        got["list"][0]["unreadEmails"].as_u64().unwrap()
    };
    assert_eq!(unread().await, 1);
    set.update(&[(1, json!({ "keywords/$seen": true }))]).await;
    assert_eq!(unread().await, 0);
}

#[tokio::test]
async fn the_next_refresh_reads_its_own_store_back_and_logs_no_email() {
    let set = synced().await;
    let stored = set.update(&[(1, json!({ "keywords/$seen": true }))]).await;
    let arguments = json!({ "accountId": ACCOUNT, "sinceState": stored["newState"] });
    let changed = set
        .rig
        .call(&json!(["Email/changes", arguments, "c1"]))
        .await;
    // The mailbox pass logs the folder, whose unseen count moved.
    for list in ["created", "updated", "destroyed"] {
        assert_eq!(changed[list], json!([]), "{changed}");
    }
    let lines = set.rig.fake.lines();
    let refreshed = lines.iter().any(|line| line.contains("CHANGEDSINCE"));
    assert!(
        refreshed,
        "the refresh asked the flags since its mod-sequence"
    );
}

#[tokio::test]
async fn a_keyword_the_row_holds_is_stored_again_and_moves_no_state() {
    let set = synced().await;
    let before = set.state();
    let again = set
        .update(&[(2, json!({ "keywords/$seen": true })), (1, json!({}))])
        .await;
    assert_eq!(again["oldState"], again["newState"], "{again}");
    assert_eq!(again["updated"].as_object().unwrap().len(), 2);
    assert_eq!(set.state(), before);
    assert_eq!(
        set.written(),
        ["SELECT \"INBOX\"", "UID STORE 2 +FLAGS.SILENT (\\Seen)"]
    );
}

#[tokio::test]
async fn a_whole_keywords_object_adds_what_it_names_and_removes_the_rest() {
    let set = synced().await;
    let replaced = set
        .update(&[(3, json!({ "keywords": { "$Flagged": true } }))])
        .await;
    assert_eq!(replaced["notUpdated"], Value::Null, "{replaced}");
    assert_eq!(
        set.written()[1..],
        [
            "UID STORE 3 +FLAGS.SILENT (\\Flagged)",
            "UID STORE 3 -FLAGS.SILENT (\\Seen work)"
        ]
    );
    assert_eq!(set.flags(INBOX, 3), ["\\Flagged"]);
    assert_eq!(set.keywords(3).await, json!({ "$flagged": true }));
}

#[tokio::test]
async fn the_messages_of_one_patch_share_a_store_over_bounded_sets() {
    let many = u32::try_from(MAX_STORE_UIDS).unwrap() + 50;
    let mail = (1..=many)
        .map(|uid| Message::new(uid).flagged(&[]))
        .collect();
    let set = started(inbox(mail), INBOX, false).await;
    let before = set.state();
    let patches: Vec<(u32, Value)> = (1..=many)
        .map(|uid| (uid, json!({ "keywords/$seen": true })))
        .collect();
    let stored = set.update(&patches).await;
    assert_eq!(
        stored["updated"].as_object().unwrap().len(),
        usize::try_from(many).unwrap()
    );
    assert_eq!(stored["newState"], (before + 1).to_string(), "one state");
    assert_eq!(
        set.written()[1..],
        [
            "UID STORE 1:100 +FLAGS.SILENT (\\Seen)",
            "UID STORE 101:150 +FLAGS.SILENT (\\Seen)"
        ]
    );
    assert_eq!(
        set.updated(ObjectType::Email, before).len(),
        usize::try_from(many).unwrap()
    );
}

#[tokio::test]
async fn if_in_state_is_honored_before_anything_is_sent_rfc8620_5_3() {
    let set = synced().await;
    let state = set.state();
    let update = json!({ &set.ids[&1]: { "keywords/$seen": true } });
    let behind = json!({
        "accountId": ACCOUNT,
        "ifInState": (state - 1).to_string(),
        "update": update,
    });
    let refused = set.rig.calls(&[json!(["Email/set", behind, "c1"])]).await;
    assert_eq!(refused["methodResponses"][0][0], "error", "{refused}");
    assert_eq!(refused["methodResponses"][0][1]["type"], "stateMismatch");
    assert!(set.written().is_empty());
    let current = json!({ "ifInState": state.to_string(), "update": update });
    let stored = set.set(current).await;
    assert_eq!(stored["oldState"], state.to_string(), "{stored}");
    assert_eq!(set.flags(INBOX, 1), ["\\Seen"]);
}

#[tokio::test]
async fn a_create_and_a_destroy_are_forbidden_per_object_rfc8620_5_3() {
    let set = synced().await;
    let answered = set
        .set(json!({
            "create": { "k1": { "subject": "x" } },
            "destroy": [set.ids[&2]],
        }))
        .await;
    assert_eq!(
        answered["notCreated"]["k1"]["type"], "forbidden",
        "{answered}"
    );
    assert_eq!(answered["notDestroyed"][&set.ids[&2]]["type"], "forbidden");
    assert_eq!(answered["oldState"], answered["newState"]);
    assert_eq!(set.keywords(2).await, json!({ "$seen": true }));
    assert!(set.written().is_empty());
}

#[tokio::test]
async fn an_id_the_account_does_not_hold_is_not_found_and_nothing_is_sent() {
    let set = synced().await;
    let unknown = EmailId::generate().to_string();
    let update = json!({ &unknown: { "keywords/$seen": true }, "nonsense": {} });
    let answered = set.set(json!({ "update": update })).await;
    assert_eq!(
        answered["notUpdated"][&unknown]["type"], "notFound",
        "{answered}"
    );
    assert_eq!(answered["notUpdated"]["nonsense"]["type"], "notFound");
    assert_eq!(answered["updated"], Value::Null);
    assert!(set.written().is_empty());
}

#[tokio::test]
async fn an_email_of_another_account_is_not_found_and_stays_as_it_was() {
    let set = synced().await;
    let other = Cache {
        key: AccountKey::new(OTHER_ACCOUNT),
        ..set.rig.cache.clone()
    };
    let update = json!({ &set.ids[&1]: { "keywords/$seen": true } });
    let request = |account: &str| {
        let arguments = json!({ "accountId": account, "update": update });
        let body = json!({
            "using": [CORE_CAPABILITY, MAIL_CAPABILITY],
            "methodCalls": [["Email/set", arguments, "c1"]],
        });
        serde_json::to_vec(&body).unwrap()
    };
    let answered = handle(&other, &set.rig.link, &request(OTHER_ACCOUNT), "s")
        .await
        .unwrap();
    let answered: Value = serde_json::from_slice(&answered).unwrap();
    let refused = &answered["methodResponses"][0][1]["notUpdated"][&set.ids[&1]];
    assert_eq!(refused["type"], "notFound", "{answered}");
    // The account of the endpoint is the one a call may name.
    let foreign = handle(&other, &set.rig.link, &request(ACCOUNT), "s")
        .await
        .unwrap();
    let foreign: Value = serde_json::from_slice(&foreign).unwrap();
    assert_eq!(foreign["methodResponses"][0][1]["type"], "accountNotFound");
    assert!(set.written().is_empty());
    assert_eq!(set.keywords(1).await, json!({}));
}

#[tokio::test]
async fn any_other_property_and_any_other_value_are_named_back_and_never_sent() {
    let set = synced().await;
    let answered = set
        .update(&[
            (1, json!({ "mailboxIds/x": true })),
            (2, json!({ "keywords/$seen": false })),
            (3, json!({ "keywords/a b": true, "keywords/x)": true })),
        ])
        .await;
    assert_eq!(
        refused(&set, &answered, 1),
        "invalidProperties",
        "{answered}"
    );
    let named = |uid: u32| answered["notUpdated"][&set.ids[&uid]]["properties"].clone();
    assert_eq!(named(1), json!(["mailboxIds/x"]));
    assert_eq!(named(2), json!(["keywords/$seen"]));
    assert_eq!(named(3), json!(["keywords/a b", "keywords/x)"]));
    assert!(set.written().is_empty());
    let both = set
        .update(&[(1, json!({ "keywords": {}, "keywords/$seen": true }))])
        .await;
    assert_eq!(refused(&set, &both, 1), "invalidPatch");
    assert!(set.written().is_empty());
}

#[tokio::test]
async fn one_object_past_the_limit_is_too_large_and_the_limit_itself_passes_rfc8620_5_3() {
    let set = synced().await;
    let unknown = |count: usize| -> Map<String, Value> {
        (0..count)
            .map(|_| (EmailId::generate().to_string(), json!({})))
            .collect()
    };
    let past = json!({ "accountId": ACCOUNT, "update": unknown(MAX_OBJECTS_IN_SET + 1) });
    let answered = set.rig.calls(&[json!(["Email/set", past, "c1"])]).await;
    assert_eq!(answered["methodResponses"][0][1]["type"], "requestTooLarge");
    let mixed = json!({
        "update": unknown(MAX_OBJECTS_IN_SET - 1),
        "destroy": [set.ids[&1]],
    });
    let at = set.set(mixed).await;
    assert_eq!(
        at["notUpdated"].as_object().unwrap().len(),
        MAX_OBJECTS_IN_SET - 1
    );
}
