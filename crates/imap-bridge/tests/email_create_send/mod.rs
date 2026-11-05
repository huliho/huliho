// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A draft sent through `EmailSubmission/set` on a folder account: the
//! message fetched whole, sent with every Bcc field off the wire and the
//! Date of the send, the envelope from its recipients, the Sent form
//! appended once with its Bcc kept and the draft destroyed once it landed
//! (RFC 8621 section 7, RFC 5321 section 3.3).

use huliho_imap_bridge::store::{ChangeKind, ObjectType};
use huliho_imap_bridge::testing::smtp::Script;
use huliho_imap_bridge::testing::{ADDRESS, Extension};
use mail_parser::MessageParser;
use serde_json::{Value, json};

use crate::draft_rig::{DRAFTS, appends, held, object, without};
use crate::send_rig::{SENT, Sending, account, answers, sending};
use crate::sync_rig::ACCOUNT;

/// The Message-ID field of a raw message.
fn message_id(raw: &str) -> String {
    let line = raw.lines().find(|line| line.starts_with("Message-ID:"));
    line.unwrap_or_else(|| panic!("{raw}")).to_owned()
}

/// The fields of a raw message with that name.
fn fields<'a>(raw: &'a str, name: &str) -> Vec<&'a str> {
    let header = raw.split("\r\n\r\n").next().unwrap_or_default();
    let prefix = format!("{}:", name.to_ascii_lowercase());
    let found = header
        .split("\r\n")
        .filter(|line| line.to_ascii_lowercase().starts_with(&prefix));
    found.collect()
}

async fn sent_draft() -> (Sending, String, Value) {
    let sending = sending(account(Extension::all()), Script::tls()).await;
    let draft = sending.draft(object(&sending.mailbox(DRAFTS))).await;
    let filing = sending.filing();
    let response = sending
        .submit(&draft, json!({ "onSuccessUpdateEmail": { "#s": filing } }))
        .await;
    (sending, draft, response)
}

#[tokio::test]
async fn the_wire_form_leaves_every_bcc_behind_and_carries_the_date_of_the_send_rfc8621_7_5() {
    let sending = sending(account(Extension::all()), Script::tls()).await;
    let draft = sending.draft(object(&sending.mailbox(DRAFTS))).await;
    let saved = held(&sending.set, DRAFTS)[0].raw();
    let response = sending.submit(&draft, json!({})).await;
    let (set, implicit) = answers(&response);
    assert!(implicit.is_none(), "{response}");
    assert!(set["created"]["s"]["id"].as_str().unwrap().starts_with('s'));
    let messages = sending.messages();
    assert_eq!(messages.len(), 1, "{messages:?}");
    let wire = &messages[0];
    assert!(fields(wire, "Bcc").is_empty(), "{wire}");
    assert!(!wire.contains("hidden@example.test"), "{wire}");
    assert_eq!(fields(wire, "Date").len(), 1, "{wire}");
    let parsed = MessageParser::default().parse(wire.as_bytes()).unwrap();
    let dated = parsed.date().map(mail_parser::DateTime::to_rfc3339);
    assert_eq!(dated.as_deref(), set["created"]["s"]["sendAt"].as_str());
    assert_eq!(message_id(wire), message_id(&saved));
    // DATA ends the message with its own line break, which the recording
    // of the lines keeps apart from the message's.
    let body = |raw: &str| {
        raw.split_once("\r\n\r\n")
            .map(|(_, body)| body.trim_end().to_owned())
    };
    assert_eq!(body(wire), body(&saved));
}

#[tokio::test]
async fn the_envelope_holds_the_identity_and_every_recipient_once_in_order_rfc8621_7() {
    let (sending, _, response) = sent_draft().await;
    assert_eq!(
        sending.transaction(),
        [
            format!("MAIL FROM:<{ADDRESS}>"),
            "RCPT TO:<mo@example.test>".to_owned(),
            "RCPT TO:<li@example.test>".to_owned(),
            "RCPT TO:<hidden@example.test>".to_owned(),
            "DATA".to_owned(),
        ]
    );
    let (set, _) = answers(&response);
    let made = &set["created"]["s"];
    assert_eq!(made["undoStatus"], "final");
    assert_eq!(made["deliveryStatus"], Value::Null);
    assert_eq!(made["envelope"]["mailFrom"]["email"], ADDRESS);
    let recipients: Vec<&str> = made["envelope"]["rcptTo"]
        .as_array()
        .unwrap()
        .iter()
        .map(|address| address["email"].as_str().unwrap())
        .collect();
    assert_eq!(
        recipients,
        ["mo@example.test", "li@example.test", "hidden@example.test"]
    );
    assert!(made["sendAt"].as_str().unwrap().ends_with('Z'));
}

