// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! `Mailbox/set` where the server says no or narrows the rights: a
//! refused command answers `serverFail` and costs the conversation, a
//! refused RENAME is followed by a pass, a SUBSCRIBE refused after a
//! CREATE leaves the folder created and unsubscribed, the rights follow
//! MYRIGHTS one mailbox at a time or through LIST with a mailbox the
//! server cannot look up holding every right, a DELETE the server keeps
//! as a name comes back as `\Noselect`, a renamed folder that renumbers
//! keeps its emails, an INBOX listed without a delimiter leaves the top
//! level under the one the other lines carry and a listing without any
//! delimiter refuses a parent.

use huliho_imap_bridge::store::{ChangeKind, ObjectType};
use huliho_imap_bridge::testing::{
    Extension, Flat, Folder, Mailboxes, Message, MyRights, ParentDelete,
};
use serde_json::json;

use crate::folders_rig::{
    id_of, mailbox, mailboxes_changed, refused, rows, sent, set, started, state,
};
use crate::sync_rig::{ACCOUNT, Rig};

/// The sign-ins the server saw.
fn logins(rig: &Rig) -> usize {
    let lines = rig.fake.lines();
    lines.iter().filter(|line| line.contains(" LOGIN ")).count()
}

/// The MYRIGHTS commands the server received.
fn myrights(rig: &Rig) -> usize {
    let sent = sent(rig);
    sent.iter()
        .filter(|command| command.starts_with("MYRIGHTS "))
        .count()
}

/// A Dovecot with a folder `Work`, behind the ACL extension when asked,
/// with a folder `Locked` the user may read and mark seen alone.
fn narrowed(list_rights: bool) -> Mailboxes {
    let mut mailboxes = Mailboxes::dovecot();
    mailboxes.extensions.insert(Extension::Acl);
    if list_rights {
        mailboxes.extensions.insert(Extension::ListMyrights);
    }
    mailboxes.push(Folder::new("Work"));
    mailboxes.push(Folder {
        rights: MyRights::Letters("lrs"),
        ..Folder::new("Locked")
    });
    mailboxes
}

/// A NO costs the call its object as `serverFail`, never the server's
/// words; the next use signs in afresh. After a refused RENAME a pass
/// on a fresh connection shows what stands before the answer.
#[tokio::test]
async fn a_refused_command_answers_server_fail_and_a_refused_rename_is_followed_by_a_pass() {
    let mut mailboxes = Mailboxes::dovecot();
    mailboxes.push(Folder::new("Work"));
    mailboxes.behavior.folder_refusal = Some("ALREADYEXISTS");
    let rig = started(mailboxes, false).await;
    let work = id_of(&rig, "Work");
    let answer = set(&rig, json!({ "create": { "k": { "name": "New" } } })).await;
    assert_eq!(refused(&answer, "notCreated", "k"), "serverFail");
    assert_eq!(answer["notCreated"]["k"].as_object().unwrap().len(), 1);
    let before = logins(&rig);
    rig.fake.script().mailboxes.push(Folder::new("Elsewhere"));
    let answer = set(&rig, json!({ "update": { (&work): { "name": "Job" } } })).await;
    assert_eq!(refused(&answer, "notUpdated", &work), "serverFail");
    assert_eq!(logins(&rig), before + 2, "the call and the pass after it");
    assert!(
        rows(&rig)
            .iter()
            .any(|row| row.facts.imap_name == "Elsewhere")
    );
    assert_eq!(id_of(&rig, "Work"), work);
}

/// A SUBSCRIBE the server refuses after a CREATE it took leaves the
/// folder created and unsubscribed: the create answers under `created`
/// with `isSubscribed` false and the row stands so. The NO costs the
/// conversation, so the next use signs in afresh; a SUBSCRIBE refused on
/// an update of its own answers `serverFail` (RFC 3501 section 6.3.6).
#[tokio::test]
async fn a_subscribe_refused_after_a_create_leaves_the_folder_created_and_unsubscribed_rfc3501_6_3_6()
 {
    let mut mailboxes = Mailboxes::dovecot();
    mailboxes.behavior.subscribe_refusal = Some("NOPERM");
    let rig = started(mailboxes, false).await;
    let before = state(&rig);
    let answer = set(&rig, json!({ "create": { "k": { "name": "New" } } })).await;
    let id = answer["created"]["k"]["id"].as_str().unwrap().to_owned();
    assert_eq!(answer["notCreated"], json!(null), "{answer}");
    assert_eq!(answer["created"]["k"]["isSubscribed"], false);
    assert_eq!(answer["newState"], (before + 1).to_string());
    assert_eq!(sent(&rig), ["CREATE \"New\"", "SUBSCRIBE \"New\""]);
    assert_eq!(id_of(&rig, "New"), id);
    assert!(!rig.folder("New").facts.subscribed);
    assert_eq!(mailbox(&rig, &id).await["isSubscribed"], false);
    let signed_in = logins(&rig);
    let shown = json!({ "update": { (&id): { "isSubscribed": true } } });
    let answer = set(&rig, shown).await;
    assert_eq!(refused(&answer, "notUpdated", &id), "serverFail");
    assert_eq!(logins(&rig), signed_in + 1, "the next use signs in afresh");
    assert_eq!(
        sent(&rig).last().map(String::as_str),
        Some("SUBSCRIBE \"New\"")
    );
    rig.pass().await.unwrap();
    assert_eq!(id_of(&rig, "New"), id);
    assert!(!rig.folder("New").facts.subscribed);
}

