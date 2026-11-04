// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A draft through `Email/set` on a Gmail account: APPEND to the Drafts
//! label, the row landed from one pass by the Message-ID the bridge wrote
//! and the old draft moved to Trash with `UID MOVE`, never a `\Deleted`
//! and never an expunge, since the account's own setting decides what one
//! does there.

use std::collections::HashMap;

use huliho_imap_bridge::session::Session;
use huliho_imap_bridge::store::Standing;
use huliho_imap_bridge::sync::FolderSync;
use huliho_imap_bridge::testing::mailboxes::{ALL_MAIL, TRASH};
use huliho_imap_bridge::testing::{Extension, Mailboxes, Message};
use serde_json::{Value, json};

use crate::draft_rig::{appends, create, created, email, held, object, refused, replace, sized};
use crate::set_rig::{Set, started};
use crate::sync_rig::Rig;

/// The wire name of the Drafts label.
const DRAFTS: &str = "[Gmail]/Drafts";

/// A text whose message passes what LITERAL- reads without waiting.
const PAST_LITERAL_MINUS: usize = 5000;

/// The highest UID that arrives under a first sync, well past what a read
/// beside it takes.
const ARRIVED_UNDER_THE_SYNC: u32 = 25;

/// The UID of a message that lies in Trash.
const TRASHED: u32 = 50;

async fn account() -> Set {
    Box::pin(started(
        Mailboxes::gmail(vec![Message::new(1)]),
        ALL_MAIL,
        true,
    ))
    .await
}

fn drafts(set: &Set) -> String {
    set.rig.folder(DRAFTS).id.to_string()
}

/// Whether the message carries the label of the Drafts folder.
fn labeled(message: &Message) -> bool {
    message.labels.iter().any(|label| label == "\\Drafts")
}

/// Whether any command stored `\Deleted` or expunged.
fn expunges(set: &Set) -> bool {
    set.written()
        .iter()
        .any(|line| line.contains("\\Deleted") || line.contains("EXPUNGE"))
}

#[tokio::test]
async fn a_draft_lands_while_all_mail_waits_for_its_first_sync_and_that_sync_skips_it() {
    let mut rig = Rig::over(Mailboxes::gmail(vec![Message::new(1)])).await;
    rig.cache.gmail = true;
    rig.pass().await.unwrap();
    let set = Set {
        rig,
        ids: HashMap::new(),
    };
    let mailbox = drafts(&set);
    let first = created(&create(&set, object(&mailbox)).await);
    let answer = replace(&set, object(&mailbox), &first).await;
    let second = created(&answer);
    assert_eq!(answer["destroyed"], json!([first]), "{answer}");
    assert_eq!(held(&set, TRASH).len(), 1);
    let before = set.state();
    Box::pin(set.rig.sync(ALL_MAIL)).await;
    let landed = set.rig.created(before);
    assert!(!landed.contains(&second), "{landed:?}");
    assert_eq!(email(&set, &second, &["id"]).await["id"], second.as_str());
}

/// The UIDs the rows hold of a folder.
fn rows_of(set: &Set, name: &str) -> Vec<u32> {
    let row = set.rig.folder(name);
    let standing = Standing {
        folder: &row.id,
        uid_validity: row.facts.uid_validity.unwrap(),
    };
    let cache = &set.rig.cache;
    cache.store.stored_uids(&cache.key, &standing).unwrap()
}

