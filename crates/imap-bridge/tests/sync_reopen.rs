// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A first sync that opens its folder again, after the process restarted
//! or after the folder gave up, keeps what its first opening read: the
//! mail that arrived, the flags that changed and the messages that left
//! in between all reach the first refresh.

mod reopen_rig;
mod sync_rig;

use std::time::Duration;

use huliho_imap_bridge::runtime::{FOLDER_FAILURE_BOUND, Timing};
use huliho_imap_bridge::store::{ChangeKind, ObjectType, Progress, Standing};
use huliho_imap_bridge::sync::{SYNC_BATCH, Step};
use huliho_imap_bridge::testing::imap::FakeImap;
use huliho_imap_bridge::testing::mailboxes::ALL_MAIL;
use huliho_imap_bridge::testing::{Behavior, Folder, Mailboxes, Message, TestConnector};
use reopen_rig::{Gate, Process, Volume, held, quick, reach, serve};
use serde_json::json;
use sync_rig::{ACCOUNT, Rig, inbox};

const INBOX: &str = "INBOX";

/// One batch and a hundred messages more, so the first opening ends after
/// one batch with mail left below it.
const COUNT: u32 = 600;

/// The mail that arrives while the folder stands between two openings.
const ARRIVALS: u32 = 10;

/// What a folder made again under a new UIDVALIDITY numbers from.
const SHIFT: u32 = 1000;

/// A turn that ends soon after a fetch stops answering, so a folder gives
/// up without the test waiting long.
const SHORT_DEADLINE: Duration = Duration::from_secs(1);

fn batch() -> u32 {
    u32::try_from(SYNC_BATCH).unwrap()
}

/// The lowest UID the first batch of a folder of `COUNT` stores.
fn first_low() -> u32 {
    COUNT + 1 - batch()
}

fn count(messages: u32) -> usize {
    usize::try_from(messages).unwrap()
}

fn uids(from: u32, through: u32) -> Vec<u32> {
    (from..=through).collect()
}

fn mail(from: u32, through: u32) -> Vec<Message> {
    (from..=through).map(Message::new).collect()
}

/// Delivers `ARRIVALS` messages above `after`, as another client does.
fn arrive(model: &Mailboxes, folder: &str, after: u32) {
    for message in mail(after + 1, after + ARRIVALS) {
        model.append(folder, message);
    }
}

/// The second process of a restart, with what it runs on and the state the
/// first one stopped at.
struct Restarted {
    process: Process<TestConnector>,
    stopped_at: u64,
    _server: FakeImap,
    _volume: Volume,
}

/// A first process runs until the first batch of `folder` landed; then
/// `between` changes the server, the process stops and a second one over
/// the same file runs every first sync to its end. Its task refreshes the
/// account once the last folder is done, so a test reads what the second
/// process wrote through `/changes` since `stopped_at`.
async fn restart(
    model: &Mailboxes,
    folder: &str,
    gmail: bool,
    between: impl FnOnce(),
) -> Restarted {
    let volume = Volume::new();
    let holding = serve(model, held()).await;
    let first = Process::start(&volume, reach(&holding), gmail, quick());
    let landed =
        |process: &Process<_>| process.progress(folder).lowest_synced_uid == Some(first_low());
    assert!(first.wait(landed).await);
    between();
    let stopped_at = first.state();
    first.stop().await;
    let server = serve(model, Behavior::default()).await;
    let process = Process::start(&volume, reach(&server), gmail, quick());
    assert!(process.wait(Process::done).await);
    Restarted {
        process,
        stopped_at,
        _server: server,
        _volume: volume,
    }
}

