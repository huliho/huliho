// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A send whose request goes away once the submission server took the
//! message: the send still ends with QUIT, the submission is recorded,
//! the Sent copy lands and the draft leaves (RFC 8621 section 7.5).

use std::time::Duration;

use huliho_imap_bridge::store::{ChangeKind, ObjectType};
use huliho_imap_bridge::testing::Extension;
use huliho_imap_bridge::testing::smtp::Script;
use serde_json::{Value, json};
use tokio::time::timeout;

use crate::draft_rig::{DRAFTS, email, held, object};
use crate::send_rig::{SENT, Sending, account, sending};
use crate::sync_rig::ACCOUNT;

/// Room on loopback for the rest of a send once its request is gone:
/// the reply to DATA, QUIT, the record and the two folder writes.
const SETTLING: Duration = Duration::from_secs(10);

/// Whether the send ended on every side: QUIT reached the server, the
/// copy landed, the draft left and the rows log its destroy.
fn settled(sending: &Sending, draft: &str) -> bool {
    let quit = sending
        .smtp
        .lines()
        .last()
        .is_some_and(|line| line == "tls QUIT");
    let logged = sending.set.rig.changes(ObjectType::Email, 0);
    quit && held(&sending.set, DRAFTS).is_empty()
        && held(&sending.set, SENT).len() == 1
        && logged.contains(&(draft.to_owned(), ChangeKind::Destroyed))
}

#[tokio::test]
async fn a_request_gone_after_the_250_still_quits_records_and_files_rfc8621_7_5() {
    let sending = sending(account(Extension::all()), Script::tls()).await;
    let draft = sending.draft(object(&sending.mailbox(DRAFTS))).await;
    let before = sending.set.state();
    let call = json!(["EmailSubmission/set", {
        "accountId": ACCOUNT,
        "create": { "s": { "identityId": ACCOUNT, "emailId": draft } },
        "onSuccessUpdateEmail": { "#s": sending.filing() },
    }, "c1"]);
    let mut request = Box::pin(sending.calls(std::slice::from_ref(&call)));
    // Nothing else polls the request, so once the server recorded the
    // message it is dropped with the reply to DATA unread.
    let answered = timeout(SETTLING, async {
        while sending.messages().is_empty() {
            tokio::select! {
                biased;
                () = tokio::task::yield_now() => {}
                answer = &mut request => return Some(answer),
            }
        }
        None
    })
    .await
    .unwrap();
    assert!(answered.is_none(), "{answered:?}");
    drop(request);
    let _settled = timeout(SETTLING, async {
        while !settled(&sending, &draft) {
            tokio::task::yield_now().await;
        }
    })
    .await;
    let lines = sending.smtp.lines();
    assert_eq!(
        lines.last().map(String::as_str),
        Some("tls QUIT"),
        "{lines:?}"
    );
    let transaction = sending.transaction();
    let count = |verb: &str| {
        let found = transaction.iter().filter(|line| line.starts_with(verb));
        found.count()
    };
    let verbs = (count("MAIL"), count("DATA"), count("RSET"));
    assert_eq!(verbs, (1, 1, 0), "{transaction:?}");
    assert_eq!(sending.messages().len(), 1);
    assert_eq!(held(&sending.set, DRAFTS).len(), 0);
    assert_eq!(email(&sending.set, &draft, &["id"]).await, Value::Null);
    let sent = held(&sending.set, SENT);
    assert_eq!(sent.len(), 1);
    assert!(sent[0].raw().contains("hidden@example.test"));
    let landed = sending.set.rig.created(before);
    assert_eq!(landed.len(), 1, "{landed:?}");
    assert_ne!(landed[0], draft);
    let get = json!(["EmailSubmission/get", { "accountId": ACCOUNT, "ids": null }, "g"]);
    let read = sending.calls(&[get]).await;
    let list = &read["methodResponses"][0][1]["list"];
    assert_eq!(list.as_array().map(Vec::len), Some(1), "{read}");
    assert_eq!(list[0]["emailId"], draft.as_str(), "{read}");
}
