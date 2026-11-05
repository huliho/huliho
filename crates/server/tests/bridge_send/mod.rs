// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A send from an IMAP account through the server's route (RFC 8621
//! section 7): the host signs in on the account's submission server
//! through the pinned resolver and the trust the instance names, the
//! draft goes out and is filed into Sent. A credential that server
//! refuses is the send's alone: the account keeps running.

use huliho_imap_bridge::jmap::{CORE_CAPABILITY, MAIL_CAPABILITY, SUBMISSION_CAPABILITY};
use huliho_imap_bridge::testing::smtp::Script;
use huliho_imap_bridge::testing::{Extension, Folder, Mailboxes, Message, PASSWORD};
use huliho_server::gate::RUN_WINDOW;
use serde_json::{Value, json};

use crate::bridge_rig::{ADDRESS, Instance, mailboxes};

async fn instance(script: Script) -> (Instance, String, String) {
    let folders = vec![
        Folder::new("INBOX").with_mail(vec![Message::new(1)]),
        Folder::special("Drafts", "\\Drafts"),
        Folder::special("Sent", "\\Sent"),
    ];
    let model = Mailboxes::new(folders, Extension::all());
    let instance = Instance::sending(model, RUN_WINDOW, script).await;
    let id = instance.add_account(PASSWORD);
    let cookie = instance.sign_in().await;
    assert!(instance.wait_synced(&cookie, &id).await);
    (instance, id, cookie)
}

/// The id of the account's mailbox with that role.
fn role(listed: &Value, role: &str) -> String {
    let list = listed["list"].as_array().unwrap();
    let found = list.iter().find(|mailbox| mailbox["role"] == role);
    found.unwrap()["id"].as_str().unwrap().to_owned()
}

/// A draft saved and sent in one request, filed into Sent on success,
/// with the identity read beside it; the Response object.
async fn sent(instance: &Instance, id: &str, cookie: &str) -> Value {
    let (_, listed) = instance.call(cookie, id, mailboxes(id)).await;
    let (drafts, sent) = (role(&listed, "drafts"), role(&listed, "sent"));
    let draft = json!({
        "mailboxIds": { (drafts.as_str()): true },
        "keywords": { "$draft": true, "$seen": true },
        "from": [{ "email": ADDRESS }],
        "to": [{ "email": "mo@example.test" }],
        "subject": "Hoi",
        "bodyStructure": { "partId": "t", "type": "text/plain" },
        "bodyValues": { "t": { "value": "Hallo" } },
    });
    let filing = json!({
        (format!("mailboxIds/{drafts}")): null,
        (format!("mailboxIds/{sent}")): true,
        "keywords/$draft": null,
    });
    let body = json!({
        "using": [CORE_CAPABILITY, MAIL_CAPABILITY, SUBMISSION_CAPABILITY],
        "methodCalls": [
            ["Email/set", { "accountId": id, "create": { "d": draft } }, "c0"],
            ["EmailSubmission/set", {
                "accountId": id,
                "create": { "s": { "identityId": id, "emailId": "#d" } },
                "onSuccessUpdateEmail": { "#s": filing },
            }, "c1"],
            ["Identity/get", { "accountId": id, "ids": null }, "c2"],
        ],
    });
    let (_, answer) = instance.request(cookie, id, &body).await;
    answer
}

/// The messages a folder of the scripted server holds.
fn held(instance: &Instance, name: &str) -> usize {
    let folders = instance.fake.script().mailboxes.folders();
    let folder = folders.iter().find(|folder| folder.name == name).unwrap();
    folder.mail.len()
}

#[tokio::test]
async fn a_draft_goes_out_through_the_host_and_lands_in_sent_rfc8621_7_5() {
    let (instance, id, cookie) = instance(Script::tls()).await;
    let (_, session) = instance.session(&cookie, &id).await;
    let capability = &session["accounts"][&id]["accountCapabilities"][SUBMISSION_CAPABILITY];
    assert_eq!(capability["maxDelayedSend"], 0, "{session}");
    let answer = sent(&instance, &id, &cookie).await;
    let responses = &answer["methodResponses"];
    assert_eq!(responses[1][0], "EmailSubmission/set", "{answer}");
    assert_eq!(responses[1][1]["created"]["s"]["undoStatus"], "final");
    let draft = responses[0][1]["created"]["d"]["id"].as_str().unwrap();
    assert_eq!(responses[2][0], "Email/set");
    assert_eq!(responses[2][1]["updated"], json!({ (draft): null }));
    assert_eq!(responses[3][0], "Identity/get");
    assert_eq!(responses[3][1]["list"][0]["email"], ADDRESS);
    assert_eq!(held(&instance, "Drafts"), 0);
    assert_eq!(held(&instance, "Sent"), 1);
}

#[tokio::test]
async fn a_credential_the_submission_server_refuses_stops_nothing_rfc4954_6() {
    let script = Script {
        accepts: false,
        ..Script::tls()
    };
    let (instance, id, cookie) = instance(script).await;
    let before = instance.account_events();
    let answer = sent(&instance, &id, &cookie).await;
    let responses = &answer["methodResponses"];
    let refused = &responses[1][1]["notCreated"]["s"];
    assert_eq!(refused["type"], "forbiddenToSend", "{answer}");
    assert_eq!(held(&instance, "Drafts"), 1);
    assert_eq!(held(&instance, "Sent"), 0);
    assert_eq!(instance.stopped_cause(&id), None);
    assert_eq!(instance.account_events(), before);
}