#[tokio::test]
async fn a_restart_between_the_openings_fetches_what_arrived_in_between_rfc3501_2_3_1_1() {
    let model = inbox(mail(1, COUNT));
    let run = restart(&model, INBOX, false, || arrive(&model, INBOX, COUNT)).await;
    let process = &run.process;
    let changed = process.changes_since(run.stopped_at).await;
    assert_eq!(process.rows(INBOX), uids(1, COUNT + ARRIVALS));
    // The second opening's batch below the first and the mail in between.
    let created = changed["created"].as_array().unwrap().len();
    assert_eq!(created, count(first_low() - 1 + ARRIVALS), "{changed}");
    let synced = process.progress(INBOX).synced;
    assert_eq!(synced.uid_next, Some(COUNT + ARRIVALS + 1));
}

#[tokio::test]
async fn a_folder_that_gave_up_fetches_what_arrived_before_it_opened_again_rfc3501_2_3_1_1() {
    let model = inbox(mail(1, COUNT));
    let volume = Volume::new();
    let holding = serve(&model, held()).await;
    let answering = serve(&model, Behavior::default()).await;
    let edited = model.clone();
    let servers = (reach(&holding), reach(&answering));
    let between = move || arrive(&edited, INBOX, COUNT);
    let gate = Gate::new(servers, FOLDER_FAILURE_BOUND, between);
    let timing = Timing {
        deadline: SHORT_DEADLINE,
        ..quick()
    };
    let process = Process::start(&volume, gate, false, timing);
    assert!(process.wait(Process::done).await);
    // A second opening lists the folder again; a resumed sync never does.
    let listed = answering
        .lines()
        .iter()
        .filter(|line| line.contains(" UID SEARCH "))
        .count();
    assert!(listed > 0, "{:?}", answering.lines());
    process.changes_since(process.state()).await;
    assert_eq!(process.rows(INBOX), uids(1, COUNT + ARRIVALS));
}

#[tokio::test]
async fn a_flag_change_between_the_openings_reaches_the_first_refresh_rfc7162_3_1_4_1() {
    let model = inbox(mail(1, COUNT));
    let flagged = first_low() + 1;
    let change = || model.store_flags(INBOX, flagged, &["\\Seen", "\\Flagged"]);
    let run = restart(&model, INBOX, false, change).await;
    let process = &run.process;
    let changed = process.changes_since(run.stopped_at).await;
    let updated = changed["updated"].as_array().unwrap();
    assert_eq!(updated.len(), 1, "{changed}");
    let email = process.email(&updated[0], &["subject", "keywords"]).await;
    assert_eq!(email["subject"], format!("Message {flagged}"));
    assert_eq!(
        email["keywords"],
        json!({ "$seen": true, "$flagged": true })
    );
}

#[tokio::test]
async fn a_message_that_left_between_the_openings_leaves_at_the_first_refresh_rfc3501_7_4_1() {
    let model = inbox(mail(1, COUNT));
    let gone = first_low() + 1;
    let run = restart(&model, INBOX, false, || model.expunge_uid(INBOX, gone)).await;
    let process = &run.process;
    let changed = process.changes_since(run.stopped_at).await;
    let mut left = uids(1, COUNT);
    left.retain(|uid| *uid != gone);
    assert_eq!(process.rows(INBOX), left);
    assert_eq!(
        changed["destroyed"].as_array().unwrap().len(),
        1,
        "{changed}"
    );
}

#[tokio::test]
async fn a_second_opening_of_all_mail_on_its_own_connection_keeps_what_the_first_read() {
    let model = Mailboxes::gmail(mail(1, COUNT));
    let archived = first_low() + 1;
    let between = || {
        arrive(&model, ALL_MAIL, COUNT);
        model.relabel(ALL_MAIL, archived, &[]);
    };
    let run = restart(&model, ALL_MAIL, true, between).await;
    let process = &run.process;
    let changed = process.changes_since(run.stopped_at).await;
    assert_eq!(process.rows(ALL_MAIL), uids(1, COUNT + ARRIVALS));
    let created = changed["created"].as_array().unwrap().len();
    assert_eq!(created, count(first_low() - 1 + ARRIVALS), "{changed}");
    let updated = changed["updated"].as_array().unwrap();
    assert_eq!(updated.len(), 1, "{changed}");
    let email = process.email(&updated[0], &["subject", "mailboxIds"]).await;
    assert_eq!(email["subject"], format!("Message {archived}"));
    let inbox = process.folder(INBOX).unwrap().id;
    assert_eq!(email["mailboxIds"].get(inbox.as_str()), None, "{email}");
}

