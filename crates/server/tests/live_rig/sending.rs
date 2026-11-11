// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The round trip through the compose Postfix (RFC 8621 section 7): the
//! Cyrus account sends to a second Dovecot user over JMAP and Postfix
//! delivers it to Dovecot over LMTP; the reply goes out through the
//! bridge and Dovecot's submission and Postfix delivers it to Cyrus over
//! LMTP, where it joins the thread. Each message lands in the other
//! account and leaves one copy in the sender's Sent mailbox. The Cyrus
//! account over IMAPS sends the same way through Dovecot's submission.

use axum::http::StatusCode;
use huliho_imap_bridge::jmap::{CORE_CAPABILITY, MAIL_CAPABILITY, SUBMISSION_CAPABILITY};
use serde_json::{Value, json};
use tokio::time::sleep;

use super::drafts::{Folder, PATIENCE, stamp, swept};
use super::second::INBOX;
use super::{
    Editor, LISTING, LOOK, LOOKS, Live, MAIL_ADDRESS, Mail, Target, Window, editor_as,
    expunge_subject, listed, within,
};

/// The second Dovecot user, which this test alone signs in as; the
/// image takes any user.
const ANNA: &str = "anna@huliho.test";

/// The word every subject of the trip starts with, by which a run finds
/// what an earlier one left.
const WORD: &str = "Rondgang";

/// One end of the trip: the account, its mailboxes and the second
/// connection that clears what the trip left.
struct Side {
    mail: Mail,
    address: &'static str,
    inbox: String,
    drafts: Folder,
    sent: Folder,
    editor: Editor,
}

impl Side {
    async fn open(live: &Live, target: Target, address: &'static str, stamp: &str) -> Self {
        let mail = live.add(&target.body_of(address), target.is_bridge()).await;
        let listed = live.wait_listed(&mail, PATIENCE).await;
        let inbox = listed.iter().find(|mailbox| mailbox["role"] == "inbox");
        let inbox = inbox.unwrap_or_else(|| panic!("{listed:?}"))["id"].clone();
        swept(live, &mail).await;
        let drafts = Folder::of(live, &mail, "drafts", stamp).await;
        let sent = Folder::of(live, &mail, "sent", stamp).await;
        let mut editor = editor_as(target, address).await;
        for wire in [INBOX, &drafts.wire, &sent.wire] {
            expunge_subject(&mut editor, wire, WORD).await;
        }
        Self {
            mail,
            address,
            inbox: inbox.as_str().unwrap().to_owned(),
            drafts,
            sent,
            editor,
        }
    }

    /// The side as the trip found it: its messages and folders gone, the
    /// second connection out, the account removed.
    async fn leave(mut self, live: &Live) {
        for wire in [INBOX, &self.drafts.wire, &self.sent.wire] {
            expunge_subject(&mut self.editor, wire, WORD).await;
        }
        self.drafts.leave(live, &self.mail).await;
        self.sent.leave(live, &self.mail).await;
        within(self.editor.logout()).await.unwrap();
        live.remove(&self.mail).await;
    }
}

/// From the Cyrus account to Dovecot and the reply back.
pub async fn round_trip() {
    let live = Live::start().await;
    let stamp = stamp();
    let cyrus = Side::open(&live, Target::Cyrus, MAIL_ADDRESS, &stamp).await;
    let dovecot = Side::open(&live, Target::Dovecot, ANNA, &stamp).await;
    let subject = format!("{WORD} {stamp}");
    let (_, arrived) = leg(&live, (&cyrus, &dovecot), &subject, None).await;
    let reply = format!("Re: {subject}");
    let (copy, back) = leg(&live, (&dovecot, &cyrus), &reply, Some(&arrived)).await;
    assert_eq!(back["inReplyTo"], arrived["messageId"], "{back}");
    let mine = filed(&live, &cyrus, &cyrus.sent.id, &subject).await;
    assert_eq!(back["threadId"], mine[0]["threadId"], "{back}");
    assert_eq!(copy["messageId"], back["messageId"], "{copy}");
    cyrus.leave(&live).await;
    dovecot.leave(&live).await;
}

/// From the Cyrus account over IMAPS to Dovecot.
pub async fn over_imaps() {
    let live = Live::start().await;
    let stamp = stamp();
    let cyrus = Side::open(&live, Target::CyrusBridge, MAIL_ADDRESS, &stamp).await;
    let dovecot = Side::open(&live, Target::Dovecot, ANNA, &stamp).await;
    let subject = format!("{WORD} {stamp}");
    let (copy, arrived) = leg(&live, (&cyrus, &dovecot), &subject, None).await;
    assert_eq!(copy["messageId"], arrived["messageId"], "{copy}");
    cyrus.leave(&live).await;
    dovecot.leave(&live).await;
}

