// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A draft through `Email/set` on a folder account: APPEND with the
//! literal in the mode the server reads, the row written at once from
//! `APPENDUID` or landed by one pass, one `UID REPLACE` where advertised
//! and the three steps elsewhere, a new id per save (RFC 8621 section 4.6,
//! RFC 8508, RFC 4315).

mod draft_rig;
mod email_create_first_sync;
mod email_create_gmail;
mod email_create_refusals;
mod email_create_uploads;
mod email_create_wire;
mod set_rig;
mod sync_rig;

use draft_rig::{
    DRAFTS, INBOX, account, appends, create, created, drafts, email, held, object, refreshed_equal,
    replace, sized, synced, without,
};
use huliho_imap_bridge::session::BODY_WINDOW_BYTES;
use huliho_imap_bridge::store::{ChangeKind, ObjectType};
use huliho_imap_bridge::testing::command::Command;
use huliho_imap_bridge::testing::{Extension, Folder, Message};
use mail_parser::{MessageParser, MimeHeaders};
use serde_json::{Value, json};
use set_rig::{refused, started};
use sync_rig::inbox;

/// The bytes LITERAL- reads without waiting (RFC 7888 section 5).
const LITERAL_MINUS_BYTES: usize = 4096;

/// A text whose message passes those bytes.
const PAST_LITERAL_MINUS: usize = 5000;

#[tokio::test]
async fn a_draft_lands_from_appenduid_and_answers_the_four_properties_rfc8621_4_6() {
    let set = synced(account(Extension::all())).await;
    let before = set.state();
    let answer = create(&set, object(&drafts(&set))).await;
    let id = created(&answer);
    let made = &answer["created"]["d"];
    assert_eq!(made["blobId"], id.as_str());
    assert!(made["threadId"].as_str().is_some(), "{answer}");
    let stored = held(&set, DRAFTS);
    assert_eq!(stored.len(), 1);
    let raw = stored[0].raw();
    assert_eq!(made["size"], raw.len());
    assert_eq!(answer["oldState"], before.to_string());
    assert_eq!(answer["newState"], (before + 1).to_string());
    assert_eq!(answer["notCreated"], Value::Null);
    let lines = appends(&set);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(
        lines[0],
        format!("APPEND \"Drafts\" (\\Draft \\Seen) {{{}+}}", raw.len())
    );
    assert!(set.written().is_empty(), "{:?}", set.written());
    let shown = email(
        &set,
        &id,
        &[
            "subject",
            "to",
            "bcc",
            "keywords",
            "mailboxIds",
            "preview",
            "hasAttachment",
            "messageId",
        ],
    )
    .await;
    assert_eq!(shown["subject"], "Caf\u{e9} om drie");
    assert_eq!(shown["to"][0]["name"], "Mo, the first");
    assert_eq!(shown["bcc"][0]["email"], "hidden@example.test");
    assert_eq!(shown["keywords"], json!({ "$draft": true, "$seen": true }));
    assert_eq!(shown["mailboxIds"], json!({ drafts(&set): true }));
    assert_eq!(shown["preview"], "Hallo wereld");
    assert_eq!(shown["hasAttachment"], false);
    let message_id = shown["messageId"][0].as_str().unwrap();
    assert!(message_id.ends_with("@example.test"), "{message_id}");
    let changes = set.rig.changes(ObjectType::Email, before);
    assert_eq!(changes, [(id.clone(), ChangeKind::Created)]);
    let mailboxes = set.rig.changes(ObjectType::Mailbox, before);
    assert_eq!(mailboxes, [(drafts(&set), ChangeKind::Updated)]);
    refreshed_equal(&set, &answer["newState"]).await;
}

#[tokio::test]
async fn the_message_on_the_server_is_the_tree_of_the_object_rfc2046_5_1() {
    let set = synced(account(Extension::all())).await;
    create(&set, object(&drafts(&set))).await;
    let raw = held(&set, DRAFTS)[0].raw();
    let parsed = MessageParser::default().parse(raw.as_bytes()).unwrap();
    assert_eq!(parsed.subject(), Some("Caf\u{e9} om drie"));
    assert_eq!(parsed.body_html(0).unwrap(), "<p>Hallo wereld</p>");
    assert_eq!(
        parsed.content_type().unwrap().c_subtype.as_deref(),
        Some("alternative")
    );
    assert!(raw.contains("format=\"flowed\""), "{raw}");
    assert!(raw.contains("delsp=\"yes\""), "{raw}");
    assert!(raw.contains("charset=\"utf-8\""), "{raw}");
    assert!(raw.contains("Bcc: <hidden@example.test>\r\n"), "{raw}");
    assert!(raw.contains("MIME-Version: 1.0\r\n"), "{raw}");
    assert!(!raw.contains("User-Agent"), "{raw}");
}

