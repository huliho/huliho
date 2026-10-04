// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! `Email/set` where the server does not take the write: a flag the
//! folder does not keep, a refused STORE, a lost connection, a server
//! that never answers, a host out of reach and a renumbered folder each
//! leave the rows as they were and say so per update; a set of messages
//! the server took before a failure is kept. On a Gmail account the
//! STORE goes to the store folder and every label mailbox of the
//! message reads as updated.

mod set_rig;
mod sync_rig;

use std::time::Duration;

use huliho_imap_bridge::runtime::{Link, Timing};
use huliho_imap_bridge::session::{MAX_STORE_UIDS, TlsMode};
use huliho_imap_bridge::store::ObjectType;
use huliho_imap_bridge::testing::folder::PERMANENT_FLAGS;
use huliho_imap_bridge::testing::imap::HOST;
use huliho_imap_bridge::testing::mailboxes::ALL_MAIL;
use huliho_imap_bridge::testing::{Behavior, Folder, Mailboxes, Message, Storing, TestConnector};
use serde_json::{Value, json};
use set_rig::{Set, refused, started};
use sync_rig::{ACCOUNT, inbox};

const INBOX: &str = "INBOX";

/// Room for a loopback exchange.
const STEP: Duration = Duration::from_secs(2);

/// A deadline a server that never answers runs past at once.
const SHORT_DEADLINE: Duration = Duration::from_millis(300);

/// Another UIDVALIDITY than the one the folder was synced under.
const RENUMBERED: u32 = 9;

/// One past the permanent flags a SELECT may name.
const TOO_MANY_FLAGS: usize = 1025;

/// An INBOX that keeps these flags over an unseen message, a seen one
/// and one that carries a keyword beside `\Seen`.
fn keeping(permanent: &'static str) -> Folder {
    let mail = vec![
        Message::new(1).flagged(&[]),
        Message::new(2),
        Message::new(3).flagged(&["\\Seen", "work"]),
    ];
    Folder {
        permanent: Some(permanent),
        ..Folder::new(INBOX).with_mail(mail)
    }
}

#[tokio::test]
async fn a_keyword_the_folder_does_not_keep_is_an_invalid_property_and_a_flag_is_forbidden() {
    let mailboxes = inbox(Vec::new());
    mailboxes.set(vec![keeping(PERMANENT_FLAGS.trim_end_matches(" \\*"))]);
    let set = started(mailboxes, INBOX, false).await;
    let answered = set
        .update(&[
            (1, json!({ "keywords/$seen": true })),
            (2, json!({ "keywords/Project": true })),
            (3, json!({ "keywords/work": null })),
        ])
        .await;
    assert_eq!(
        answered["updated"],
        json!({ &set.ids[&1]: null }),
        "{answered}"
    );
    assert_eq!(refused(&set, &answered, 2), "invalidProperties");
    let named = &answered["notUpdated"][&set.ids[&2]]["properties"];
    assert_eq!(*named, json!(["keywords/Project"]), "the path as written");
    assert_eq!(refused(&set, &answered, 3), "invalidProperties");
    assert_eq!(set.written().len(), 2, "one SELECT and the one STORE");
    set.rig
        .fake
        .script()
        .mailboxes
        .set(vec![keeping("\\Flagged")]);
    let forbidden = set.update(&[(1, json!({ "keywords/$seen": null }))]).await;
    assert_eq!(refused(&set, &forbidden, 1), "forbidden", "{forbidden}");
    assert_eq!(set.keywords(1).await, json!({ "$seen": true }));
}

/// Two unseen messages on a server that takes a STORE this way.
async fn storing(storing: Storing) -> Set {
    let behavior = Behavior {
        storing,
        ..Behavior::default()
    };
    over(2, behavior).await
}

/// That many unseen messages on a server that behaves this way.
async fn over(messages: u32, behavior: Behavior) -> Set {
    let mail = (1..=messages).map(|uid| Message::new(uid).flagged(&[]));
    let mut mailboxes = inbox(mail.collect());
    mailboxes.behavior = behavior;
    started(mailboxes, INBOX, false).await
}

