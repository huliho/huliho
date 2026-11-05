// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The creation ids of a request that sends: the submission's own comes
//! back in `createdIds`, the Sent copy's stays inside the implicit call
//! and an id the client named keeps naming its draft (RFC 8620 sections
//! 3.3 and 5.3).

use huliho_imap_bridge::jmap::{CORE_CAPABILITY, MAIL_CAPABILITY, SUBMISSION_CAPABILITY, handle};
use huliho_imap_bridge::testing::Extension;
use huliho_imap_bridge::testing::smtp::Script;
use serde_json::{Value, json};

use crate::draft_rig::{DRAFTS, held, object};
use crate::send_rig::{SENT, Sending, account, answers, sending};
use crate::sync_rig::ACCOUNT;

/// The Response object to these calls under the submission capability,
/// the request carrying empty `createdIds` so they come back.
async fn request(sending: &Sending, calls: &[Value]) -> Value {
    let body = json!({
        "using": [CORE_CAPABILITY, MAIL_CAPABILITY, SUBMISSION_CAPABILITY],
        "methodCalls": calls,
        "createdIds": {},
    });
    let bytes = serde_json::to_vec(&body).unwrap();
    let answer = handle(&sending.set.rig.cache, &sending.link, &bytes, ACCOUNT)
        .await
        .unwrap();
    serde_json::from_slice(&answer).unwrap()
}

#[tokio::test]
async fn a_filed_send_answers_created_ids_with_the_submissions_creation_id_alone_rfc8620_3_4() {
    let sending = sending(account(Extension::all()), Script::tls()).await;
    let draft = sending.draft(object(&sending.mailbox(DRAFTS))).await;
    let send = json!(["EmailSubmission/set", {
        "accountId": ACCOUNT,
        "create": { "s": { "identityId": ACCOUNT, "emailId": draft } },
        "onSuccessUpdateEmail": { "#s": sending.filing() },
    }, "c1"]);
    let response = request(&sending, &[send]).await;
    let (set, implicit) = answers(&response);
    let id = set["created"]["s"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("{response}"));
    let implicit = implicit.unwrap_or_else(|| panic!("{response}"));
    assert_eq!(
        implicit["updated"],
        json!({ (draft.as_str()): null }),
        "{response}"
    );
    assert_eq!(held(&sending.set, SENT).len(), 1);
    assert_eq!(response["createdIds"], json!({ "s": id }), "{response}");
}

#[tokio::test]
async fn a_creation_id_named_sent_keeps_naming_the_draft_past_the_filing_rfc8620_5_3() {
    let sending = sending(account(Extension::all()), Script::tls()).await;
    let save = json!(["Email/set", {
        "accountId": ACCOUNT,
        "create": { "sent": object(&sending.mailbox(DRAFTS)) },
    }, "c0"]);
    let send = json!(["EmailSubmission/set", {
        "accountId": ACCOUNT,
        "create": { "s": { "identityId": ACCOUNT, "emailId": "#sent" } },
        "onSuccessUpdateEmail": { "#s": sending.filing() },
    }, "c1"]);
    let again = json!(["EmailSubmission/set", {
        "accountId": ACCOUNT,
        "create": { "t": { "identityId": ACCOUNT, "emailId": "#sent" } },
    }, "c2"]);
    let response = request(&sending, &[save, send, again]).await;
    let responses = response["methodResponses"].as_array().unwrap();
    assert_eq!(responses.len(), 4, "{response}");
    let draft = responses[0][1]["created"]["sent"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("{response}"));
    assert_eq!(
        responses[1][1]["created"]["s"]["undoStatus"], "final",
        "{response}"
    );
    assert_eq!(responses[2][1]["updated"], json!({ (draft): null }));
    assert_eq!(held(&sending.set, SENT).len(), 1);
    let ids = response["createdIds"]
        .as_object()
        .unwrap_or_else(|| panic!("{response}"));
    assert_eq!(ids["sent"], draft, "{response}");
    assert_eq!(ids.len(), 2, "{response}");
    // The draft left with the filing, so the later send under its
    // creation id finds no email and nothing more goes out.
    let later = &responses[3][1]["notCreated"]["t"];
    assert_eq!(later["type"], "invalidProperties", "{response}");
    assert_eq!(later["properties"], json!(["emailId"]), "{response}");
    assert_eq!(sending.messages().len(), 1);
}
