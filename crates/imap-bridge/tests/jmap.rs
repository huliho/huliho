// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The session object, `Mailbox/get`, `Core/echo` and every error the
//! request parser answers.

mod jmap_rig;

use huliho_imap_bridge::jmap::{
    CORE_CAPABILITY, HULIHO_CAPABILITY, MAIL_CAPABILITY, MAX_CALLS_IN_REQUEST, MAX_OBJECTS_IN_GET,
    MAX_SIZE_REQUEST, RequestError, Urls, session_object,
};
use huliho_imap_bridge::runtime::Registration;
use huliho_imap_bridge::store::AccountKey;
use jmap_rig::{ACCOUNT, Rig, SESSION_STATE, error_type, first, mailbox_get};
use serde_json::{Value, json};

const ADDRESS: &str = "sanne@example.test";

/// The rig's account as the host registers it.
fn registration() -> Registration {
    Registration {
        key: AccountKey::new(ACCOUNT),
        gmail: false,
        session_state: SESSION_STATE.to_owned(),
    }
}

fn urls() -> Urls {
    Urls {
        api: "/api/jmap/a1".to_owned(),
        download: "/api/jmap/a1/download/{accountId}/{blobId}/{name}?type={type}".to_owned(),
        upload: "/api/jmap/a1/upload/{accountId}".to_owned(),
        event_source: "/api/jmap/a1/events?types={types}&closeafter={closeafter}&ping={ping}"
            .to_owned(),
    }
}

fn keys(value: &Value) -> Vec<&str> {
    value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect()
}

#[tokio::test]
async fn the_session_object_carries_the_limits_and_all_six_mail_properties_rfc8620_2() {
    let rig = Rig::start().await;
    rig.edit();
    assert_eq!(rig.pass().await, 2);
    let bytes = session_object(&registration(), ADDRESS, &urls()).unwrap();
    let session: Value = serde_json::from_slice(&bytes).unwrap();
    // The state is the host's; the counter of the cache stands at 2.
    assert_eq!(session["state"], SESSION_STATE);
    let answered = rig.mail(json!([mailbox_get("c1")])).await;
    assert_eq!(answered["sessionState"], SESSION_STATE);
    assert_eq!(session["username"], ADDRESS);
    assert_eq!(session["apiUrl"], "/api/jmap/a1");
    assert_eq!(session["eventSourceUrl"], urls().event_source);
    assert_eq!(
        keys(&session["capabilities"]),
        [HULIHO_CAPABILITY, CORE_CAPABILITY, MAIL_CAPABILITY]
    );
    let core = &session["capabilities"][CORE_CAPABILITY];
    assert_eq!(core["maxSizeRequest"], 1_048_576);
    assert_eq!(core["maxConcurrentRequests"], 4);
    assert_eq!(core["maxCallsInRequest"], 16);
    assert_eq!(core["maxObjectsInGet"], 500);
    assert_eq!(core["maxObjectsInSet"], 0);
    assert_eq!(core["collationAlgorithms"], json!(["i;unicode-casemap"]));
    assert_eq!(session["capabilities"][MAIL_CAPABILITY], json!({}));
    let mail = &session["accounts"][ACCOUNT]["accountCapabilities"][MAIL_CAPABILITY];
    assert_eq!(
        keys(mail),
        [
            "emailQuerySortOptions",
            "maxMailboxDepth",
            "maxMailboxesPerEmail",
            "maxSizeAttachmentsPerEmail",
            "maxSizeMailboxName",
            "mayCreateTopLevelMailbox"
        ]
    );
    assert_eq!(mail["maxMailboxesPerEmail"], 1);
    assert_eq!(mail["maxMailboxDepth"], Value::Null);
    assert_eq!(mail["maxSizeMailboxName"], 255);
    let account = &session["accounts"][ACCOUNT];
    assert_eq!(account["name"], ADDRESS);
    assert_eq!(
        (&account["isPersonal"], &account["isReadOnly"]),
        (&json!(true), &json!(true))
    );
    assert_eq!(
        keys(&account["accountCapabilities"]),
        [HULIHO_CAPABILITY, MAIL_CAPABILITY]
    );
    assert_eq!(session["primaryAccounts"][MAIL_CAPABILITY], ACCOUNT);
    assert_eq!(session["primaryAccounts"][HULIHO_CAPABILITY], ACCOUNT);
}

