// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A submission refused: from the rows and the message before anything
//! connects, or by the SMTP server, each reply mapped to its error with
//! none of the server's words; the draft stays and nothing reaches Sent
//! (RFC 8621 section 7.5, RFC 5321 section 4.2, RFC 3463).

use std::time::Duration;

use huliho_imap_bridge::runtime::Link;
use huliho_imap_bridge::session::{Target, TlsMode};
use huliho_imap_bridge::testing::mailboxes::{ALL_MAIL, TRASH};
use huliho_imap_bridge::testing::smtp::{self, Refusal, Script};
use huliho_imap_bridge::testing::{CLOSED, Extension, Mailboxes, Message, Storing, Submitting};
use serde_json::{Value, json};

use crate::draft_rig::{DRAFTS, appends, held, object};
use crate::send_rig::{SENT, Sending, account, answers, over, refusal, sending};
use crate::set_rig::started;
use crate::sync_rig::ACCOUNT;

/// The bridge's bound on a message, which a refusal for size names where
/// the server named no SIZE.
const MAX_MESSAGE_BYTES: u64 = 40 * 1024 * 1024;

/// What the scripted server says when it refuses, none of which may
/// reach an answer.
const SERVER_WORDS: [&str; 6] = [
    "no such mailbox",
    "message refused",
    "sender refused",
    "too many recipients",
    "try again later",
    "message too big",
];

async fn refusing(refusal: Refusal) -> (Sending, String, Value) {
    let script = Script {
        refusal,
        ..Script::tls()
    };
    let sending = sending(account(Extension::all()), script).await;
    let draft = sending.draft(object(&sending.mailbox(DRAFTS))).await;
    let filing = sending.filing();
    let response = sending
        .submit(&draft, json!({ "onSuccessUpdateEmail": { "#s": filing } }))
        .await;
    let text = response.to_string();
    for words in SERVER_WORDS {
        assert!(!text.contains(words), "{text}");
    }
    (sending, draft, response)
}

/// Nothing went out: the draft stands, Sent is empty and the implicit
/// call the request asked for answers with nothing done at the
/// account's state.
fn kept(sending: &Sending, response: &Value) {
    assert_eq!(held(&sending.set, DRAFTS).len(), 1);
    assert_eq!(held(&sending.set, SENT).len(), 0);
    let (_, implicit) = answers(response);
    let implicit = implicit.unwrap_or_else(|| panic!("{response}"));
    for list in ["updated", "notUpdated", "destroyed", "notDestroyed"] {
        assert_eq!(implicit[list], Value::Null, "{list}: {response}");
    }
    assert_eq!(implicit["oldState"], implicit["newState"], "{response}");
}

#[tokio::test]
async fn a_recipient_refused_mid_list_is_named_and_no_data_goes_out_rfc5321_3_3() {
    let (sending, _, response) = refusing(Refusal::Recipient("li@")).await;
    assert_eq!(refusal(&response), "invalidRecipients");
    let (set, _) = answers(&response);
    assert_eq!(
        set["notCreated"]["s"]["invalidRecipients"],
        json!(["li@example.test"])
    );
    let transaction = sending.transaction();
    assert_eq!(transaction.len(), 5, "{transaction:?}");
    assert!(transaction[3].contains("hidden@"), "{transaction:?}");
    assert_eq!(transaction[4], "RSET");
    assert_eq!(sending.messages(), Vec::<String>::new());
    kept(&sending, &response);
}

#[tokio::test]
async fn too_many_recipients_names_the_count_that_passed_rfc3463_3_6() {
    let (sending, _, response) = refusing(Refusal::TooMany(1)).await;
    assert_eq!(refusal(&response), "tooManyRecipients");
    let (set, _) = answers(&response);
    assert_eq!(set["notCreated"]["s"]["maxRecipients"], 1);
    assert_eq!(sending.transaction().last().unwrap(), "RSET");
    kept(&sending, &response);
}

#[tokio::test]
async fn a_refused_sender_is_forbidden_and_one_to_try_later_keeps_the_send_waiting() {
    let (sending, _, response) = refusing(Refusal::Sender).await;
    assert_eq!(refusal(&response), "forbiddenMailFrom");
    kept(&sending, &response);
    let (sending, _, response) = refusing(Refusal::Later).await;
    assert_eq!(refusal(&response), "serverUnavailable");
    kept(&sending, &response);
}