#[tokio::test]
async fn a_given_envelope_goes_and_reads_back_as_the_message_and_the_identity_spell_it_rfc8621_7() {
    let sending = sending(account(Extension::all()), Script::tls()).await;
    let draft = sending.draft(object(&sending.mailbox(DRAFTS))).await;
    let address = |email: &str| json!({ "email": email, "parameters": null });
    let envelope = json!({ "create": { "s": { "envelope": {
        "mailFrom": address("SANNE@example.test"),
        "rcptTo": [address("LI@example.test"), address("Mo@Example.Test")],
    } } } });
    let response = sending.submit(&draft, envelope).await;
    let (set, _) = answers(&response);
    let id = set["created"]["s"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("{response}"));
    assert_eq!(
        sending.transaction(),
        [
            format!("MAIL FROM:<{ADDRESS}>"),
            "RCPT TO:<li@example.test>".to_owned(),
            "RCPT TO:<mo@example.test>".to_owned(),
            "DATA".to_owned(),
        ]
    );
    let get = json!(["EmailSubmission/get", { "accountId": ACCOUNT, "ids": [id] }, "g"]);
    let read = sending.calls(&[get]).await;
    let recorded = &read["methodResponses"][0][1]["list"][0]["envelope"];
    assert_eq!(recorded["mailFrom"]["email"], ADDRESS, "{read}");
    let recipients: Vec<&str> = recorded["rcptTo"]
        .as_array()
        .unwrap_or_else(|| panic!("{read}"))
        .iter()
        .map(|address| address["email"].as_str().unwrap())
        .collect();
    assert_eq!(recipients, ["li@example.test", "mo@example.test"]);
}

#[tokio::test]
async fn a_filed_draft_lands_in_sent_once_with_its_bcc_and_leaves_drafts_rfc8621_7_5() {
    let (sending, draft, response) = sent_draft().await;
    let (submitted, implicit) = answers(&response);
    assert_eq!(submitted["notCreated"], Value::Null, "{response}");
    let implicit = implicit.unwrap_or_else(|| panic!("{response}"));
    assert_eq!(implicit["updated"], json!({ (draft.as_str()): null }));
    assert_eq!(implicit["notUpdated"], Value::Null);
    // The copy goes in on its own, then the draft leaves; no replace
    // spans the two folders.
    let written = appends(&sending.set);
    assert_eq!(written.len(), 2, "{written:?}");
    assert!(
        written[1].starts_with("APPEND \"Sent\" (\\Seen) {"),
        "{written:?}"
    );
    assert_eq!(held(&sending.set, DRAFTS).len(), 0);
    let sent = held(&sending.set, SENT);
    assert_eq!(sent.len(), 1);
    assert!(sent[0].raw().contains("hidden@example.test"));
    assert_eq!(sent[0].flags, ["\\Seen"]);
    let changes = sending.set.rig.changes(ObjectType::Email, 0);
    assert!(
        changes.contains(&(draft.clone(), ChangeKind::Destroyed)),
        "{changes:?}"
    );
    let landed: Vec<&String> = changes
        .iter()
        .filter(|(id, kind)| *kind == ChangeKind::Created && *id != draft)
        .map(|(id, _)| id)
        .collect();
    let copy = landed.last().unwrap();
    let arguments = json!({ "accountId": ACCOUNT, "ids": [copy], "properties": ["mailboxIds", "keywords", "bcc"] });
    let got = sending
        .set
        .rig
        .call(&json!(["Email/get", arguments, "g"]))
        .await;
    let shown = &got["list"][0];
    assert_eq!(
        shown["mailboxIds"],
        json!({ (sending.mailbox(SENT)): true })
    );
    assert_eq!(shown["keywords"], json!({ "$seen": true }));
    assert_eq!(shown["bcc"][0]["email"], "hidden@example.test");
}