#[tokio::test]
async fn mailbox_get_answers_every_mailbox_with_its_rights_and_counts_rfc8621_2() {
    let rig = Rig::start().await;
    let response = rig.mail(json!([mailbox_get("c1")])).await;
    assert_eq!(response["sessionState"], SESSION_STATE);
    let answer = first(&response);
    assert_eq!(answer[0], "Mailbox/get");
    assert_eq!(answer[2], "c1");
    assert_eq!(answer[1]["accountId"], ACCOUNT);
    assert_eq!(answer[1]["state"], "1");
    assert_eq!(answer[1]["notFound"], json!([]));
    let list = answer[1]["list"].as_array().unwrap();
    assert_eq!(list.len(), 6);
    let inbox = &list[0];
    assert_eq!(inbox["id"], rig.id_of("INBOX"));
    assert_eq!(inbox["name"], "INBOX");
    assert_eq!(inbox["role"], "inbox");
    assert_eq!(inbox["parentId"], Value::Null);
    assert_eq!(inbox["sortOrder"], 0);
    assert_eq!(
        (&inbox["totalEmails"], &inbox["unreadEmails"]),
        (&json!(17), &json!(3))
    );
    assert_eq!(
        (&inbox["totalThreads"], &inbox["unreadThreads"]),
        (&json!(0), &json!(0))
    );
    assert_eq!(inbox["isSubscribed"], true);
    assert_eq!(inbox["myRights"]["mayReadItems"], true);
    assert_eq!(inbox["myRights"]["mayAddItems"], false);
    assert_eq!(keys(&inbox["myRights"]).len(), 9);
    assert_eq!(inbox.get("syncedEmails"), None);
    assert_eq!(list[5]["role"], "trash");
}

#[tokio::test]
async fn mailbox_get_with_ids_answers_each_id_once_and_cuts_to_the_properties_rfc8620_5_1() {
    let rig = Rig::start().await;
    let sent = rig.id_of("Sent");
    let inbox = rig.id_of("INBOX");
    let response = rig
        .mail(json!([[
            "Mailbox/get",
            {
                "accountId": ACCOUNT,
                "ids": [sent, "mnope", inbox, sent, "mnope", "mother"],
                "properties": ["name", "role"]
            },
            "c1"
        ]]))
        .await;
    let answer = &first(&response)[1];
    assert_eq!(answer["notFound"], json!(["mnope", "mother"]));
    assert_eq!(
        answer["list"],
        json!([
            { "id": sent, "name": "Sent", "role": "sent" },
            { "id": inbox, "name": "INBOX", "role": "inbox" }
        ])
    );
    let unknown = rig
        .mail(json!([[
            "Mailbox/get",
            { "accountId": ACCOUNT, "ids": null, "properties": ["flavor"] },
            "c2"
        ]]))
        .await;
    assert_eq!(error_type(first(&unknown)), Some("invalidArguments"));
    let none = rig
        .mail(json!([[
            "Mailbox/get",
            { "accountId": ACCOUNT, "ids": [] },
            "c3"
        ]]))
        .await;
    let answer = &first(&none)[1];
    assert_eq!(
        (&answer["list"], &answer["notFound"]),
        (&json!([]), &json!([]))
    );
}

#[tokio::test]
async fn synced_emails_exists_only_under_the_vendor_capability_rfc8620_1_8() {
    let rig = Rig::start().await;
    let with = rig
        .request(
            &[CORE_CAPABILITY, MAIL_CAPABILITY, HULIHO_CAPABILITY],
            json!([mailbox_get("c1")]),
        )
        .await
        .unwrap();
    assert_eq!(first(&with)[1]["list"][0]["syncedEmails"], 0);
    let named = json!([[
        "Mailbox/get",
        { "accountId": ACCOUNT, "ids": null, "properties": ["syncedEmails"] },
        "c1"
    ]]);
    let asked = rig
        .request(
            &[CORE_CAPABILITY, MAIL_CAPABILITY, HULIHO_CAPABILITY],
            named.clone(),
        )
        .await
        .unwrap();
    assert_eq!(keys(&first(&asked)[1]["list"][0]), ["id", "syncedEmails"]);
    let without = rig.mail(named).await;
    assert_eq!(error_type(first(&without)), Some("invalidArguments"));
}