#[tokio::test]
async fn a_message_past_the_servers_size_is_refused_before_mail_from_rfc1870_4() {
    let script = Script {
        size: 64,
        ..Script::tls()
    };
    let sending = sending(account(Extension::all()), script).await;
    let draft = sending.draft(object(&sending.mailbox(DRAFTS))).await;
    let response = sending.submit(&draft, json!({})).await;
    assert_eq!(refusal(&response), "tooLarge");
    let (set, _) = answers(&response);
    assert_eq!(set["notCreated"]["s"]["maxSize"], 64);
    assert!(
        sending.transaction().is_empty(),
        "{:?}",
        sending.transaction()
    );
}

#[tokio::test]
async fn a_refusal_after_the_message_keeps_the_draft_rfc5321_4_2_5() {
    let (sending, _, response) = refusing(Refusal::TooLarge).await;
    assert_eq!(refusal(&response), "tooLarge");
    let (set, _) = answers(&response);
    assert_eq!(set["notCreated"]["s"]["maxSize"], MAX_MESSAGE_BYTES);
    assert_eq!(sending.messages().len(), 1);
    kept(&sending, &response);
    let (sending, _, response) = refusing(Refusal::Rejected).await;
    assert_eq!(refusal(&response), "serverFail");
    kept(&sending, &response);
}

#[tokio::test]
async fn a_credential_the_smtp_server_refuses_is_forbidden_to_send_rfc4954_6() {
    let script = Script {
        accepts: false,
        ..Script::tls()
    };
    let sending = sending(account(Extension::all()), script).await;
    let draft = sending.draft(object(&sending.mailbox(DRAFTS))).await;
    let response = sending.submit(&draft, json!({})).await;
    assert_eq!(refusal(&response), "forbiddenToSend");
    assert_eq!(sending.transaction(), Vec::<String>::new());
}

#[tokio::test]
async fn a_server_out_of_reach_keeps_the_send_waiting() {
    let started = sending(account(Extension::all()), Script::tls()).await;
    let draft = started.draft(object(&started.mailbox(DRAFTS))).await;
    let mut connector: Submitting = started.connector.clone();
    connector.smtp = Target {
        host: smtp::HOST.to_owned(),
        addresses: vec![CLOSED],
        tls: TlsMode::Implicit,
    };
    let down = Sending {
        link: std::sync::Arc::new(Link::with_interval(connector.clone(), Duration::ZERO)),
        connector,
        ..started
    };
    let response = down.submit(&draft, json!({})).await;
    assert_eq!(refusal(&response), "serverUnavailable");
}

#[tokio::test]
async fn what_the_rows_refuse_connects_nothing_rfc8621_7_5() {
    let sending = sending(account(Extension::all()), Script::tls()).await;
    let draft = sending.draft(object(&sending.mailbox(DRAFTS))).await;
    let address = |email: &str| json!({ "email": email, "parameters": null });
    let cases = [
        (json!({ "identityId": "other" }), "invalidProperties"),
        (json!({ "emailId": "e-never" }), "invalidProperties"),
        (json!({ "undoStatus": "pending" }), "invalidProperties"),
        (
            json!({ "envelope": { "mailFrom": address("mo@example.test"), "rcptTo": [address("li@example.test")] } }),
            "forbiddenMailFrom",
        ),
        (
            json!({ "envelope": { "mailFrom": { "email": "sanne@example.test", "parameters": { "SIZE": "1" } }, "rcptTo": [address("li@example.test")] } }),
            "invalidProperties",
        ),
        (
            json!({ "envelope": { "mailFrom": address("sanne@example.test"), "rcptTo": [] } }),
            "noRecipients",
        ),
    ];
    for (fields, expected) in cases {
        let mut object = json!({ "identityId": ACCOUNT, "emailId": draft });
        for (name, value) in fields.as_object().unwrap() {
            object[name] = value.clone();
        }
        let arguments = json!({ "accountId": ACCOUNT, "create": { "s": object } });
        let response = sending
            .calls(&[json!(["EmailSubmission/set", arguments, "c1"])])
            .await;
        assert_eq!(refusal(&response), expected, "{fields}");
    }
    assert!(
        sending.smtp.lines().is_empty(),
        "{:?}",
        sending.smtp.lines()
    );
    assert_eq!(held(&sending.set, DRAFTS).len(), 1);
}