/// Where ACL is advertised the rights follow the letters: MYRIGHTS per
/// mailbox on the first pass of a connection and for a mailbox new to
/// the rows (RFC 4314 section 3.5). A folder the user holds `lrs` on
/// refuses a child, a rename and a destroy with nothing sent.
#[tokio::test]
async fn the_rights_follow_myrights_and_a_narrowed_folder_refuses_with_nothing_sent_rfc4314_4() {
    let rig = started(narrowed(false), false).await;
    let locked = id_of(&rig, "Locked");
    let rights = mailbox(&rig, &locked).await["myRights"].clone();
    for (right, held) in [
        ("mayReadItems", true),
        ("maySetSeen", true),
        ("maySetKeywords", false),
        ("mayAddItems", false),
        ("mayRemoveItems", false),
        ("mayCreateChild", false),
        ("mayRename", false),
        ("mayDelete", false),
    ] {
        assert_eq!(rights[right], held, "{right}");
    }
    let work = mailbox(&rig, &id_of(&rig, "Work")).await;
    assert_eq!(work["myRights"]["mayDelete"], true);
    let asked = myrights(&rig);
    let selectable = rows(&rig).iter().filter(|row| row.facts.selectable).count();
    assert_eq!(asked, selectable, "every selectable mailbox once");
    let call = json!({
        "create": { "k": { "name": "Below", "parentId": locked } },
        "update": { (&locked): { "name": "Free" } },
    });
    let answer = set(&rig, call).await;
    assert_eq!(refused(&answer, "notCreated", "k"), "forbidden");
    assert_eq!(refused(&answer, "notUpdated", &locked), "forbidden");
    let answer = set(&rig, json!({ "destroy": [&locked] })).await;
    assert_eq!(refused(&answer, "notDestroyed", &locked), "forbidden");
    assert_eq!(myrights(&rig), asked, "nothing sent");
    let changes = json!({ "accountId": ACCOUNT, "sinceState": "0" });
    rig.call(&json!(["Mailbox/changes", changes, "c1"])).await;
    assert_eq!(myrights(&rig), asked * 2, "the conversation's first pass");
    rig.call(&json!(["Mailbox/changes", changes, "c1"])).await;
    assert_eq!(
        myrights(&rig),
        asked * 2,
        "a later pass reads no letters it holds"
    );
    rig.fake.script().mailboxes.push(Folder::new("Later"));
    rig.call(&json!(["Mailbox/changes", changes, "c1"])).await;
    assert_eq!(
        myrights(&rig),
        asked * 2 + 1,
        "one for the mailbox new to the rows"
    );
}

/// Where LIST-MYRIGHTS stands beside ACL the letters ride the listing
/// and no MYRIGHTS goes out (RFC 8440).
#[tokio::test]
async fn the_rights_ride_the_listing_where_list_myrights_is_advertised_rfc8440() {
    let rig = started(narrowed(true), false).await;
    assert_eq!(myrights(&rig), 0);
    let lines = rig.fake.lines();
    assert!(
        lines.iter().any(|line| line.contains("MYRIGHTS)")),
        "{lines:?}"
    );
    let locked = mailbox(&rig, &id_of(&rig, "Locked")).await;
    assert_eq!(locked["myRights"]["mayCreateChild"], false);
    let inbox = mailbox(&rig, &id_of(&rig, "INBOX")).await;
    assert_eq!(inbox["myRights"]["mayCreateChild"], true);
}