/// The first command line and the literal it carried.
fn wire(set: &set_rig::Set) -> Command {
    let lines = set.rig.fake.lines();
    let line = lines.iter().find(|line| line.contains(" APPEND ")).unwrap();
    let (_, tagged) = line.split_once(' ').unwrap();
    Command::from_line(tagged).unwrap()
}

#[tokio::test]
async fn a_literal_of_each_mode_arrives_whole_rfc7888_3_5() {
    let cases = [
        (Extension::all(), LITERAL_MINUS_BYTES * 2, "+", 0),
        (literal_minus(), LITERAL_MINUS_BYTES / 2, "+", 0),
        (literal_minus(), PAST_LITERAL_MINUS, "", 1),
        (without(&[Extension::LiteralPlus]), 16, "", 1),
    ];
    for (extensions, bytes, plus, waited) in cases {
        let set = synced(account(extensions)).await;
        let answer = create(&set, sized(&drafts(&set), bytes)).await;
        created(&answer);
        let raw = held(&set, DRAFTS)[0].raw();
        let line = &appends(&set)[0];
        assert!(
            line.ends_with(&format!("{{{}{plus}}}", raw.len())),
            "{line}"
        );
        let command = wire(&set);
        assert_eq!(command.waited(), waited, "{line}");
        assert_eq!(
            command.line().split_once("\r\n").unwrap().1,
            raw,
            "the literal came whole"
        );
    }
}

/// Dovecot's extensions with LITERAL- in place of LITERAL+.
fn literal_minus() -> std::collections::BTreeSet<Extension> {
    let mut extensions = without(&[Extension::LiteralPlus]);
    extensions.insert(Extension::LiteralMinus);
    extensions
}

#[tokio::test]
async fn a_replace_is_one_command_where_the_server_offers_it_rfc8508_3_3() {
    let set = synced(account(Extension::all())).await;
    let mailbox = drafts(&set);
    let first = created(&create(&set, object(&mailbox)).await);
    let old_uid = held(&set, DRAFTS)[0].uid;
    let before = set.state();
    let answer = replace(&set, object(&mailbox), &first).await;
    let second = created(&answer);
    assert_ne!(first, second, "a new message has a new id");
    assert_eq!(answer["created"]["d"]["blobId"], second.as_str());
    assert_eq!(answer["destroyed"], json!([first]));
    assert_eq!(answer["newState"], (before + 1).to_string());
    let stored = held(&set, DRAFTS);
    assert_eq!(stored.len(), 1);
    assert!(stored[0].uid > old_uid);
    let lines = appends(&set);
    assert!(
        lines[1].starts_with(&format!(
            "UID REPLACE {old_uid} \"Drafts\" (\\Draft \\Seen) {{"
        )),
        "{lines:?}"
    );
    assert_eq!(set.written(), ["SELECT \"Drafts\""]);
    assert_eq!(email(&set, &first, &["id"]).await, Value::Null);
    assert_eq!(email(&set, &second, &["id"]).await["id"], second.as_str());
    let changes = set.rig.changes(ObjectType::Email, before);
    assert!(
        changes.contains(&(first.clone(), ChangeKind::Destroyed)),
        "{changes:?}"
    );
    assert!(
        changes.contains(&(second.clone(), ChangeKind::Created)),
        "{changes:?}"
    );
    refreshed_equal(&set, &answer["newState"]).await;
}

#[tokio::test]
async fn without_replace_the_three_steps_run_and_the_old_row_goes_rfc4315_2_1() {
    let set = synced(account(without(&[Extension::Replace]))).await;
    let mailbox = drafts(&set);
    let first = created(&create(&set, object(&mailbox)).await);
    let old_uid = held(&set, DRAFTS)[0].uid;
    let answer = replace(&set, object(&mailbox), &first).await;
    let second = created(&answer);
    assert_eq!(answer["destroyed"], json!([first]));
    assert_eq!(appends(&set).len(), 2);
    assert!(appends(&set)[1].starts_with("APPEND "));
    assert_eq!(
        set.written(),
        [
            "SELECT \"Drafts\"".to_owned(),
            format!("UID STORE {old_uid} +FLAGS.SILENT (\\Deleted)"),
            format!("UID EXPUNGE {old_uid}"),
        ]
    );
    assert_eq!(held(&set, DRAFTS).len(), 1);
    assert_eq!(email(&set, &first, &["id"]).await, Value::Null);
    assert_eq!(email(&set, &second, &["id"]).await["id"], second.as_str());
    refreshed_equal(&set, &answer["newState"]).await;
}