async fn mark_both(set: &Set) -> Value {
    let seen = json!({ "keywords/$seen": true });
    set.update(&[(1, seen.clone()), (2, seen)]).await
}

/// The sign-ins the server saw.
fn logins(set: &Set) -> usize {
    let lines = set.rig.fake.lines();
    lines.iter().filter(|line| line.contains(" LOGIN ")).count()
}

/// Nothing of the write stands: no state, no keyword.
async fn untouched(set: &Set, before: u64) {
    assert_eq!(set.state(), before);
    for uid in [1, 2] {
        assert_eq!(set.keywords(uid).await, json!({}));
    }
}

#[tokio::test]
async fn a_refused_store_writes_nothing_and_keeps_the_session() {
    let set = storing(Storing::Refuses).await;
    let before = set.state();
    let answered = mark_both(&set).await;
    for uid in [1, 2] {
        assert_eq!(refused(&set, &answered, uid), "serverFail", "{answered}");
    }
    assert_eq!(answered["updated"], Value::Null);
    assert_eq!(answered["oldState"], answered["newState"]);
    untouched(&set, before).await;
    let signed = logins(&set);
    mark_both(&set).await;
    assert_eq!(logins(&set), signed, "the session stood");
    let words = serde_json::to_string(&answered).unwrap();
    assert!(!words.contains("refused"), "no server words: {words}");
}

#[tokio::test]
async fn a_store_answered_bad_and_a_select_answered_no_are_server_failures() {
    let set = storing(Storing::Bad).await;
    let before = set.state();
    let signed = logins(&set);
    let answered = mark_both(&set).await;
    assert_eq!(refused(&set, &answered, 1), "serverFail", "{answered}");
    untouched(&set, before).await;
    // The folder is gone on the server: SELECT answers NO.
    set.rig.fake.script().mailboxes.set(Vec::new());
    let answered = mark_both(&set).await;
    assert_eq!(refused(&set, &answered, 2), "serverFail", "{answered}");
    untouched(&set, before).await;
    let commands = set.written();
    assert_eq!(commands.len(), 3, "{commands:?}");
    assert_eq!(
        logins(&set),
        signed + 2,
        "the BAD cost a session, the NO none"
    );
    mark_both(&set).await;
    assert_eq!(logins(&set), signed + 2);
}

#[tokio::test]
async fn a_set_the_server_took_is_kept_when_the_next_one_fails() {
    let many = u32::try_from(MAX_STORE_UIDS).unwrap() + 50;
    let behavior = Behavior {
        storing: Storing::Drops,
        storing_from: 1,
        ..Behavior::default()
    };
    let set = over(many, behavior).await;
    let before = set.state();
    let patches: Vec<(u32, Value)> = (1..=many)
        .map(|uid| (uid, json!({ "keywords/$seen": true })))
        .collect();
    let answered = set.update(&patches).await;
    assert_eq!(
        answered["updated"].as_object().unwrap().len(),
        MAX_STORE_UIDS,
        "{answered}"
    );
    assert_eq!(answered["notUpdated"].as_object().unwrap().len(), 50);
    assert_eq!(refused(&set, &answered, many), "serverUnavailable");
    assert_eq!(answered["newState"], (before + 1).to_string());
    assert_eq!(set.keywords(1).await, json!({ "$seen": true }));
    assert_eq!(set.keywords(many).await, json!({}));
    assert_eq!(set.updated(ObjectType::Email, before).len(), MAX_STORE_UIDS);
}