#[tokio::test]
async fn a_read_beside_a_first_sync_that_just_ended_loses_no_mail_and_moves_no_progress() {
    let mut rig = Rig::over(Mailboxes::gmail((1..=5).map(Message::new).collect())).await;
    rig.cache.gmail = true;
    rig.pass().await.unwrap();
    let (mut walking, walk) = rig.open(ALL_MAIL).await;
    let mut walk = walk.unwrap();
    let row = rig.folder(ALL_MAIL);
    let model = rig.fake.script().mailboxes.clone();
    for uid in 6..=ARRIVED_UNDER_THE_SYNC {
        model.append(ALL_MAIL, Message::new(uid));
    }
    walk.finish(&mut walking, &rig.cache).await.unwrap();
    let ended = rig
        .cache
        .store
        .sync_progress(&rig.cache.key, &row.id)
        .unwrap();
    let mut session = rig.session().await;
    session.examine(ALL_MAIL).await.unwrap();
    let top = (ARRIVED_UNDER_THE_SYNC - 2, ARRIVED_UNDER_THE_SYNC);
    let above = FolderSync::above(&mut session, &row, top, true).await;
    let mut beside = above.unwrap().unwrap().beside();
    beside.finish(&mut session, &rig.cache).await.unwrap();
    let read = rig
        .cache
        .store
        .sync_progress(&rig.cache.key, &row.id)
        .unwrap();
    assert_eq!(read, ended);
    let set = Set {
        rig,
        ids: HashMap::new(),
    };
    set.rig.link.refresh(&set.rig.cache).await.unwrap();
    let rows = rows_of(&set, ALL_MAIL);
    assert_eq!(rows, (1..=ARRIVED_UNDER_THE_SYNC).collect::<Vec<u32>>());
}

#[tokio::test]
async fn a_draft_into_a_store_that_waits_for_its_first_sync_lands_there() {
    let mut rig = Rig::over(Mailboxes::gmail(vec![Message::new(1)])).await;
    rig.cache.gmail = true;
    rig.pass().await.unwrap();
    let set = Set {
        rig,
        ids: HashMap::new(),
    };
    let trash = set.rig.folder(TRASH).id.to_string();
    let id = created(&create(&set, object(&trash)).await);
    assert_eq!(email(&set, &id, &["id"]).await["id"], id.as_str());
    assert_eq!(rows_of(&set, TRASH).len(), 1);
}

#[tokio::test]
async fn a_gmail_account_that_names_replace_never_replaces_in_trash() {
    let mut mailboxes = Mailboxes::gmail(vec![Message::new(1)]);
    mailboxes.extensions.insert(Extension::Replace);
    let set = Box::pin(started(mailboxes, ALL_MAIL, true)).await;
    let model = set.rig.fake.script().mailboxes.clone();
    model.append(TRASH, Message::new(TRASHED));
    let before = set.state();
    set.rig.pass().await.unwrap();
    Box::pin(set.rig.sync(TRASH)).await;
    let old = set.rig.created(before)[0].clone();
    let trash = set.rig.folder(TRASH).id.to_string();
    let answer = replace(&set, object(&trash), &old).await;
    assert_eq!(answer["destroyed"], json!([old]), "{answer}");
    created(&answer);
    let lines = appends(&set);
    assert!(
        lines.iter().all(|line| line.starts_with("APPEND ")),
        "{lines:?}"
    );
}

#[tokio::test]
async fn a_gmail_account_that_names_replace_still_moves_the_old_draft() {
    let mut mailboxes = Mailboxes::gmail(vec![Message::new(1)]);
    mailboxes.extensions.insert(Extension::Replace);
    let set = Box::pin(started(mailboxes, ALL_MAIL, true)).await;
    let mailbox = drafts(&set);
    let first = created(&create(&set, object(&mailbox)).await);
    let answer = replace(&set, object(&mailbox), &first).await;
    assert_eq!(answer["destroyed"], json!([first]), "{answer}");
    let lines = appends(&set);
    assert!(
        lines.iter().all(|line| line.starts_with("APPEND ")),
        "{lines:?}"
    );
    assert_eq!(held(&set, TRASH).len(), 1);
}

#[tokio::test]
async fn a_draft_lands_from_one_pass_under_its_labels() {
    let set = account().await;
    let before = set.state();
    let answer = create(&set, object(&drafts(&set))).await;
    let id = created(&answer);
    let lines = appends(&set);
    assert!(
        lines[0].starts_with("APPEND \"[Gmail]/Drafts\" (\\Draft \\Seen) {"),
        "{lines:?}"
    );
    let stored = held(&set, ALL_MAIL);
    let raw = stored.iter().find(|message| labeled(message));
    assert_eq!(answer["created"]["d"]["size"], raw.unwrap().raw().len());
    assert!(
        answer["created"]["d"]["threadId"]
            .as_str()
            .unwrap()
            .starts_with('t')
    );
    let shown = email(&set, &id, &["subject", "keywords", "mailboxIds"]).await;
    assert_eq!(shown["subject"], "Caf\u{e9} om drie");
    assert_eq!(shown["keywords"]["$draft"], true);
    let member = |name: &str| set.rig.folder(name).id.to_string();
    assert_eq!(
        shown["mailboxIds"],
        json!({ member(ALL_MAIL): true, member(DRAFTS): true }),
    );
    assert_eq!(set.rig.created(before), [id], "one row");
    assert!(!expunges(&set));
}