/// One message from one side to the other: the sender's one Sent copy
/// and the email that landed.
async fn leg(
    live: &Live,
    (from, to): (&Side, &Side),
    subject: &str,
    parent: Option<&Value>,
) -> (Value, Value) {
    sent(live, (from, to), subject, parent).await;
    let arrived = landed(live, to, subject).await;
    let copies = filed(live, from, &from.sent.id, subject).await;
    assert_eq!(copies.len(), 1, "one Sent copy: {copies:?}");
    let left = filed(live, from, &from.drafts.id, subject).await;
    assert!(left.is_empty(), "the draft left Drafts: {left:?}");
    (copies[0].clone(), arrived)
}

/// The method responses of one request that names the submission
/// capability.
async fn requested(live: &Live, mail: &Mail, calls: Value) -> Value {
    let using = [CORE_CAPABILITY, MAIL_CAPABILITY, SUBMISSION_CAPABILITY];
    let body = json!({ "using": using, "methodCalls": calls });
    let (status, answered) = live.post(&format!("/api/jmap/{}", mail.id), &body).await;
    assert_eq!(status, StatusCode::OK, "{answered}");
    answered["methodResponses"].clone()
}

/// The draft saved and sent in one request under the account's one
/// identity, filed into Sent on success; the three answers checked.
async fn sent(live: &Live, (from, to): (&Side, &Side), subject: &str, parent: Option<&Value>) {
    let mail = &from.mail;
    let ask = json!([["Identity/get", { "accountId": mail.account }, "i"]]);
    let identities = requested(live, mail, ask).await;
    let identity = &identities[0][1]["list"][0];
    assert_eq!(identity["email"], from.address, "{identities}");
    let mut draft = json!({
        "mailboxIds": { (from.drafts.id.as_str()): true },
        "keywords": { "$draft": true, "$seen": true },
        "from": [{ "email": from.address }],
        "to": [{ "email": to.address }],
        "subject": subject,
        "bodyStructure": { "type": "text/plain", "partId": "text" },
        "bodyValues": { "text": { "value": subject } },
    });
    if let Some(parent) = parent {
        draft["inReplyTo"] = parent["messageId"].clone();
        draft["references"] = parent["messageId"].clone();
    }
    let filing = json!({
        (format!("mailboxIds/{}", from.drafts.id)): null,
        (format!("mailboxIds/{}", from.sent.id)): true,
        "keywords/$draft": null,
    });
    let submission = json!({ "identityId": identity["id"], "emailId": "#d" });
    let calls = json!([
        ["Email/set", { "accountId": mail.account, "create": { "d": draft } }, "c1"],
        ["EmailSubmission/set", {
            "accountId": mail.account,
            "create": { "s": submission },
            "onSuccessUpdateEmail": { "#s": filing },
        }, "c2"],
    ]);
    let responses = requested(live, mail, calls).await;
    let draft = responses[0][1]["created"]["d"]["id"].clone();
    assert!(draft.is_string(), "{responses}");
    assert_eq!(responses[1][0], "EmailSubmission/set", "{responses}");
    let made = &responses[1][1]["created"]["s"];
    assert_eq!(made["undoStatus"], "final", "{responses}");
    assert_eq!(responses[2][0], "Email/set", "{responses}");
    let implicit = &responses[2][1]["updated"];
    assert!(
        implicit.get(draft.as_str().unwrap()).is_some(),
        "{responses}"
    );
}

/// The email with that subject once the side's inbox shows it. Each
/// look asks for the changes first, as a client polls, which is what
/// refreshes a bridge account.
async fn landed(live: &Live, side: &Side, subject: &str) -> Value {
    let since = live.state(&side.mail).await;
    let changes = json!({ "accountId": side.mail.account, "sinceState": since });
    for _ in 0..LOOKS {
        live.call(&side.mail, "Email/changes", changes.clone())
            .await;
        if let Some(email) = filed(live, side, &side.inbox, subject).await.pop() {
            return email;
        }
        sleep(LOOK).await;
    }
    panic!("nothing with the subject reached the inbox");
}

/// The emails of the mailbox with that subject.
async fn filed(live: &Live, side: &Side, mailbox: &str, subject: &str) -> Vec<Value> {
    let window = Window {
        mailbox,
        position: 0,
        limit: LISTING,
        collapse: false,
        total: false,
    };
    let ids = listed(&live.query(&side.mail, window).await, "ids");
    let properties = ["subject", "messageId", "inReplyTo", "threadId"];
    let mut emails = live.emails(&side.mail, &ids, &properties).await;
    emails.retain(|email| email["subject"] == subject);
    emails
}