#[tokio::test]
async fn a_create_refused_from_the_rows_still_gets_the_implicit_answer_it_asked_for_rfc8621_7_5() {
    let sending = sending(account(Extension::all()), Script::tls()).await;
    let draft = sending.draft(object(&sending.mailbox(DRAFTS))).await;
    let asked = json!({
        "accountId": ACCOUNT,
        "create": { "s": { "identityId": "other", "emailId": draft } },
        "onSuccessUpdateEmail": { "#s": sending.filing() },
    });
    let call = |arguments: &Value| json!(["EmailSubmission/set", arguments, "c1"]);
    let response = sending.calls(&[call(&asked)]).await;
    assert_eq!(refusal(&response), "invalidProperties");
    let (_, implicit) = answers(&response);
    let implicit = implicit.unwrap_or_else(|| panic!("{response}"));
    let state = sending.set.state().to_string();
    assert_eq!(implicit["oldState"], state, "{response}");
    assert_eq!(implicit["newState"], state, "{response}");
    for list in [
        "created",
        "updated",
        "destroyed",
        "notCreated",
        "notUpdated",
        "notDestroyed",
    ] {
        assert_eq!(implicit[list], Value::Null, "{list}: {response}");
    }
    let mut unasked = asked;
    unasked["onSuccessUpdateEmail"] = Value::Null;
    let response = sending.calls(&[call(&unasked)]).await;
    assert_eq!(refusal(&response), "invalidProperties");
    assert!(answers(&response).1.is_none(), "{response}");
    assert!(
        sending.smtp.lines().is_empty(),
        "{:?}",
        sending.smtp.lines()
    );
}

#[tokio::test]
async fn a_message_that_is_not_the_identitys_goes_nowhere_rfc8621_7_5() {
    let sending = sending(account(Extension::all()), Script::tls()).await;
    let drafts = sending.mailbox(DRAFTS);
    let mut other = object(&drafts);
    other["from"] = json!([{ "email": "mo@example.test" }]);
    let foreign = sending.draft(other).await;
    let response = sending.submit(&foreign, json!({})).await;
    assert_eq!(refusal(&response), "forbiddenFrom");
    let mut two = object(&drafts);
    two["from"] = json!([{ "email": "sanne@example.test" }, { "email": "mo@example.test" }]);
    let shared = sending.draft(two).await;
    let response = sending.submit(&shared, json!({})).await;
    assert_eq!(refusal(&response), "invalidEmail");
    let (set, _) = answers(&response);
    assert_eq!(set["notCreated"]["s"]["properties"], json!(["from"]));
    let mut nobody = object(&drafts);
    for field in ["to", "bcc"] {
        nobody.as_object_mut().unwrap().remove(field);
    }
    let alone = sending.draft(nobody).await;
    let response = sending.submit(&alone, json!({})).await;
    assert_eq!(refusal(&response), "noRecipients");
    assert_eq!(sending.smtp.lines(), Vec::<String>::new());
}

#[tokio::test]
async fn a_given_envelope_names_the_messages_own_recipients_and_no_other_rfc8621_7() {
    let sending = sending(account(Extension::all()), Script::tls()).await;
    let draft = sending.draft(object(&sending.mailbox(DRAFTS))).await;
    let address = |email: &str| json!({ "email": email, "parameters": null });
    let envelope = |recipients: &[&str]| {
        let listed: Vec<Value> = recipients.iter().map(|email| address(email)).collect();
        json!({ "create": { "s": { "envelope": {
            "mailFrom": address("Sanne@Example.Test"),
            "rcptTo": listed,
        } } } })
    };
    let beyond = envelope(&["li@example.test", "kim@example.test"]);
    let response = sending.submit(&draft, beyond).await;
    assert_eq!(refusal(&response), "invalidRecipients");
    let (set, _) = answers(&response);
    assert_eq!(
        set["notCreated"]["s"]["invalidRecipients"],
        json!(["kim@example.test"])
    );
    assert!(
        sending.smtp.lines().is_empty(),
        "{:?}",
        sending.smtp.lines()
    );
    let response = sending.submit(&draft, envelope(&["Li@example.test"])).await;
    let (set, _) = answers(&response);
    assert!(set["created"]["s"]["id"].is_string(), "{response}");
    assert_eq!(set["created"]["s"].get("envelope"), None, "{response}");
    assert_eq!(
        sending.transaction().join(" "),
        "MAIL FROM:<sanne@example.test> RCPT TO:<li@example.test> DATA"
    );
    assert_eq!(held(&sending.set, DRAFTS).len(), 1);
    let derived = json!({ "create": { "s": { "envelope": null } } });
    let response = sending.submit(&draft, derived).await;
    assert!(answers(&response).0["created"]["s"]["envelope"].is_object());
}

