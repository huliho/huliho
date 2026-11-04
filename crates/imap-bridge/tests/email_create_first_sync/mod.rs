// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A draft on a folder account without UIDPLUS while its folder waits for
//! its first sync, which a pass leaves alone: its newest UIDs are read
//! beside the walk, so `created` still carries the id (RFC 4315 section 3).

use huliho_imap_bridge::sync::{SYNC_BATCH, Step};
use huliho_imap_bridge::testing::{Extension, Folder, Message};
use serde_json::{Map, Value, json};

use crate::draft_rig::{DRAFTS, INBOX, account, create, created, drafts, email, object, without};
use crate::set_rig::{Set, started};
use crate::sync_rig::inbox;

/// Creates in one call, more than the mail a read beside a sync allows for.
const MANY_CREATES: usize = 12;

async fn waiting() -> Set {
    let mailboxes = account(without(&[Extension::Uidplus, Extension::Replace]));
    Box::pin(started(mailboxes, INBOX, false)).await
}

#[tokio::test]
async fn a_draft_lands_and_the_first_sync_skips_its_row_rfc4315_3() {
    let set = waiting().await;
    let id = created(&create(&set, object(&drafts(&set))).await);
    let before = set.state();
    Box::pin(set.rig.sync(DRAFTS)).await;
    assert_eq!(set.rig.created(before), Vec::<String>::new());
    assert_eq!(email(&set, &id, &["id"]).await["id"], id.as_str());
}

#[tokio::test]
async fn a_read_beside_a_running_first_sync_moves_no_progress_rfc4315_3() {
    let messages = u32::try_from(SYNC_BATCH).unwrap() + 1;
    let mut mailboxes = inbox(vec![Message::new(1)]);
    let held = (1..=messages).map(Message::new).collect();
    mailboxes.push(Folder::special(DRAFTS, "\\Drafts").with_mail(held));
    mailboxes.extensions = without(&[Extension::Uidplus, Extension::Replace]);
    let set = Box::pin(started(mailboxes, INBOX, false)).await;
    let (mut session, walk) = set.rig.open(DRAFTS).await;
    let cache = &set.rig.cache;
    assert_eq!(
        walk.unwrap().batch(&mut session, cache).await.unwrap(),
        Step::More
    );
    let folder = set.rig.folder(DRAFTS).id;
    let before = cache.store.sync_progress(&cache.key, &folder).unwrap();
    created(&create(&set, object(&drafts(&set))).await);
    let after = cache.store.sync_progress(&cache.key, &folder).unwrap();
    assert_eq!(after, before);
}

#[tokio::test]
async fn every_create_of_a_call_lands_rfc4315_3() {
    let set = waiting().await;
    let mailbox = drafts(&set);
    let creates: Map<String, Value> = (0..MANY_CREATES)
        .map(|index| (format!("d{index}"), object(&mailbox)))
        .collect();
    let answer = set.set(json!({ "create": creates })).await;
    assert_eq!(answer["notCreated"], Value::Null, "{answer}");
    assert_eq!(answer["created"].as_object().unwrap().len(), MANY_CREATES);
}