#[tokio::test]
async fn an_update_whose_second_command_is_refused_is_not_updated_until_the_refresh() {
    let mut mailboxes = inbox(vec![Message::new(1).flagged(&["work"])]);
    mailboxes.behavior = Behavior {
        storing: Storing::Refuses,
        storing_from: 1,
        ..Behavior::default()
    };
    let set = started(mailboxes, INBOX, false).await;
    let before = set.state();
    let patch = json!({ "keywords/$seen": true, "keywords/work": null });
    let answered = set.update(&[(1, patch)]).await;
    assert_eq!(refused(&set, &answered, 1), "serverFail", "{answered}");
    assert_eq!(set.state(), before);
    assert_eq!(set.keywords(1).await, json!({ "work": true }));
    // The server took what the message gains and refused what it loses.
    assert_eq!(
        set.written()[1..],
        [
            "UID STORE 1 +FLAGS.SILENT (\\Seen)",
            "UID STORE 1 -FLAGS.SILENT (work)"
        ]
    );
    assert_eq!(set.flags(INBOX, 1), ["work", "\\Seen"]);
    let arguments = json!({ "accountId": ACCOUNT, "sinceState": before.to_string() });
    let changed = set
        .rig
        .call(&json!(["Email/changes", arguments, "c1"]))
        .await;
    assert_eq!(changed["updated"], json!([set.ids[&1]]), "{changed}");
    let read = json!({ "work": true, "$seen": true });
    assert_eq!(set.keywords(1).await, read);
}

#[tokio::test]
async fn a_connection_lost_under_the_store_writes_nothing_and_costs_the_session() {
    let set = storing(Storing::Drops).await;
    let before = set.state();
    let answered = mark_both(&set).await;
    for uid in [1, 2] {
        assert_eq!(refused(&set, &answered, uid), "serverUnavailable");
    }
    untouched(&set, before).await;
    let signed = logins(&set);
    mark_both(&set).await;
    assert_eq!(
        logins(&set),
        signed + 1,
        "a fresh session for the next write"
    );
}

/// Two unseen messages on a server that never answers a STORE, behind
/// a link whose deadline such a STORE runs past at once.
async fn stalling() -> Set {
    let mut set = storing(Storing::Stalls).await;
    let fake = &set.rig.fake;
    let connector =
        TestConnector::scripted(fake.trusting(), fake.target(HOST, TlsMode::Implicit), STEP);
    let timing = Timing {
        deadline: SHORT_DEADLINE,
        ..Timing::default()
    };
    set.rig.link = Link::with_timing(connector, timing);
    set
}

#[tokio::test]
async fn a_store_that_never_answers_ends_at_the_deadline_and_writes_nothing() {
    let set = stalling().await;
    let before = set.state();
    let answered = mark_both(&set).await;
    assert_eq!(
        refused(&set, &answered, 1),
        "serverUnavailable",
        "{answered}"
    );
    untouched(&set, before).await;
}

#[tokio::test]
async fn the_writes_of_one_round_share_a_deadline() {
    let set = stalling().await;
    let before = set.state();
    let marking = |uid: u32, call: &str| {
        let update = json!({ &set.ids[&uid]: { "keywords/$seen": true } });
        json!(["Email/set", { "accountId": ACCOUNT, "update": update }, call])
    };
    let answered = set.rig.calls(&[marking(1, "c1"), marking(2, "c2")]).await;
    for (index, uid) in [(0, 1), (1, 2)] {
        let response = &answered["methodResponses"][index][1];
        assert_eq!(
            refused(&set, response, uid),
            "serverUnavailable",
            "{answered}"
        );
    }
    untouched(&set, before).await;
    let commands = set.written();
    let stores = commands.iter().filter(|line| line.starts_with("UID STORE"));
    assert_eq!(stores.count(), 1, "the second write sent nothing");
}