#[tokio::test]
async fn without_uidplus_the_copy_lands_from_one_pass_and_the_draft_stays_rfc4315_3() {
    let extensions = without(&[Extension::Uidplus, Extension::Replace]);
    let sending = sending(account(extensions), Script::tls()).await;
    let draft = sending.draft(object(&sending.mailbox(DRAFTS))).await;
    let before = sending.set.state();
    let filing = sending.filing();
    let response = sending
        .submit(&draft, json!({ "onSuccessUpdateEmail": { "#s": filing } }))
        .await;
    let (set, implicit) = answers(&response);
    assert!(set["created"]["s"]["id"].is_string(), "{response}");
    let implicit = implicit.unwrap_or_else(|| panic!("{response}"));
    assert_eq!(implicit["updated"], json!({ (draft.as_str()): null }));
    assert_eq!(implicit["notUpdated"], Value::Null, "{response}");
    let copies = held(&sending.set, SENT);
    assert_eq!(copies.len(), 1);
    assert!(copies[0].raw().contains("hidden@example.test"));
    // The server offers no UID EXPUNGE, so the draft stays with its row.
    assert_eq!(held(&sending.set, DRAFTS).len(), 1);
    let written = sending.set.written();
    assert!(
        !written.iter().any(|line| line.contains("EXPUNGE")),
        "{written:?}"
    );
    let landed = sending.set.rig.created(before);
    assert_eq!(landed.len(), 1, "{landed:?}");
    let copy = landed[0].as_str();
    assert_ne!(copy, draft);
    let arguments = json!({
        "accountId": ACCOUNT,
        "ids": [copy, &draft],
        "properties": ["mailboxIds", "keywords", "bcc"],
    });
    let got = sending
        .set
        .rig
        .call(&json!(["Email/get", arguments, "g"]))
        .await;
    let list = got["list"].as_array().unwrap_or_else(|| panic!("{got}"));
    let shown = |id: &str| {
        list.iter()
            .find(|email| email["id"] == id)
            .unwrap_or_else(|| panic!("{got}"))
    };
    assert_eq!(
        shown(copy)["mailboxIds"],
        json!({ (sending.mailbox(SENT)): true })
    );
    assert_eq!(shown(copy)["keywords"], json!({ "$seen": true }));
    assert_eq!(shown(copy)["bcc"][0]["email"], "hidden@example.test");
    assert_eq!(
        shown(&draft)["mailboxIds"],
        json!({ (sending.mailbox(DRAFTS)): true })
    );
    assert_eq!(got["notFound"], json!([]));
}

#[tokio::test]
async fn a_draft_saved_in_the_same_request_is_sent_by_its_creation_id_rfc8620_5_3() {
    let sending = sending(account(Extension::all()), Script::tls()).await;
    let save = json!(["Email/set", {
        "accountId": ACCOUNT,
        "create": { "d": object(&sending.mailbox(DRAFTS)) },
    }, "c0"]);
    let send = json!(["EmailSubmission/set", {
        "accountId": ACCOUNT,
        "create": { "s": { "identityId": ACCOUNT, "emailId": "#d" } },
        "onSuccessDestroyEmail": ["#s"],
    }, "c1"]);
    let response = sending.calls(&[save, send]).await;
    let responses = response["methodResponses"].as_array().unwrap();
    assert_eq!(responses.len(), 3, "{response}");
    let draft = responses[0][1]["created"]["d"]["id"].as_str().unwrap();
    assert_eq!(
        responses[1][1]["created"]["s"]["undoStatus"], "final",
        "{response}"
    );
    assert_eq!(responses[2][0], "Email/set");
    assert_eq!(responses[2][1]["destroyed"], json!([draft]));
    assert_eq!(sending.messages().len(), 1);
    assert_eq!(held(&sending.set, DRAFTS).len(), 0);
    assert_eq!(held(&sending.set, SENT).len(), 0);
}

#[tokio::test]
async fn a_send_goes_from_the_state_it_names_and_another_state_stops_it_rfc8620_5_3() {
    let sending = sending(account(Extension::all()), Script::tls()).await;
    let draft = sending.draft(object(&sending.mailbox(DRAFTS))).await;
    let stale = sending
        .submit(&draft, json!({ "ifInState": "s-other" }))
        .await;
    let refused = &stale["methodResponses"][0];
    assert_eq!(
        (&refused[0], &refused[1]["type"]),
        (&json!("error"), &json!("stateMismatch")),
        "{stale}"
    );
    let reading = json!(["EmailSubmission/get", { "accountId": ACCOUNT, "ids": [] }, "g"]);
    let read = sending.calls(&[reading]).await;
    let current = read["methodResponses"][0][1]["state"].clone();
    let response = sending
        .submit(&draft, json!({ "ifInState": current }))
        .await;
    let (set, _) = answers(&response);
    assert_eq!(set["oldState"], current, "{response}");
    assert!(set["created"]["s"]["id"].is_string(), "{response}");
    assert_ne!(set["newState"], current);
}