/// A folder whose MYRIGHTS the server answers NO holds every right like
/// one without letters; its row counts as asked, so the next pass of
/// the connection sends no MYRIGHTS for it (RFC 4314 section 3.5).
#[tokio::test]
async fn a_myrights_answered_no_reads_as_every_right_and_is_asked_once_rfc4314_3_5() {
    let mailboxes = narrowed(false);
    mailboxes.push(Folder {
        rights: MyRights::Refused,
        ..Folder::new("Unsure")
    });
    let rig = started(mailboxes, false).await;
    let unsure = id_of(&rig, "Unsure");
    let rights = mailbox(&rig, &unsure).await["myRights"].clone();
    for right in [
        "mayReadItems",
        "mayAddItems",
        "mayCreateChild",
        "mayRename",
        "mayDelete",
    ] {
        assert_eq!(rights[right], true, "{right}");
    }
    let locked = mailbox(&rig, &id_of(&rig, "Locked")).await;
    assert_eq!(locked["myRights"]["mayRename"], false);
    let asked = myrights(&rig);
    let changes = json!({ "accountId": ACCOUNT, "sinceState": "0" });
    rig.call(&json!(["Mailbox/changes", changes, "c1"])).await;
    rig.call(&json!(["Mailbox/changes", changes, "c1"])).await;
    assert_eq!(myrights(&rig), asked * 2, "once per connection");
    assert_eq!(mailbox(&rig, &unsure).await["myRights"]["mayDelete"], true);
}

/// Where the rights ride the listing a mailbox the answer carries no
/// MYRIGHTS line for holds every right; one that held letters loses
/// them once its line stays out (RFC 8440 section 3).
#[tokio::test]
async fn a_mailbox_the_listing_carries_no_rights_for_holds_every_right_rfc8440_3() {
    let mailboxes = narrowed(true);
    mailboxes.push(Folder {
        rights: MyRights::Refused,
        ..Folder::new("Unsure")
    });
    let rig = started(mailboxes, false).await;
    let unsure = mailbox(&rig, &id_of(&rig, "Unsure")).await;
    assert_eq!(unsure["myRights"]["mayDelete"], true);
    let locked = id_of(&rig, "Locked");
    assert_eq!(
        mailbox(&rig, &locked).await["myRights"]["mayCreateChild"],
        false
    );
    let model = &rig.fake.script().mailboxes;
    let mut folders = model.folders();
    for folder in folders.iter_mut().filter(|folder| folder.name == "Locked") {
        folder.rights = MyRights::Refused;
    }
    model.set(folders);
    let before = state(&rig);
    rig.pass().await.unwrap();
    assert_eq!(
        mailboxes_changed(&rig, before),
        [(locked.clone(), ChangeKind::Updated)]
    );
    assert_eq!(
        mailbox(&rig, &locked).await["myRights"]["mayCreateChild"],
        true
    );
    assert_eq!(myrights(&rig), 0);
}

/// `ifInState` naming another state ends the call before anything is
/// sent (RFC 8620 section 5.3).
#[tokio::test]
async fn a_set_against_another_state_answers_state_mismatch_rfc8620_5_3() {
    let rig = started(Mailboxes::dovecot(), false).await;
    let arguments = json!({
        "accountId": ACCOUNT,
        "ifInState": "999",
        "create": { "k": { "name": "New" } },
    });
    let answer = rig.calls(&[json!(["Mailbox/set", arguments, "c1"])]).await;
    assert_eq!(answer["methodResponses"][0][0], "error");
    assert_eq!(answer["methodResponses"][0][1]["type"], "stateMismatch");
    assert_eq!(sent(&rig), Vec::<String>::new());
}

/// Dovecot ends a connection whose open mailbox a DELETE takes, so the
/// conversation opens INBOX ahead of the first RENAME or DELETE: a
/// folder a write just selected is destroyed and its subscription goes.
#[tokio::test]
async fn a_folder_the_conversation_has_open_is_destroyed_without_losing_the_session() {
    let mailboxes = Mailboxes::dovecot();
    mailboxes.push(Folder::new("Work").with_mail(vec![Message::new(1)]));
    let rig = started(mailboxes, false).await;
    rig.sync("Work").await;
    let email = rig.created(0).remove(0);
    let flag =
        json!({ "accountId": ACCOUNT, "update": { (&email): { "keywords/$flagged": true } } });
    let flagged = rig.call(&json!(["Email/set", flag, "c1"])).await;
    assert!(flagged["updated"].get(&email).is_some(), "{flagged}");
    let work = id_of(&rig, "Work");
    let destroy = json!({ "destroy": [&work], "onDestroyRemoveEmails": true });
    let before = logins(&rig);
    let answer = set(&rig, destroy).await;
    assert_eq!(answer["destroyed"], json!([work]), "{answer}");
    assert_eq!(sent(&rig), ["DELETE \"Work\"", "UNSUBSCRIBE \"Work\""]);
    assert_eq!(logins(&rig), before, "the conversation stands");
}