#[tokio::test]
async fn a_large_draft_waits_for_the_continuation_under_literal_minus_rfc7888_5() {
    let set = account().await;
    let answer = create(&set, sized(&drafts(&set), PAST_LITERAL_MINUS)).await;
    created(&answer);
    let line = &appends(&set)[0];
    assert!(!line.contains('+'), "{line}");
    let small = create(&set, sized(&drafts(&set), 1)).await;
    created(&small);
    assert!(appends(&set)[1].ends_with("+}"), "{:?}", appends(&set));
}

#[tokio::test]
async fn a_replace_moves_the_old_draft_to_trash_and_never_expunges() {
    let set = account().await;
    let mailbox = drafts(&set);
    let first = created(&create(&set, object(&mailbox)).await);
    let old = held(&set, ALL_MAIL)
        .iter()
        .find(|message| labeled(message))
        .map(|message| message.uid)
        .unwrap();
    let answer = replace(&set, object(&mailbox), &first).await;
    let second = created(&answer);
    assert_ne!(first, second);
    assert_eq!(answer["destroyed"], json!([first]), "{answer}");
    let written = set.written();
    assert!(
        written.contains(&format!("UID MOVE {old} \"{TRASH}\"")),
        "{written:?}"
    );
    assert!(!expunges(&set), "{written:?}");
    assert_eq!(held(&set, TRASH).len(), 1);
    assert_eq!(email(&set, &first, &["id"]).await, Value::Null);
    assert_eq!(email(&set, &second, &["id"]).await["id"], second.as_str());
}

#[tokio::test]
async fn a_destroy_of_a_draft_alone_goes_to_trash_and_of_any_other_message_stays_forbidden() {
    let set = account().await;
    let id = created(&create(&set, object(&drafts(&set))).await);
    let answer = set.set(json!({ "destroy": [id] })).await;
    assert_eq!(answer["destroyed"], json!([id]), "{answer}");
    assert_eq!(held(&set, TRASH).len(), 1);
    assert!(!expunges(&set));
    let other = set.set(json!({ "destroy": [set.ids[&1]] })).await;
    assert_eq!(
        other["notDestroyed"][&set.ids[&1]]["type"], "forbidden",
        "{other}"
    );
}

#[tokio::test]
async fn a_failed_append_leaves_the_old_draft_where_it_lies() {
    let mut mailboxes = Mailboxes::gmail(vec![Message::new(1)]);
    mailboxes.append_limit = Some(u64::from(u32::MAX));
    let set = Box::pin(started(mailboxes, ALL_MAIL, true)).await;
    let mailbox = drafts(&set);
    let first = created(&create(&set, object(&mailbox)).await);
    let mut bad = object(&mailbox);
    bad["bodyStructure"] = json!({ "blobId": "never handed out", "type": "image/png" });
    let answer = replace(&set, bad, &first).await;
    assert_eq!(refused(&answer), "blobNotFound", "{answer}");
    assert_eq!(answer["notDestroyed"][&first]["type"], "serverFail");
    assert_eq!(held(&set, TRASH).len(), 0);
}

#[tokio::test]
async fn a_gmail_server_without_move_answers_forbidden_for_the_old_draft() {
    let mut mailboxes = Mailboxes::gmail(vec![Message::new(1)]);
    mailboxes.extensions.remove(&Extension::Move);
    let set = Box::pin(started(mailboxes, ALL_MAIL, true)).await;
    let mailbox = drafts(&set);
    let first = created(&create(&set, object(&mailbox)).await);
    let answer = replace(&set, object(&mailbox), &first).await;
    created(&answer);
    assert_eq!(
        answer["notDestroyed"][&first]["type"], "forbidden",
        "{answer}"
    );
}