#[tokio::test]
async fn the_record_answers_get_and_every_submission_is_final_rfc8621_7() {
    let (sending, draft, response) = sent_draft().await;
    let (set, _) = answers(&response);
    let id = set["created"]["s"]["id"].as_str().unwrap().to_owned();
    let get = json!(["EmailSubmission/get", { "accountId": ACCOUNT, "ids": [id, "s-other"] }, "g"]);
    let changes = json!(["EmailSubmission/changes", { "accountId": ACCOUNT, "sinceState": set["newState"] }, "c"]);
    let elsewhere = json!(["EmailSubmission/changes", { "accountId": ACCOUNT, "sinceState": set["oldState"] }, "e"]);
    let undo = json!(["EmailSubmission/set", {
        "accountId": ACCOUNT,
        "update": { (id.as_str()): { "undoStatus": "canceled" }, "s-other": {} },
        "destroy": [id],
    }, "u"]);
    let query = json!(["EmailSubmission/query", { "accountId": ACCOUNT }, "q"]);
    let answered = sending.calls(&[get, changes, elsewhere, undo, query]).await;
    let responses = &answered["methodResponses"];
    let found = &responses[0][1];
    assert_eq!(found["list"][0]["emailId"], draft.as_str());
    assert_eq!(found["list"][0]["identityId"], ACCOUNT);
    assert_eq!(found["notFound"], json!(["s-other"]));
    assert_eq!(responses[1][1]["created"], json!([]));
    assert_eq!(responses[2][1]["type"], "cannotCalculateChanges");
    let refused = &responses[3][1];
    assert_eq!(refused["notUpdated"][&id]["type"], "cannotUnsend");
    assert_eq!(refused["notUpdated"]["s-other"]["type"], "notFound");
    assert_eq!(refused["notDestroyed"][&id]["type"], "forbidden");
    assert_eq!(responses[4][1]["type"], "unknownMethod");
}

#[tokio::test]
async fn one_identity_answers_with_the_accounts_address_and_no_call_changes_it_rfc8621_6() {
    let sending = sending(account(Extension::all()), Script::tls()).await;
    let get = json!(["Identity/get", { "accountId": ACCOUNT, "ids": null }, "g"]);
    let named = json!(["Identity/get", { "accountId": ACCOUNT, "ids": [ACCOUNT, "i2"], "properties": ["email"] }, "n"]);
    let set = json!(["Identity/set", {
        "accountId": ACCOUNT,
        "create": { "n": { "email": "other@example.test" } },
        "update": { (ACCOUNT): { "name": "Sanne" } },
        "destroy": [ACCOUNT],
    }, "s"]);
    let changes = json!(["Identity/changes", { "accountId": ACCOUNT, "sinceState": "0" }, "c"]);
    let answered = sending.calls(&[get, named, set, changes]).await;
    let responses = &answered["methodResponses"];
    assert_eq!(
        responses[0][1]["list"],
        json!([{
            "id": ACCOUNT,
            "name": "",
            "email": ADDRESS,
            "replyTo": null,
            "bcc": null,
            "textSignature": "",
            "htmlSignature": "",
            "mayDelete": false,
        }])
    );
    assert_eq!(
        responses[1][1]["list"],
        json!([{ "id": ACCOUNT, "email": ADDRESS }])
    );
    assert_eq!(responses[1][1]["notFound"], json!(["i2"]));
    let refused = &responses[2][1];
    for list in ["notCreated", "notUpdated", "notDestroyed"] {
        let first = refused[list].as_object().unwrap().values().next().unwrap();
        assert_eq!(first["type"], "forbidden", "{list}");
    }
    assert_eq!(responses[3][1]["updated"], json!([]));
    assert_eq!(sending.smtp.lines(), Vec::<String>::new());
}

#[tokio::test]
async fn without_the_submission_capability_neither_method_runs_rfc8620_3_6_2() {
    let sending = sending(account(Extension::all()), Script::tls()).await;
    let call = json!(["Identity/get", { "accountId": ACCOUNT, "ids": null }, "g"]);
    let answered = sending.set.rig.call(&call).await;
    assert_eq!(answered["type"], "unknownMethod");
}
