// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! `Mailbox/set` on a Gmail account: a label is created, renamed and
//! deleted through CREATE, RENAME and DELETE, its mail stays in All
//! Mail, what Gmail keeps for itself refuses every change and a
//! conversation old enough is logged out and signs in afresh before a
//! pass, since Gmail fixes a connection's folders when it signs in.

use std::time::Duration;

use huliho_imap_bridge::jmap::{CORE_CAPABILITY, MAIL_CAPABILITY, handle};
use huliho_imap_bridge::runtime::{Link, Timing};
use huliho_imap_bridge::session::TlsMode;
use huliho_imap_bridge::testing::imap::HOST;
use huliho_imap_bridge::testing::mailboxes::ALL_MAIL;
use huliho_imap_bridge::testing::{Mailboxes, Message, Storing, TestConnector};
use serde_json::json;
use tokio::time::timeout;

use crate::folders_rig::{id_of, mailbox, refused, sent, set, started};
use crate::sync_rig::{ACCOUNT, Rig};

/// Room for a loopback exchange.
const STEP: Duration = Duration::from_secs(2);

/// A conversation a test treats as fresh for good.
const AGES: Duration = Duration::from_secs(3600);

/// A Gmail account with one message in All Mail under the label `Work`.
fn gmail() -> Mailboxes {
    let mail = vec![Message::new(1).labeled(&["Work"])];
    Mailboxes::gmail(mail).with_label("Work")
}

/// The mailboxes the one email of the account is a member of.
async fn memberships(rig: &Rig) -> Vec<String> {
    let email = rig.created(0).remove(0);
    let arguments = json!({ "accountId": ACCOUNT, "ids": [email], "properties": ["mailboxIds"] });
    let got = rig.call(&json!(["Email/get", arguments, "c1"])).await;
    let ids = got["list"][0]["mailboxIds"].as_object().unwrap();
    ids.keys().cloned().collect()
}

/// A label made, renamed and deleted keeps its mail where Gmail keeps
/// it: the message stays a member under the new name and stays in All
/// Mail when the label goes.
#[tokio::test]
async fn a_label_is_created_renamed_and_deleted_and_its_mail_stays_in_all_mail() {
    let rig = started(gmail(), true).await;
    rig.sync(ALL_MAIL).await;
    let answer = set(
        &rig,
        json!({ "create": { "k": { "name": "Plan \u{fc}" } } }),
    )
    .await;
    let plan = answer["created"]["k"]["id"].as_str().unwrap().to_owned();
    let row = rig.folder("Plan &APw-");
    assert_eq!(row.facts.gmail_label.as_deref(), Some("Plan &APw-"));
    assert!(!row.facts.store);
    let work = id_of(&rig, "Work");
    assert!(memberships(&rig).await.contains(&work));
    let renamed = set(&rig, json!({ "update": { (&work): { "name": "Job" } } })).await;
    assert_eq!(renamed["updated"][&work], json!(null), "{renamed}");
    assert_eq!(rig.folder("Job").facts.gmail_label.as_deref(), Some("Job"));
    assert!(memberships(&rig).await.contains(&work));
    let destroy = json!({ "destroy": [&work, &plan], "onDestroyRemoveEmails": true });
    let gone = set(&rig, destroy).await;
    assert_eq!(gone["destroyed"], json!([work, plan]), "{gone}");
    let kept = memberships(&rig).await;
    assert_eq!(kept, [id_of(&rig, ALL_MAIL)]);
    assert_eq!(
        sent(&rig),
        [
            "CREATE \"Plan &APw-\"",
            "SUBSCRIBE \"Plan &APw-\"",
            "RENAME \"Work\" \"Job\"",
            "SUBSCRIBE \"Job\"",
            "UNSUBSCRIBE \"Work\"",
            "DELETE \"Job\"",
            "UNSUBSCRIBE \"Job\"",
            "DELETE \"Plan &APw-\"",
            "UNSUBSCRIBE \"Plan &APw-\""
        ]
    );
}

/// The stores, `[Gmail]` and the system labels take no rename, no
/// destroy and no child; the roles of a Gmail account stand as the
/// server names them. Nothing is sent.
#[tokio::test]
async fn what_gmail_keeps_for_itself_refuses_every_change_with_nothing_sent() {
    let rig = started(gmail(), true).await;
    let (all, sent_mail, inbox) = (
        id_of(&rig, ALL_MAIL),
        id_of(&rig, "[Gmail]/Sent Mail"),
        id_of(&rig, "INBOX"),
    );
    let gmail_root = id_of(&rig, "[Gmail]");
    let call = json!({
        "create": { "k": { "name": "Mine", "parentId": gmail_root } },
        "update": {
            (&all): { "name": "Everything" },
            (&sent_mail): { "name": "Out" },
            (&inbox): { "role": null },
        },
    });
    let answer = set(&rig, call).await;
    assert_eq!(refused(&answer, "notCreated", "k"), "forbidden");
    for id in [&all, &sent_mail, &inbox] {
        assert_eq!(refused(&answer, "notUpdated", id), "forbidden");
    }
    let answer = set(&rig, json!({ "destroy": [&all, &sent_mail, &gmail_root] })).await;
    for id in [&all, &sent_mail, &gmail_root] {
        assert_eq!(refused(&answer, "notDestroyed", id), "forbidden");
    }
    assert_eq!(sent(&rig), Vec::<String>::new());
    let work = mailbox(&rig, &id_of(&rig, "Work")).await;
    assert_eq!(work["myRights"]["mayRename"], true);
    assert_eq!(work["myRights"]["mayDelete"], true);
}