/// A DELETE of a folder another client gave a child since the last pass
/// may leave its name as `\Noselect` (RFC 3501 section 6.3.4): the row
/// leaves with the call and the next pass lists the name again.
#[tokio::test]
async fn a_delete_the_server_keeps_as_a_name_comes_back_as_noselect_rfc3501_6_3_4() {
    let mut mailboxes = Mailboxes::dovecot();
    mailboxes.push(Folder::new("Work"));
    mailboxes.behavior.parent_delete = ParentDelete::KeepsName;
    let rig = started(mailboxes, false).await;
    let work = id_of(&rig, "Work");
    rig.fake.script().mailboxes.push(Folder::new("Work/New"));
    let answer = set(&rig, json!({ "destroy": [&work] })).await;
    assert_eq!(answer["destroyed"], json!([work]), "{answer}");
    rig.pass().await.unwrap();
    let kept = rig.folder("Work");
    assert!(!kept.facts.selectable);
    assert_ne!(kept.id.to_string(), work);
    assert_eq!(
        mailbox(&rig, kept.id.as_str()).await["myRights"]["mayDelete"],
        false
    );
}

/// A rename promises nothing about UIDVALIDITY (RFC 3501 section 6.3.5):
/// when the renamed folder comes back renumbered its emails match again
/// under their new UIDs and keep their ids.
#[tokio::test]
async fn a_renamed_folder_that_renumbers_keeps_its_emails_rfc3501_2_3_1_1() {
    let mailboxes = Mailboxes::dovecot();
    mailboxes.push(Folder::new("Work").with_mail(vec![Message::new(1), Message::new(2)]));
    let rig = started(mailboxes, false).await;
    rig.sync("Work").await;
    let held = rig.created(0);
    let work = id_of(&rig, "Work");
    let answer = set(&rig, json!({ "update": { (&work): { "name": "Job" } } })).await;
    assert_eq!(answer["updated"][&work], json!(null), "{answer}");
    let model = &rig.fake.script().mailboxes;
    let mut folders = model.folders();
    for folder in folders.iter_mut().filter(|folder| folder.name == "Job") {
        folder.uid_validity += 1;
    }
    model.set(folders);
    let state = rig.cache.store.state(&rig.cache.key).unwrap();
    rig.pass().await.unwrap();
    rig.sync("Job").await;
    assert_eq!(id_of(&rig, "Job"), work);
    let created = rig.changes(ObjectType::Email, state);
    assert!(
        created.iter().all(|(id, _)| held.contains(id)),
        "{created:?}"
    );
}

/// A server may list INBOX with a NIL delimiter and the rest under one
/// (RFC 9051 section 7.3.1): a top-level name holding that delimiter is
/// refused with nothing sent past the listing and one without it goes
/// out.
#[tokio::test]
async fn a_top_level_name_follows_the_delimiter_of_the_other_lines_where_inbox_lists_none_rfc9051_7_3_1()
 {
    let mut mailboxes = Mailboxes::dovecot();
    mailboxes.flat = Flat::Inbox;
    let rig = started(mailboxes, false).await;
    let answer = set(&rig, json!({ "create": { "k": { "name": "a/b" } } })).await;
    assert_eq!(refused(&answer, "notCreated", "k"), "invalidProperties");
    assert_eq!(answer["notCreated"]["k"]["properties"], json!(["name"]));
    assert_eq!(sent(&rig), Vec::<String>::new());
    let answer = set(&rig, json!({ "create": { "k": { "name": "Plain" } } })).await;
    assert!(answer["created"]["k"]["id"].is_string(), "{answer}");
    assert_eq!(sent(&rig), ["CREATE \"Plain\"", "SUBSCRIBE \"Plain\""]);
}

/// A server without hierarchy lists NIL on every line (RFC 9051 section
/// 7.3.1): a create under a parent is refused on `parentId` once the
/// listing is read and nothing is sent after it; a top-level create
/// goes out.
#[tokio::test]
async fn a_create_under_a_parent_is_refused_where_every_line_lists_nil_rfc9051_7_3_1() {
    let mut mailboxes = Mailboxes::dovecot();
    mailboxes.flat = Flat::Everywhere;
    mailboxes.push(Folder::new("Work"));
    let rig = started(mailboxes, false).await;
    let work = id_of(&rig, "Work");
    let under = json!({ "create": { "k": { "name": "Q3", "parentId": work } } });
    let answer = set(&rig, under).await;
    assert_eq!(refused(&answer, "notCreated", "k"), "invalidProperties");
    assert_eq!(answer["notCreated"]["k"]["properties"], json!(["parentId"]));
    assert_eq!(sent(&rig), Vec::<String>::new());
    let last = rig.fake.lines().pop().unwrap();
    assert!(last.contains(" LIST "), "{last}");
    let answer = set(&rig, json!({ "create": { "k": { "name": "Plain" } } })).await;
    assert!(answer["created"]["k"]["id"].is_string(), "{answer}");
    assert_eq!(sent(&rig), ["CREATE \"Plain\"", "SUBSCRIBE \"Plain\""]);
}