#[tokio::test]
async fn a_blob_of_many_windows_opens_its_folder_once_rfc3501_6_3_2() {
    let uid = 7;
    let bytes = usize::try_from(BODY_WINDOW_BYTES).unwrap();
    let big = Message {
        body: "x".repeat(bytes + 1),
        ..Message::new(uid)
    };
    let mailboxes = inbox(vec![big]);
    mailboxes.push(Folder::special(DRAFTS, "\\Drafts"));
    let set = Box::pin(started(mailboxes, INBOX, false)).await;
    Box::pin(set.rig.sync(DRAFTS)).await;
    let before = set.rig.fake.lines().len();
    let mut object = object(&drafts(&set));
    object["bodyStructure"] = json!({ "blobId": set.ids[&uid], "type": "message/rfc822" });
    created(&create(&set, object).await);
    let lines = set.rig.fake.lines();
    let asked = &lines[before..];
    let opened = asked
        .iter()
        .filter(|line| line.contains("EXAMINE \"INBOX\""));
    assert_eq!(opened.count(), 1, "{asked:?}");
    let windows = asked.iter().filter(|line| line.contains("BODY.PEEK[]<"));
    assert!(windows.count() > 1, "{asked:?}");
}

#[tokio::test]
async fn a_destroy_in_another_mailbox_runs_beside_the_create_rfc8620_5_3() {
    let set = synced(account(Extension::all())).await;
    let other = set.ids[&1].clone();
    let answer = replace(&set, object(&drafts(&set)), &other).await;
    created(&answer);
    assert_eq!(answer["destroyed"], json!([other]), "{answer}");
    let lines = appends(&set);
    assert!(lines[0].starts_with("APPEND "), "{lines:?}");
    assert_eq!(held(&set, INBOX).len(), 2);
}

#[tokio::test]
async fn a_refused_destroy_after_a_landed_append_keeps_both_for_the_next_save_rfc8620_5_3() {
    let mut mailboxes = account(without(&[Extension::Replace]));
    mailboxes.behavior.moving = huliho_imap_bridge::testing::Storing::Refuses;
    let set = synced(mailboxes).await;
    let mailbox = drafts(&set);
    let first = created(&create(&set, object(&mailbox)).await);
    let answer = replace(&set, object(&mailbox), &first).await;
    let second = created(&answer);
    assert_eq!(
        answer["notDestroyed"][&first]["type"], "serverFail",
        "{answer}"
    );
    assert_eq!(answer["destroyed"], Value::Null);
    assert_eq!(held(&set, DRAFTS).len(), 2);
    assert_eq!(email(&set, &first, &["id"]).await["id"], first.as_str());
    assert_eq!(email(&set, &second, &["id"]).await["id"], second.as_str());
}

#[tokio::test]
async fn a_destroy_of_a_draft_alone_runs_as_any_destroy_rfc4315_2_1() {
    let set = synced(account(Extension::all())).await;
    let first = created(&create(&set, object(&drafts(&set))).await);
    let uid = held(&set, DRAFTS)[0].uid;
    let answer = set.set(json!({ "destroy": [first] })).await;
    assert_eq!(answer["destroyed"], json!([first]), "{answer}");
    assert_eq!(
        set.written(),
        [
            "SELECT \"Drafts\"".to_owned(),
            format!("UID STORE {uid} +FLAGS.SILENT (\\Deleted)"),
            format!("UID EXPUNGE {uid}"),
        ]
    );
    assert_eq!(held(&set, DRAFTS).len(), 0);
}

#[tokio::test]
async fn a_server_without_uidplus_lands_the_row_from_one_pass_rfc4315_3() {
    let set = synced(account(without(&[Extension::Uidplus, Extension::Replace]))).await;
    let before = set.state();
    let answer = create(&set, object(&drafts(&set))).await;
    let id = created(&answer);
    let raw_size = held(&set, DRAFTS)[0].raw().len();
    assert_eq!(answer["created"]["d"]["size"], raw_size, "{answer}");
    assert_eq!(answer["created"]["d"]["blobId"], id.as_str());
    assert!(
        answer["newState"].as_str().unwrap().parse::<u64>().unwrap() > before,
        "{answer}"
    );
    let shown = email(&set, &id, &["subject", "keywords", "mailboxIds"]).await;
    assert_eq!(shown["subject"], "Caf\u{e9} om drie");
    assert_eq!(shown["keywords"], json!({ "$draft": true, "$seen": true }));
    assert_eq!(shown["mailboxIds"], json!({ drafts(&set): true }));
    let created_ids = set.rig.created(before);
    assert_eq!(created_ids, [id], "one row, as the pass found it");
}