#[tokio::test]
async fn a_refused_sent_copy_keeps_the_draft_and_names_the_failure_rfc8621_7_5() {
    let mut mailboxes = account(Extension::all());
    mailboxes.append(DRAFTS, Message::new(7).flagged(&["\\Draft", "\\Seen"]));
    mailboxes.behavior.appending = Storing::Refuses;
    let sending = sending(mailboxes, Script::tls()).await;
    let query = json!(["Email/query", {
        "accountId": ACCOUNT,
        "filter": { "inMailbox": sending.mailbox(DRAFTS) },
    }, "q"]);
    let listed = sending.set.rig.call(&query).await;
    let draft = listed["ids"][0].as_str().unwrap().to_owned();
    let filing = sending.filing();
    let response = sending
        .submit(&draft, json!({ "onSuccessUpdateEmail": { "#s": filing } }))
        .await;
    let (set, implicit) = answers(&response);
    assert!(set["created"]["s"]["id"].is_string(), "{response}");
    let implicit = implicit.unwrap_or_else(|| panic!("{response}"));
    assert_eq!(implicit["notUpdated"][&draft]["type"], "serverFail");
    assert_eq!(held(&sending.set, DRAFTS).len(), 1);
    assert_eq!(held(&sending.set, SENT).len(), 0);
}

#[tokio::test]
async fn a_failure_of_the_store_after_the_send_keeps_the_submissions_answer_rfc8621_7_5() {
    let sending = sending(account(Extension::all()), Script::tls()).await;
    let draft = sending.draft(object(&sending.mailbox(DRAFTS))).await;
    let filing = sending.filing();
    // A forgotten account refuses every write of the store, so the row of
    // the Sent copy fails once the message went out.
    let cache = &sending.set.rig.cache;
    cache.store.forget(&cache.key);
    let response = sending
        .submit(&draft, json!({ "onSuccessUpdateEmail": { "#s": filing } }))
        .await;
    let (set, implicit) = answers(&response);
    assert!(set["created"]["s"]["id"].is_string(), "{response}");
    let implicit = implicit.unwrap_or_else(|| panic!("{response}"));
    assert_eq!(
        implicit["notUpdated"][&draft]["type"], "serverFail",
        "{response}"
    );
    assert_eq!(implicit["updated"], Value::Null, "{response}");
    assert_eq!(sending.messages().len(), 1);
}

#[tokio::test]
async fn a_patch_that_does_not_read_is_named_back_after_the_send_rfc8621_7_5() {
    let sending = sending(account(Extension::all()), Script::tls()).await;
    let draft = sending.draft(object(&sending.mailbox(DRAFTS))).await;
    let patch = json!({ "subject": "x" });
    let response = sending
        .submit(&draft, json!({ "onSuccessUpdateEmail": { "#s": patch } }))
        .await;
    let (set, implicit) = answers(&response);
    assert!(set["created"]["s"]["id"].is_string(), "{response}");
    let implicit = implicit.unwrap_or_else(|| panic!("{response}"));
    let refused = &implicit["notUpdated"][&draft];
    assert_eq!(refused["type"], "invalidProperties", "{response}");
    assert_eq!(refused["properties"], json!(["subject"]), "{response}");
    assert_eq!(implicit["updated"], Value::Null, "{response}");
    let state = sending.set.state().to_string();
    assert_eq!(implicit["oldState"], state, "{response}");
    assert_eq!(implicit["newState"], state, "{response}");
    assert_eq!(sending.messages().len(), 1);
    assert_eq!(held(&sending.set, DRAFTS).len(), 1);
    assert_eq!(held(&sending.set, SENT).len(), 0);
}

#[tokio::test]
async fn on_gmail_the_draft_goes_to_trash_and_gmail_keeps_the_copy_rfc8621_7_5() {
    let set = Box::pin(started(
        Mailboxes::gmail(vec![Message::new(1)]),
        ALL_MAIL,
        true,
    ))
    .await;
    let sending = over(set, Script::tls()).await;
    let drafts = sending.mailbox("[Gmail]/Drafts");
    let draft = sending.draft(object(&drafts)).await;
    let filing = json!({
        (format!("mailboxIds/{drafts}")): null,
        (format!("mailboxIds/{}", sending.mailbox("[Gmail]/Sent Mail"))): true,
        "keywords/$draft": null,
    });
    let response = sending
        .submit(&draft, json!({ "onSuccessUpdateEmail": { "#s": filing } }))
        .await;
    let (_, implicit) = answers(&response);
    let implicit = implicit.unwrap_or_else(|| panic!("{response}"));
    assert_eq!(implicit["updated"], json!({ (draft.as_str()): null }));
    assert_eq!(sending.messages().len(), 1);
    assert_eq!(held(&sending.set, TRASH).len(), 1);
    assert_eq!(
        appends(&sending.set).len(),
        1,
        "{:?}",
        appends(&sending.set)
    );
    let written = sending.set.written();
    assert!(
        !written.iter().any(|line| line.contains("EXPUNGE")),
        "{written:?}"
    );
}