/// Opens the folder and writes one batch, as an opening that ends there.
async fn one_batch(rig: &Rig) {
    let (mut session, sync) = rig.open(INBOX).await;
    let step = sync.unwrap().batch(&mut session, &rig.cache).await;
    assert_eq!(step.unwrap(), Step::More);
}

fn progress(rig: &Rig) -> Progress {
    let folder = rig.folder(INBOX).id;
    rig.cache
        .store
        .sync_progress(&rig.cache.key, &folder)
        .unwrap()
}

fn rows(rig: &Rig) -> Vec<u32> {
    let row = rig.folder(INBOX);
    let standing = Standing {
        folder: &row.id,
        uid_validity: row.facts.uid_validity.unwrap(),
    };
    rig.cache
        .store
        .stored_uids(&rig.cache.key, &standing)
        .unwrap()
}

/// A `/changes` call, which refreshes the account first; the state before.
async fn refresh(rig: &Rig) -> u64 {
    let state = rig.cache.store.state(&rig.cache.key).unwrap();
    let arguments = json!({ "accountId": ACCOUNT, "sinceState": state.to_string() });
    rig.call(&json!(["Email/changes", arguments, "c1"])).await;
    state
}

#[tokio::test]
async fn a_folder_renumbered_between_the_openings_starts_over_under_its_new_uids_rfc3501_2_3_1_1() {
    let model = inbox(mail(1, COUNT));
    let rig = Rig::start(model.clone()).await;
    one_batch(&rig).await;
    let renumbered = (1..=COUNT).map(|uid| Message {
        uid: uid + SHIFT,
        ..Message::new(uid)
    });
    let mut folder = Folder::new(INBOX).with_mail(renumbered.collect());
    folder.uid_validity += 1;
    model.set(vec![folder]);
    rig.pass().await.unwrap();
    assert_eq!(rig.sync(INBOX).await.0, Step::Done);
    assert_eq!(rows(&rig), uids(SHIFT + 1, SHIFT + COUNT));
    let synced = progress(&rig).synced;
    assert_eq!(synced.uid_next, Some(SHIFT + COUNT + 1));
    assert_eq!(synced.messages, Some(COUNT));
}

#[tokio::test]
async fn a_folder_emptied_between_the_openings_holds_no_row_after_the_first_refresh_rfc3501_7_4_1()
{
    let model = inbox(mail(1, COUNT));
    let rig = Rig::start(model.clone()).await;
    one_batch(&rig).await;
    for uid in 1..=COUNT {
        model.expunge_uid(INBOX, uid);
    }
    assert_eq!(rig.sync(INBOX).await.0, Step::Done);
    let before = refresh(&rig).await;
    assert_eq!(rows(&rig), Vec::<u32>::new());
    let changes = rig.changes(ObjectType::Email, before);
    assert_eq!(changes.len(), SYNC_BATCH);
    assert!(
        changes
            .iter()
            .all(|(_, kind)| *kind == ChangeKind::Destroyed)
    );
}

#[tokio::test]
async fn a_third_opening_keeps_what_the_first_read_rfc3501_2_3_1_1() {
    let count = COUNT + batch();
    let model = inbox(mail(1, count));
    let rig = Rig::start(model.clone()).await;
    let mut top = count;
    for _ in 0..2 {
        one_batch(&rig).await;
        arrive(&model, INBOX, top);
        top += ARRIVALS;
    }
    assert_eq!(rig.sync(INBOX).await.0, Step::Done);
    let before = refresh(&rig).await;
    assert_eq!(rows(&rig), uids(1, top));
    assert_eq!(
        rig.created(before).len(),
        usize::try_from(top - count).unwrap()
    );
}