#[tokio::test]
async fn a_folder_made_in_the_same_request_takes_the_draft_and_the_ids_come_back_rfc8620_5_3() {
    let set = synced(account(Extension::all())).await;
    let body = json!({
        "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
        "createdIds": {},
        "methodCalls": [
            ["Mailbox/set", { "accountId": sync_rig::ACCOUNT, "create": { "m": { "name": "Concepten" } } }, "c0"],
            ["Email/set", { "accountId": sync_rig::ACCOUNT, "create": { "d": object("#m") } }, "c1"],
        ],
    });
    let bytes = huliho_imap_bridge::jmap::handle(
        &set.rig.cache,
        &set.rig.link,
        &serde_json::to_vec(&body).unwrap(),
        sync_rig::ACCOUNT,
    )
    .await
    .unwrap();
    let answer: Value = serde_json::from_slice(&bytes).unwrap();
    let made = &answer["methodResponses"][1][1]["created"]["d"];
    let id = made["id"].as_str().unwrap_or_else(|| panic!("{answer}"));
    assert_eq!(answer["createdIds"]["d"], id, "{answer}");
    let mailbox = answer["createdIds"]["m"].as_str().unwrap();
    assert_eq!(held(&set, "Concepten").len(), 1);
    let shown = email(&set, id, &["mailboxIds"]).await;
    assert_eq!(shown["mailboxIds"], json!({ mailbox: true }));
}

#[tokio::test]
async fn an_answer_whose_state_moved_sends_nothing_rfc8620_5_3() {
    let set = synced(account(Extension::all())).await;
    let arguments = json!({ "ifInState": "nine", "create": { "d": object(&drafts(&set)) } });
    let answer = set.set(arguments).await;
    assert!(answer["type"] == "stateMismatch", "{answer}");
    assert_eq!(appends(&set), Vec::<String>::new());
}

#[tokio::test]
async fn two_creates_of_one_call_land_in_order_rfc8620_5_3() {
    let set = synced(account(Extension::all())).await;
    let mailbox = drafts(&set);
    let answer = set
        .set(json!({ "create": { "a": object(&mailbox), "b": object(&mailbox) } }))
        .await;
    let (a, b) = (&answer["created"]["a"]["id"], &answer["created"]["b"]["id"]);
    assert!(a.is_string() && b.is_string() && a != b, "{answer}");
    assert_eq!(held(&set, DRAFTS).len(), 2);
    assert_eq!(answer["notCreated"], Value::Null);
}

#[tokio::test]
async fn a_second_create_after_a_refused_first_still_goes_rfc8620_5_3() {
    let set = synced(account(Extension::all())).await;
    let mailbox = drafts(&set);
    let answer = set
        .set(json!({ "create": { "a": { "subject": "x" }, "d": object(&mailbox) } }))
        .await;
    assert_eq!(answer["notCreated"]["a"]["type"], "invalidProperties");
    assert!(answer["created"]["d"]["id"].is_string(), "{answer}");
}

#[tokio::test]
async fn an_update_of_one_call_runs_after_its_create_rfc8620_5_3() {
    let set = synced(account(Extension::all())).await;
    let before = set.state();
    let update = json!({ &set.ids[&1]: { "keywords/$flagged": true } });
    let arguments = json!({ "create": { "d": object(&drafts(&set)) }, "update": update });
    let answer = set.set(arguments).await;
    created(&answer);
    assert_eq!(answer["updated"], json!({ &set.ids[&1]: null }), "{answer}");
    assert_eq!(set.keywords(1).await["$flagged"], true);
    assert!(
        set.flags(draft_rig::INBOX, 1)
            .contains(&"\\Flagged".to_owned())
    );
    assert_eq!(
        set.updated(ObjectType::Email, before),
        [set.ids[&1].clone()]
    );
    let lines = set.rig.fake.lines();
    let at = |word: &str| lines.iter().position(|line| line.contains(word)).unwrap();
    assert!(at(" APPEND ") < at(" UID STORE "), "{lines:?}");
    let unknown = json!({ "mailboxIds/nowhere": true });
    let answer = set.update(&[(2, unknown)]).await;
    assert_eq!(refused(&set, &answer, 2), "invalidProperties");
}
