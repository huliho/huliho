// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! `Email/query` with a text or a sender on a folder account: the
//! folders of each scope in order with Junk and Trash last, a folder
//! gone or renumbered since its sync passed over, the dates exact in
//! SQL against a SEARCH a day wider each side, one email per thread
//! over hits from two folders, pages that cost no SEARCH, a filter
//! without a text or a sender from SQL alone, the literals in the form
//! the server takes and what the server refuses.

mod search_faults;
mod search_gmail;
mod search_rig;
mod sync_rig;

use huliho_imap_bridge::testing::{Behavior, Extension};
use search_rig::{
    behaving, bins, commands, dovecot, examined, found, id_of, inbox_mail, mark, query, quick,
    synced, uids,
};
use serde_json::json;
use sync_rig::inbox;

/// The dates of the cuts, as `UTCDate`s.
const JAN_4: &str = "2026-01-04T00:00:00Z";
const JAN_4_LATE: &str = "2026-01-04T22:00:00Z";
const JAN_5: &str = "2026-01-05T00:00:00Z";

#[tokio::test]
async fn the_default_scope_searches_every_folder_but_junk_and_trash_in_tree_order_rfc8621_4_4_1() {
    let rig = synced(dovecot(), false, quick()).await;
    let from = mark(&rig);
    let filter = json!({ "inMailboxOtherThan": bins(&rig), "text": "plan" });
    assert_eq!(found(&rig, filter).await, [11, 3, 1]);
    assert_eq!(examined(&rig, from), ["INBOX", "Drafts", "Sent", "Archive"]);
    assert_eq!(
        commands(&rig, from, "UID SEARCH "),
        [
            "UID SEARCH CHARSET UTF-8 1:3 TEXT {4+}\r\nplan",
            "UID SEARCH CHARSET UTF-8 1:1 TEXT {4+}\r\nplan",
        ]
    );
}

#[tokio::test]
async fn a_search_of_the_whole_account_takes_junk_and_trash_last_and_a_mailbox_its_own_folder() {
    let rig = synced(dovecot(), false, quick()).await;
    let from = mark(&rig);
    assert_eq!(
        found(&rig, json!({ "text": "plan" })).await,
        [11, 31, 21, 3, 1]
    );
    assert_eq!(
        examined(&rig, from),
        ["INBOX", "Drafts", "Sent", "Archive", "Junk", "Trash"]
    );
    let from = mark(&rig);
    let archive = json!({ "inMailbox": id_of(&rig, "Archive"), "text": "plan" });
    assert_eq!(found(&rig, archive).await, [11]);
    assert_eq!(examined(&rig, from), ["Archive"]);
    let junk = json!({ "inMailbox": id_of(&rig, "Junk"), "from": "deals" });
    assert_eq!(found(&rig, junk).await, [21]);
}

#[tokio::test]
async fn a_folder_gone_or_renumbered_is_skipped_and_the_rest_answer_rfc3501_2_3_1_1_and_6_3_2() {
    let mailboxes = dovecot();
    let rig = synced(mailboxes.clone(), false, quick()).await;
    // Archive shows a UIDVALIDITY one past the one its rows stand under
    // and Junk is gone, so its EXAMINE answers NO; both hold the word.
    let mut folders = mailboxes.folders();
    let archive = folders.iter_mut().find(|folder| folder.name == "Archive");
    archive.unwrap().uid_validity += 1;
    mailboxes.set(folders);
    mailboxes.remove("Junk");
    let from = mark(&rig);
    assert_eq!(found(&rig, json!({ "text": "plan" })).await, [31, 3, 1]);
    assert_eq!(
        examined(&rig, from),
        ["INBOX", "Drafts", "Sent", "Archive", "Junk", "Trash"]
    );
    assert_eq!(
        commands(&rig, from, "UID SEARCH "),
        [
            "UID SEARCH CHARSET UTF-8 1:3 TEXT {4+}\r\nplan",
            "UID SEARCH CHARSET UTF-8 1:1 TEXT {4+}\r\nplan",
        ]
    );
}

#[tokio::test]
async fn the_dates_are_exact_in_sql_and_the_search_goes_a_day_wider_each_side_rfc3501_6_4_4() {
    let rig = synced(dovecot(), false, quick()).await;
    let from = mark(&rig);
    // Bob wrote on 3 January at 23:30 behind UTC, which is 4 January in UTC.
    let since = json!({ "from": "bob", "after": JAN_4 });
    assert_eq!(found(&rig, since).await, [2]);
    // The archived answer is 4 January in UTC and 5 January as written.
    let both = json!({ "from": "anna", "after": JAN_4, "before": JAN_5 });
    assert_eq!(found(&rig, both).await, [11]);
    let searched = commands(&rig, from, "UID SEARCH ");
    assert!(
        searched[0].ends_with("FROM {3+}\r\nbob SINCE 3-Jan-2026"),
        "{searched:?}"
    );
    assert!(
        searched
            .iter()
            .any(|line| line.ends_with("SINCE 3-Jan-2026 BEFORE 6-Jan-2026")),
        "{searched:?}"
    );
    let cut = json!({ "inMailboxOtherThan": bins(&rig), "text": "plan", "before": JAN_4_LATE });
    assert_eq!(found(&rig, cut).await, [3, 1]);
}

