// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Moves and destroys the server does not take: a refused MOVE or
//! expunge, a copy whose expunge fails, a `COPYUID` that does not pair
//! with the ask, a STORE of `\Deleted` that never answers, a folder that
//! does not keep `\Deleted` and a host out of reach each leave the rows
//! as they were and say so per object.

mod set_rig;
mod sync_rig;

use std::time::Duration;

use huliho_imap_bridge::runtime::{Link, Timing};
use huliho_imap_bridge::session::TlsMode;
use huliho_imap_bridge::store::ObjectType;
use huliho_imap_bridge::testing::folder::PERMANENT_FLAGS;
use huliho_imap_bridge::testing::imap::HOST;
use huliho_imap_bridge::testing::{Behavior, Extension, Folder, Storing, TestConnector};
use serde_json::{Value, json};
use set_rig::{Set, refused, started};
use sync_rig::{ACCOUNT, inbox};

const INBOX: &str = "INBOX";
const ARCHIVE: &str = "Archive";

/// Room for a loopback exchange.
const STEP: Duration = Duration::from_secs(2);

/// A deadline a server that never answers runs past at once.
const SHORT_DEADLINE: Duration = Duration::from_millis(300);

/// The flags a folder keeps when `\Deleted` is not among them.
const WITHOUT_DELETED: &str = "\\Answered \\Flagged \\Seen \\Draft \\*";

/// Two messages in an INBOX beside an Archive, on a server that behaves
/// this way, with or without MOVE.
async fn over(behavior: Behavior, moves: bool) -> Set {
    let mail = (1..=2).map(huliho_imap_bridge::testing::Message::new);
    let mut mailboxes = inbox(mail.collect());
    mailboxes.push(Folder::special(ARCHIVE, "\\Archive"));
    mailboxes.behavior = behavior;
    if !moves {
        mailboxes.extensions.remove(&Extension::Move);
    }
    started(mailboxes, INBOX, false).await
}

/// Both messages to the Archive.
async fn archive_both(set: &Set) -> Value {
    let (inbox, archive) = (set.rig.folder(INBOX).id, set.rig.folder(ARCHIVE).id);
    let patch =
        json!({ format!("mailboxIds/{inbox}"): null, format!("mailboxIds/{archive}"): true });
    set.update(&[(1, patch.clone()), (2, patch)]).await
}

/// Nothing of the write stands: no state, both rows in the INBOX as they
/// were.
async fn untouched(set: &Set, before: u64) {
    assert_eq!(set.state(), before);
    let inbox = set.rig.folder(INBOX).id.to_string();
    let ids = [&set.ids[&1], &set.ids[&2]];
    let arguments = json!({ "accountId": ACCOUNT, "ids": ids, "properties": ["mailboxIds"] });
    let got = set.rig.call(&json!(["Email/get", arguments, "c1"])).await;
    for email in got["list"].as_array().unwrap() {
        assert_eq!(email["mailboxIds"], json!({ &inbox: true }), "{email}");
    }
    for uid in [1, 2] {
        assert_eq!(set.keywords(uid).await, json!({ "$seen": true }));
    }
}

/// The sign-ins the server saw.
fn logins(set: &Set) -> usize {
    let lines = set.rig.fake.lines();
    lines.iter().filter(|line| line.contains(" LOGIN ")).count()
}

#[tokio::test]
async fn a_refused_move_and_a_refused_expunge_write_nothing_and_keep_the_session() {
    let refusing = Behavior {
        moving: Storing::Refuses,
        ..Behavior::default()
    };
    let set = over(refusing, true).await;
    let before = set.state();
    let answered = archive_both(&set).await;
    assert_eq!(refused(&set, &answered, 1), "serverFail", "{answered}");
    assert_eq!(refused(&set, &answered, 2), "serverFail");
    untouched(&set, before).await;
    let signed = logins(&set);
    let destroyed = set.set(json!({ "destroy": [set.ids[&1]] })).await;
    assert_eq!(
        destroyed["notDestroyed"][&set.ids[&1]]["type"],
        "serverFail"
    );
    assert_eq!(logins(&set), signed, "a refusal keeps the session");
    assert_eq!(set.updated(ObjectType::Email, before), Vec::<String>::new());
}

