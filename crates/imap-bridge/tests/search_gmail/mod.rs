// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! `Email/query` with a text or a sender on a Gmail account: one
//! X-GM-RAW string on All Mail for the default scope, the label of a
//! mailbox as Gmail spells it, Spam or Trash alone for its own scope and
//! both behind All Mail for the whole account.

use huliho_imap_bridge::testing::mailboxes::{ALL_MAIL, SPAM, TRASH};
use serde_json::json;

use crate::search_rig::{bins, commands, examined, found, gmail, id_of, mark, quick, synced};

#[tokio::test]
async fn the_default_scope_is_one_raw_search_of_all_mail_without_a_label() {
    let rig = synced(gmail(), true, quick()).await;
    let from = mark(&rig);
    let filter = json!({ "inMailboxOtherThan": bins(&rig), "text": "plan" });
    assert_eq!(found(&rig, filter).await, [11, 3, 1]);
    assert_eq!(examined(&rig, from), [ALL_MAIL]);
    assert_eq!(
        commands(&rig, from, "UID SEARCH "),
        ["UID SEARCH CHARSET UTF-8 1:4 X-GM-RAW {4+}\r\nplan"]
    );
    let from = mark(&rig);
    let sender = json!({ "inMailboxOtherThan": bins(&rig), "from": "Anna Jansen" });
    assert_eq!(found(&rig, sender).await, [1]);
    let raw = "from:\"Anna Jansen\"";
    assert_eq!(
        commands(&rig, from, "UID SEARCH "),
        [format!(
            "UID SEARCH CHARSET UTF-8 1:4 X-GM-RAW {{{}+}}\r\n{raw}",
            raw.len()
        )]
    );
}

#[tokio::test]
async fn a_label_mailbox_searches_all_mail_under_its_label_as_gmail_spells_it() {
    let rig = synced(gmail(), true, quick()).await;
    let from = mark(&rig);
    let inbox = json!({ "inMailbox": id_of(&rig, "INBOX"), "text": "plan" });
    assert_eq!(found(&rig, inbox).await, [3, 1]);
    let starred = json!({ "inMailbox": id_of(&rig, "[Gmail]/Starred"), "text": "plan" });
    assert_eq!(found(&rig, starred).await, [1]);
    let all_mail = json!({ "inMailbox": id_of(&rig, ALL_MAIL), "text": "plan" });
    assert_eq!(found(&rig, all_mail).await, [11, 3, 1]);
    assert_eq!(
        commands(&rig, from, "UID SEARCH "),
        [
            "UID SEARCH CHARSET UTF-8 1:4 X-GM-RAW {4+}\r\nplan X-GM-LABELS {6+}\r\n\\Inbox",
            "UID SEARCH CHARSET UTF-8 1:4 X-GM-RAW {4+}\r\nplan X-GM-LABELS {8+}\r\n\\Starred",
            "UID SEARCH CHARSET UTF-8 1:4 X-GM-RAW {4+}\r\nplan",
        ]
    );
    assert_eq!(examined(&rig, from), [ALL_MAIL, ALL_MAIL, ALL_MAIL]);
}

#[tokio::test]
async fn spam_or_trash_is_searched_alone_for_its_own_scope_and_after_all_mail_for_the_account() {
    let rig = synced(gmail(), true, quick()).await;
    let from = mark(&rig);
    let spam = json!({ "inMailbox": id_of(&rig, SPAM), "text": "plan" });
    assert_eq!(found(&rig, spam).await, [21]);
    assert_eq!(examined(&rig, from), [SPAM]);
    let from = mark(&rig);
    assert_eq!(
        found(&rig, json!({ "text": "plan" })).await,
        [11, 31, 21, 3, 1]
    );
    assert_eq!(examined(&rig, from), [ALL_MAIL, SPAM, TRASH]);
}