#[tokio::test]
async fn one_email_stands_for_its_thread_over_hits_from_two_folders_rfc8621_4_4_3() {
    let rig = synced(dovecot(), false, quick()).await;
    let filter = json!({ "inMailboxOtherThan": bins(&rig), "text": "plan" });
    let collapsed = query(&rig, filter.clone(), json!({ "collapseThreads": true })).await;
    assert_eq!(uids(&rig, &collapsed).await, [11]);
    assert_eq!(collapsed["total"], 1);
    let oldest = json!({ "collapseThreads": true, "sort": [{ "property": "receivedAt" }] });
    let first = query(&rig, filter, oldest).await;
    assert_eq!(uids(&rig, &first).await, [1]);
}

#[tokio::test]
async fn pages_by_position_and_by_anchor_cost_no_search_rfc8620_5_5() {
    let rig = synced(dovecot(), false, quick()).await;
    let filter = json!({ "inMailboxOtherThan": bins(&rig), "text": "plan" });
    let whole = query(&rig, filter.clone(), json!({})).await;
    assert_eq!(whole["total"], 3);
    let from = mark(&rig);
    let second = query(&rig, filter.clone(), json!({ "position": 1, "limit": 1 })).await;
    assert_eq!(uids(&rig, &second).await, [3]);
    let anchor = whole["ids"][1].clone();
    let after = json!({ "anchor": anchor, "anchorOffset": 1, "limit": 2 });
    let third = query(&rig, filter.clone(), after).await;
    assert_eq!(
        (third["position"].clone(), uids(&rig, &third).await),
        (json!(2), vec![1])
    );
    let gone = query(&rig, filter, json!({ "anchor": "e-nope" })).await;
    assert_eq!(gone["type"], "anchorNotFound");
    assert_eq!(commands(&rig, from, "UID SEARCH "), Vec::<String>::new());
    assert_eq!(commands(&rig, from, "LOGIN "), Vec::<String>::new());
}

#[tokio::test]
async fn a_filter_without_a_text_or_a_sender_answers_from_sql_alone_rfc8621_4_4_1() {
    let rig = synced(dovecot(), false, quick()).await;
    let from = mark(&rig);
    let unread = json!({ "inMailboxOtherThan": bins(&rig), "notKeyword": "$seen" });
    assert_eq!(found(&rig, unread).await, [3]);
    let attached = json!({ "hasAttachment": true });
    assert_eq!(found(&rig, attached).await, [3]);
    let recent = json!({ "after": JAN_4 });
    assert_eq!(found(&rig, recent).await, [11, 2]);
    for whole in [json!({}), json!(null)] {
        assert_eq!(found(&rig, whole).await, [11, 2, 31, 21, 3, 1]);
    }
    let unfiltered = json!({ "accountId": sync_rig::ACCOUNT });
    let answer = rig.call(&json!(["Email/query", unfiltered, "q"])).await;
    let mut every: Vec<String> = serde_json::from_value(answer["ids"].clone()).unwrap();
    let mut created = rig.created(0);
    every.sort();
    created.sort();
    assert_eq!(every, created, "every email the sync created");
    let inbox = json!({ "inMailbox": id_of(&rig, "INBOX"), "after": JAN_4 });
    assert_eq!(found(&rig, inbox).await, [2]);
    assert_eq!(commands(&rig, from, "LOGIN "), Vec::<String>::new());
}

#[tokio::test]
async fn each_term_goes_in_the_literal_form_the_server_takes_rfc7888_1() {
    let mut synchronizing = inbox(inbox_mail());
    synchronizing.extensions.remove(&Extension::LiteralPlus);
    let rig = synced(synchronizing, false, quick()).await;
    let from = mark(&rig);
    let filter = json!({ "inMailbox": id_of(&rig, "INBOX"), "text": "zoë", "from": "anna" });
    assert_eq!(found(&rig, filter).await, [3, 1]);
    assert_eq!(
        commands(&rig, from, "UID SEARCH "),
        ["UID SEARCH CHARSET UTF-8 1:3 TEXT {4}\r\nzoë FROM {4}\r\nanna"]
    );
}

#[tokio::test]
async fn a_charset_the_server_refuses_is_an_unsupported_filter_with_why_rfc3501_7_1() {
    let refusing = behaving(
        dovecot(),
        Behavior {
            only_charset: Some("US-ASCII"),
            ..Behavior::default()
        },
    );
    let rig = synced(refusing, false, quick()).await;
    let filter = json!({ "inMailbox": id_of(&rig, "INBOX"), "text": "plan" });
    let answer = query(&rig, filter, json!({})).await;
    assert_eq!(answer["type"], "unsupportedFilter", "{answer}");
    assert_eq!(answer["description"], "the server cannot search in UTF-8");
}

#[tokio::test]
async fn a_term_past_the_caps_or_holding_a_nul_is_refused_before_anything_connects() {
    let rig = synced(dovecot(), false, quick()).await;
    let from = mark(&rig);
    let long = "a".repeat(huliho_imap_bridge::session::SEARCH_TERM_BYTES + 1);
    for (text, sender) in [
        (long.as_str(), ""),
        ("a b c d e", "f g h i"),
        ("plan\0", ""),
    ] {
        let answer = query(&rig, json!({ "text": text, "from": sender }), json!({})).await;
        assert_eq!(answer["type"], "invalidArguments", "{answer}");
        assert!(answer["description"].is_string());
    }
    assert!(
        rig.fake.lines()[from..].is_empty(),
        "{:?}",
        &rig.fake.lines()[from..]
    );
}
