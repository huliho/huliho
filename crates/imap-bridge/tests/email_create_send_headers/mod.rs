// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A draft the recipient reader and the header rewrite read differently:
//! a date the reader runs past the empty line, so a Bcc line of the body
//! reads as a field or the fields under the date read as body text. The
//! send is refused before anything connects and the draft stays (RFC
//! 8621 section 7.5).

use huliho_imap_bridge::testing::messages::plain_leaf;
use huliho_imap_bridge::testing::smtp::Script;
use huliho_imap_bridge::testing::{Extension, Message};
use serde_json::{Value, json};

use crate::draft_rig::{DRAFTS, held};
use crate::send_rig::{SENT, Sending, account, answers, sending};
use crate::sync_rig::ACCOUNT;

/// The account beside the SMTP server with a draft of these bytes in
/// Drafts and the id that draft got.
async fn seeded(header: &str, body: &str) -> (Sending, String) {
    let mailboxes = account(Extension::all());
    mailboxes.append(
        DRAFTS,
        Message {
            header: header.to_owned(),
            structure: plain_leaf(body.len()),
            body: body.to_owned(),
            ..Message::new(7).flagged(&["\\Draft", "\\Seen"])
        },
    );
    let sending = sending(mailboxes, Script::tls()).await;
    let query = json!(["Email/query", {
        "accountId": ACCOUNT,
        "filter": { "inMailbox": sending.mailbox(DRAFTS) },
    }, "q"]);
    let listed = sending.set.rig.call(&query).await;
    let draft = listed["ids"][0].as_str().unwrap().to_owned();
    (sending, draft)
}

/// The send was refused for the header fields, nothing reached the SMTP
/// server and nothing moved.
fn refused_unconnected(sending: &Sending, response: &Value) {
    let (set, _) = answers(response);
    assert_eq!(set["notCreated"]["s"]["type"], "invalidEmail", "{response}");
    assert_eq!(
        set["notCreated"]["s"]["properties"],
        json!(["headers"]),
        "{response}"
    );
    assert_eq!(sending.smtp.lines(), Vec::<String>::new());
    assert_eq!(held(&sending.set, DRAFTS).len(), 1);
    assert_eq!(held(&sending.set, SENT).len(), 0);
}

#[tokio::test]
async fn a_bcc_line_of_the_body_the_reader_takes_as_a_field_connects_nothing_rfc8621_7_5() {
    // A date cut at a zone name runs the recipient reader past the empty
    // line, so the Bcc line of the body reads as a field.
    let header = "From: Sanne <sanne@example.test>\r\nTo: mo@example.test\r\nSubject: x\r\n\
Message-ID: <m7@example.test>\r\nResent-Date: +e\r\n\r\n";
    let (sending, draft) = seeded(header, "Bcc: kim@example.test\r\n").await;
    let response = sending.submit(&draft, json!({})).await;
    refused_unconnected(&sending, &response);
}

#[tokio::test]
async fn the_fields_under_a_date_the_reader_runs_past_connect_nothing_rfc8621_7_5() {
    // The reader takes the From, To and Bcc lines as fields of the
    // draft; the rewrite keeps them as body text under the Date stamp.
    let header = "Date: Mon, 2 Nov 2026 10:00:00 +0000\r\n +e\n\n\n";
    let body = "From: Sanne <sanne@example.test>\r\nTo: mo@example.test\r\nSubject: x\r\n\
Message-ID: <m7@example.test>\r\nBcc: secret@example.test\r\n\r\nBody of message 7.";
    let (sending, draft) = seeded(header, body).await;
    let response = sending.submit(&draft, json!({})).await;
    refused_unconnected(&sending, &response);
}