#[tokio::test]
async fn core_echo_returns_its_arguments_rfc8620_4() {
    let rig = Rig::start().await;
    let response = rig
        .request(
            &[CORE_CAPABILITY],
            json!([["Core/echo", { "hello": "world", "n": 1 }, "c1"]]),
        )
        .await;
    let response = response.unwrap();
    assert_eq!(
        first(&response),
        &json!(["Core/echo", { "hello": "world", "n": 1 }, "c1"])
    );
    let unnamed = rig
        .request(&[MAIL_CAPABILITY], json!([["Core/echo", {}, "c1"]]))
        .await;
    assert_eq!(error_type(first(&unnamed.unwrap())), Some("unknownMethod"));
}

#[tokio::test]
async fn a_request_that_cannot_run_answers_its_problem_rfc8620_3_6_1() {
    let rig = Rig::start().await;
    assert!(matches!(
        rig.raw(b"not json").await.unwrap_err(),
        RequestError::NotJson
    ));
    assert!(matches!(
        rig.raw(br#"{"using": []}"#).await.unwrap_err(),
        RequestError::NotRequest
    ));
    let websocket = rig
        .request(&["urn:ietf:params:jmap:websocket"], json!([]))
        .await;
    assert!(matches!(
        websocket.unwrap_err(),
        RequestError::UnknownCapability
    ));
    let calls: Vec<Value> = (0..=MAX_CALLS_IN_REQUEST)
        .map(|index| json!(["Core/echo", {}, index.to_string()]))
        .collect();
    let many = rig.request(&[CORE_CAPABILITY], Value::Array(calls)).await;
    assert_eq!(many.unwrap_err().limit(), Some("maxCallsInRequest"));
    let filler = "x".repeat(MAX_SIZE_REQUEST);
    let large = rig
        .request(
            &[CORE_CAPABILITY],
            json!([["Core/echo", { "f": filler }, "c1"]]),
        )
        .await;
    assert_eq!(large.unwrap_err().limit(), Some("maxSizeRequest"));
    let empty = rig.request(&[], json!([])).await.unwrap();
    assert_eq!(empty["methodResponses"], json!([]));
    assert_eq!(empty.get("createdIds"), None);
}

#[tokio::test]
async fn created_ids_come_back_as_they_were_sent_rfc8620_3_4() {
    let rig = Rig::start().await;
    let body = json!({ "using": [], "methodCalls": [], "createdIds": { "a": "m1" } });
    let body = serde_json::to_vec(&body).unwrap();
    let bytes = rig.raw(&body).await.unwrap();
    let response: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(response["createdIds"], json!({ "a": "m1" }));
}

#[tokio::test]
async fn a_method_that_cannot_run_answers_inside_the_response_rfc8620_3_6_2() {
    let rig = Rig::start().await;
    let cases = [
        (&[CORE_CAPABILITY][..], mailbox_get("c1"), "unknownMethod"),
        (
            &[CORE_CAPABILITY, MAIL_CAPABILITY][..],
            json!(["Email/queryChanges", { "accountId": ACCOUNT }, "c1"]),
            "unknownMethod",
        ),
        (
            &[CORE_CAPABILITY, MAIL_CAPABILITY][..],
            json!(["Mailbox/get", { "accountId": "other", "ids": null }, "c1"]),
            "accountNotFound",
        ),
        (
            &[CORE_CAPABILITY, MAIL_CAPABILITY][..],
            json!(["Mailbox/get", { "accountId": ACCOUNT, "ids": null, "extra": 1 }, "c1"]),
            "invalidArguments",
        ),
        (
            &[CORE_CAPABILITY, MAIL_CAPABILITY][..],
            json!(["Mailbox/get", { "accountId": ACCOUNT, "ids": vec!["m"; MAX_OBJECTS_IN_GET + 1] }, "c1"]),
            "requestTooLarge",
        ),
    ];
    for (using, call, expected) in cases {
        let response = rig.request(using, json!([call])).await.unwrap();
        assert_eq!(error_type(first(&response)), Some(expected), "{expected}");
        assert_eq!(first(&response)[2], "c1");
    }
}