#[tokio::test]
async fn a_move_answered_bad_or_cut_off_writes_nothing_and_costs_the_session() {
    for (moving, error) in [
        (Storing::Bad, "serverFail"),
        (Storing::Drops, "serverUnavailable"),
    ] {
        let behavior = Behavior {
            moving,
            ..Behavior::default()
        };
        let set = over(behavior, true).await;
        let before = set.state();
        let answered = archive_both(&set).await;
        assert_eq!(refused(&set, &answered, 1), error, "{moving:?}: {answered}");
        untouched(&set, before).await;
        let signed = logins(&set);
        archive_both(&set).await;
        assert_eq!(logins(&set), signed + 1, "{moving:?}: a fresh session");
    }
}

#[tokio::test]
async fn a_copy_whose_expunge_fails_leaves_the_rows_for_the_next_refresh() {
    let refusing = Behavior {
        storing: Storing::Refuses,
        ..Behavior::default()
    };
    let set = over(refusing, false).await;
    let before = set.state();
    let answered = archive_both(&set).await;
    assert_eq!(refused(&set, &answered, 1), "serverFail", "{answered}");
    assert_eq!(
        set.written()[1..],
        [
            "UID COPY 1:2 \"Archive\"",
            "UID STORE 1:2 +FLAGS.SILENT (\\Deleted)"
        ]
    );
    untouched(&set, before).await;
    assert_eq!(set.flags(INBOX, 1), ["\\Seen"], "nothing was marked");
}

#[tokio::test]
async fn a_copyuid_that_names_a_uid_not_asked_fails_the_set_and_costs_the_session() {
    let lying = Behavior {
        copyuid: Some("COPYUID 1 9 1"),
        ..Behavior::default()
    };
    let set = over(lying, true).await;
    let before = set.state();
    let answered = archive_both(&set).await;
    assert_eq!(refused(&set, &answered, 1), "serverFail", "{answered}");
    untouched(&set, before).await;
    let signed = logins(&set);
    archive_both(&set).await;
    assert_eq!(
        logins(&set),
        signed + 1,
        "a fresh session for the next write"
    );
}

#[tokio::test]
async fn a_deleted_store_that_never_answers_ends_at_the_deadline() {
    let stalling = Behavior {
        storing: Storing::Stalls,
        ..Behavior::default()
    };
    let mut set = over(stalling, false).await;
    let fake = &set.rig.fake;
    let connector =
        TestConnector::scripted(fake.trusting(), fake.target(HOST, TlsMode::Implicit), STEP);
    let timing = Timing {
        deadline: SHORT_DEADLINE,
        ..Timing::default()
    };
    set.rig.link = Link::with_timing(connector, timing);
    let before = set.state();
    let destroyed = set.set(json!({ "destroy": [set.ids[&1]] })).await;
    let refusal = &destroyed["notDestroyed"][&set.ids[&1]]["type"];
    assert_eq!(refusal, "serverUnavailable", "{destroyed}");
    untouched(&set, before).await;
}

#[tokio::test]
async fn a_folder_that_does_not_keep_deleted_takes_no_destroy_and_no_copied_move() {
    let set = over(Behavior::default(), false).await;
    let mailboxes = &set.rig.fake.script().mailboxes;
    let mut folders = mailboxes.folders();
    assert!(PERMANENT_FLAGS.contains("\\Deleted"));
    folders[0].permanent = Some(WITHOUT_DELETED);
    mailboxes.set(folders);
    let before = set.state();
    let answered = archive_both(&set).await;
    assert_eq!(refused(&set, &answered, 1), "forbidden", "{answered}");
    let destroyed = set.set(json!({ "destroy": [set.ids[&2]] })).await;
    assert_eq!(destroyed["notDestroyed"][&set.ids[&2]]["type"], "forbidden");
    assert_eq!(set.written(), ["SELECT \"INBOX\"", "SELECT \"INBOX\""]);
    untouched(&set, before).await;
}

#[tokio::test]
async fn a_host_out_of_reach_answers_per_object_and_writes_nothing() {
    let mut set = over(Behavior::default(), true).await;
    set.rig.link = Link::new(TestConnector::Refusing);
    let before = set.state();
    let answered = archive_both(&set).await;
    assert_eq!(
        refused(&set, &answered, 2),
        "serverUnavailable",
        "{answered}"
    );
    let destroyed = set.set(json!({ "destroy": [set.ids[&1]] })).await;
    assert_eq!(
        destroyed["notDestroyed"][&set.ids[&1]]["type"],
        "serverUnavailable"
    );
    assert!(set.written().is_empty());
    untouched(&set, before).await;
}