#[tokio::test]
async fn a_select_the_bridge_cannot_read_is_a_server_failure_and_costs_the_session() {
    let set = storing(Storing::Stores).await;
    // A static text, as the folder model takes its flags.
    let flags: &'static str = Box::leak(vec!["k"; TOO_MANY_FLAGS].join(" ").into_boxed_str());
    let mail = vec![Message::new(1).flagged(&[]), Message::new(2).flagged(&[])];
    let naming = Folder {
        permanent: Some(flags),
        ..Folder::new(INBOX).with_mail(mail)
    };
    set.rig.fake.script().mailboxes.set(vec![naming]);
    let before = set.state();
    let signed = logins(&set);
    let answered = mark_both(&set).await;
    for uid in [1, 2] {
        assert_eq!(refused(&set, &answered, uid), "serverFail", "{answered}");
    }
    untouched(&set, before).await;
    assert_eq!(set.written(), ["SELECT \"INBOX\""]);
    mark_both(&set).await;
    assert_eq!(logins(&set), signed + 2, "each answer cost its session");
}

#[tokio::test]
async fn a_host_out_of_reach_answers_per_update_and_the_cache_keeps_answering() {
    let mut set = storing(Storing::Stores).await;
    set.rig.link = Link::new(TestConnector::Refusing);
    let before = set.state();
    let answered = mark_both(&set).await;
    for uid in [1, 2] {
        assert_eq!(refused(&set, &answered, uid), "serverUnavailable");
    }
    untouched(&set, before).await;
    assert!(set.written().is_empty());
}

#[tokio::test]
async fn a_folder_renumbered_since_the_sync_is_not_written() {
    let set = storing(Storing::Stores).await;
    let renumbered = Folder {
        uid_validity: RENUMBERED,
        ..Folder::new(INBOX).with_mail(vec![Message::new(1), Message::new(2)])
    };
    set.rig.fake.script().mailboxes.set(vec![renumbered]);
    let before = set.state();
    let answered = mark_both(&set).await;
    assert_eq!(
        refused(&set, &answered, 1),
        "serverUnavailable",
        "{answered}"
    );
    untouched(&set, before).await;
    assert_eq!(set.written(), ["SELECT \"INBOX\""]);
    // Once the pass saw the renumbering the rows wait for their match
    // and no UID names their messages: nothing is sent at all.
    set.rig.pass().await.unwrap();
    let answered = mark_both(&set).await;
    for uid in [1, 2] {
        assert_eq!(refused(&set, &answered, uid), "serverUnavailable");
        assert_eq!(set.keywords(uid).await, json!({}));
    }
    assert_eq!(set.written(), ["SELECT \"INBOX\""]);
}

#[tokio::test]
async fn on_gmail_the_store_goes_to_the_store_folder_and_every_label_mailbox_is_updated() {
    let mail = vec![
        Message::new(1).flagged(&[]).labeled(&["\\Inbox", "Work"]),
        Message::new(2).flagged(&[]).labeled(&[]),
    ];
    let mailboxes = Mailboxes::gmail(mail).with_label("Work");
    let set = started(mailboxes, ALL_MAIL, true).await;
    let unread = async |name: &str| {
        let arguments = json!({ "accountId": ACCOUNT, "ids": [set.rig.folder(name).id] });
        let got = set.rig.call(&json!(["Mailbox/get", arguments, "c1"])).await;
        got["list"][0]["unreadEmails"].as_u64().unwrap()
    };
    assert_eq!((unread(INBOX).await, unread("Work").await), (1, 1));
    let before = set.state();
    let answered = mark_both(&set).await;
    assert_eq!(answered["notUpdated"], Value::Null, "{answered}");
    assert_eq!(
        set.written(),
        [
            "SELECT \"[Gmail]/All Mail\"",
            "UID STORE 1:2 +FLAGS.SILENT (\\Seen)"
        ]
    );
    assert_eq!(set.flags(ALL_MAIL, 1), ["\\Seen"]);
    let mut expected: Vec<String> = [ALL_MAIL, INBOX, "Work"]
        .iter()
        .map(|name| set.rig.folder(name).id.to_string())
        .collect();
    expected.sort();
    assert_eq!(set.updated(ObjectType::Mailbox, before), expected);
    assert_eq!(set.updated(ObjectType::Email, before).len(), 2);
    assert_eq!((unread(INBOX).await, unread("Work").await), (0, 0));
}