/// The LOGIN, LOGOUT and LIST commands the server saw, in order, on the
/// conversation of a link that refreshes on every `/changes` call,
/// after two of them; `step` bounds each command of the connector.
async fn verbs_after_two_refreshes(
    mailboxes: Mailboxes,
    gmail_account: bool,
    list_age: Duration,
    step: Duration,
) -> Vec<String> {
    let mut rig = Rig::over(mailboxes).await;
    rig.cache.gmail = gmail_account;
    let connector = TestConnector::scripted(
        rig.fake.trusting(),
        rig.fake.target(HOST, TlsMode::Implicit),
        step,
    );
    let timing = Timing {
        interval: Duration::ZERO,
        list_age,
        ..Timing::default()
    };
    let link = Link::with_timing(connector, timing);
    let body = json!({
        "using": [CORE_CAPABILITY, MAIL_CAPABILITY],
        "methodCalls": [["Mailbox/changes", { "accountId": ACCOUNT, "sinceState": "0" }, "c1"]],
    });
    let bytes = serde_json::to_vec(&body).unwrap();
    for _ in 0..2 {
        handle(&rig.cache, &link, &bytes, ACCOUNT).await.unwrap();
    }
    let lines = rig.fake.lines();
    lines
        .iter()
        .filter_map(|line| line.split(' ').nth(2))
        .filter(|verb| matches!(*verb, "LOGIN" | "LOGOUT" | "LIST"))
        .map(str::to_owned)
        .collect()
}

/// What the server sees when the conversation signs in afresh for the
/// second pass: logged out first (RFC 9051 section 6.1.3).
const AFRESH: [&str; 5] = ["LOGIN", "LIST", "LOGOUT", "LOGIN", "LIST"];

/// What the server sees when the conversation is kept for the second
/// pass.
const KEPT: [&str; 3] = ["LOGIN", "LIST", "LIST"];

/// Gmail shows a connection the folders as they stood when it signed
/// in, so the pass of a Gmail account signs in afresh once its
/// conversation passed the list age; any other account keeps it.
#[tokio::test]
async fn a_gmail_conversation_past_the_list_age_signs_in_afresh_before_a_pass() {
    assert_eq!(
        verbs_after_two_refreshes(gmail(), true, Duration::ZERO, STEP).await,
        AFRESH
    );
    assert_eq!(
        verbs_after_two_refreshes(gmail(), true, AGES, STEP).await,
        KEPT
    );
    assert_eq!(
        verbs_after_two_refreshes(Mailboxes::dovecot(), false, Duration::ZERO, STEP).await,
        KEPT
    );
}

/// A conversation past the list age ends with LOGOUT before the fresh
/// sign-in (RFC 9051 section 6.1.3); a server that drops the connection
/// on it or answers BAD holds up neither the sign-in nor the pass.
#[tokio::test]
async fn a_gmail_conversation_past_the_list_age_is_logged_out_before_it_signs_in_afresh_rfc9051_6_1_3()
 {
    for logging_out in [Storing::Drops, Storing::Bad] {
        let mut mailboxes = gmail();
        mailboxes.behavior.logging_out = logging_out;
        assert_eq!(
            verbs_after_two_refreshes(mailboxes, true, Duration::ZERO, STEP).await,
            AFRESH,
            "{logging_out:?}"
        );
    }
}

/// A server that holds the answer to LOGOUT holds up no request: the
/// LOGOUT of a conversation past the list age runs on a task of its
/// own, so the fresh sign-in and the pass go ahead at once while it
/// waits (RFC 9051 section 6.1.3).
#[tokio::test]
async fn a_held_logout_of_a_conversation_past_the_list_age_holds_up_no_pass_rfc9051_6_1_3() {
    let mut mailboxes = gmail();
    mailboxes.behavior.logging_out = Storing::Stalls;
    let verbs = timeout(
        STEP,
        verbs_after_two_refreshes(mailboxes, true, Duration::ZERO, AGES),
    )
    .await
    .expect("the pass waited on the held LOGOUT");
    assert_eq!(verbs, AFRESH);
}
