// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The rights of a mailbox from the letters, the server's commands and
//! the rules that hold whatever the letters say (RFC 8621 section 2,
//! RFC 4314 sections 2.1 and 4).

use std::collections::{HashMap, HashSet};

use async_imap::imap_proto::AclRight;

use super::*;
use crate::session::Moving;
use crate::store::{MailboxFacts, MailboxId};

/// A server that offers MOVE and UIDPLUS.
const BOTH: Moving = Moving {
    move_command: true,
    uidplus: true,
};

fn row(imap_name: &str, role: Option<&str>) -> MailboxRow {
    MailboxRow {
        id: MailboxId::generate(),
        parent_id: None,
        facts: MailboxFacts::folder(imap_name, role),
    }
}

/// A snapshot that holds these letters for the row, if any.
fn snapshot(
    row: &MailboxRow,
    letters: Option<&str>,
    moving: Moving,
    gmail: bool,
) -> MailboxSnapshot {
    let rights = letters
        .map(|letters| HashMap::from([(row.id.clone(), letters.to_owned())]))
        .unwrap_or_default();
    MailboxSnapshot {
        state: 0,
        rows: vec![row.clone()],
        counts: HashMap::new(),
        done: HashSet::new(),
        gmail,
        moving,
        rights,
    }
}

fn held(rights: MailboxRights) -> Vec<&'static str> {
    Right::ALL
        .iter()
        .filter(|right| rights.may(**right))
        .map(|right| right.property())
        .collect()
}

#[test]
fn without_acl_every_right_the_commands_allow_is_held_rfc8621_2() {
    let work = row("Work", None);
    let rights = of(&work, &snapshot(&work, None, BOTH, false));
    assert_eq!(
        held(rights),
        [
            "mayReadItems",
            "mayAddItems",
            "mayRemoveItems",
            "maySetSeen",
            "maySetKeywords",
            "mayCreateChild",
            "mayRename",
            "mayDelete"
        ]
    );
    let still = of(&work, &snapshot(&work, None, Moving::default(), false));
    assert!(!still.may(Right::AddItems) && !still.may(Right::RemoveItems));
}

#[test]
fn the_letters_narrow_every_right_they_cover_rfc4314_4() {
    let locked = row("Locked", None);
    let rights = of(&locked, &snapshot(&locked, Some("lrs"), BOTH, false));
    assert_eq!(held(rights), ["mayReadItems", "maySetSeen"]);
    let owner = of(
        &locked,
        &snapshot(&locked, Some("acdeiklnprstwx"), BOTH, false),
    );
    assert_eq!(held(owner).len(), Right::ALL.len() - 1);
    let half = of(&locked, &snapshot(&locked, Some("lrite"), BOTH, false));
    assert!(half.may(Right::AddItems) && half.may(Right::RemoveItems));
    let no_expunge = of(&locked, &snapshot(&locked, Some("lrit"), BOTH, false));
    assert!(!no_expunge.may(Right::RemoveItems));
    let none = of(&locked, &snapshot(&locked, Some(""), BOTH, false));
    assert_eq!(held(none), Vec::<&str>::new());
}

#[test]
fn inbox_is_never_renamed_or_deleted_and_a_role_mailbox_never_deleted_rfc3501_6_3_4() {
    let inbox = row("inbox", Some("inbox"));
    let rights = of(&inbox, &snapshot(&inbox, None, BOTH, false));
    assert!(!rights.may(Right::Rename) && !rights.may(Right::Delete));
    assert!(rights.may(Right::CreateChild));
    let sent = row("Sent", Some("sent"));
    let rights = of(&sent, &snapshot(&sent, None, BOTH, false));
    assert!(rights.may(Right::Rename) && !rights.may(Right::Delete));
}

#[test]
fn a_mailbox_nothing_can_select_takes_a_child_by_its_letters_and_nothing_else_rfc3501_7_2_2() {
    let mut parent = row("Lists", None);
    parent.facts.selectable = false;
    let open = of(&parent, &snapshot(&parent, None, BOTH, false));
    assert_eq!(held(open), ["mayCreateChild"]);
    let shut = of(&parent, &snapshot(&parent, Some("lr"), BOTH, false));
    assert_eq!(held(shut), Vec::<&str>::new());
}

#[test]
fn what_gmail_keeps_for_itself_takes_no_rename_no_delete_and_no_child() {
    let all_mail = row("[Gmail]/All Mail", Some("archive"));
    let rights = of(&all_mail, &snapshot(&all_mail, None, BOTH, true));
    assert!(!rights.may(Right::Rename) && !rights.may(Right::CreateChild));
    assert!(rights.may(Right::AddItems));
    let mut sent = row("[Gmail]/Sent Mail", Some("sent"));
    sent.facts.store = false;
    sent.facts.gmail_label = Some("\\Sent".to_owned());
    let rights = of(&sent, &snapshot(&sent, None, BOTH, true));
    assert!(!rights.may(Right::Rename) && !rights.may(Right::CreateChild));
    let mut root = row("[Gmail]", None);
    (root.facts.selectable, root.facts.store) = (false, false);
    let rights = of(&root, &snapshot(&root, None, BOTH, true));
    assert_eq!(held(rights), Vec::<&str>::new());
    let mut label = row("Work", None);
    label.facts.store = false;
    label.facts.gmail_label = Some("Work".to_owned());
    let rights = of(&label, &snapshot(&label, None, BOTH, true));
    assert!(rights.may(Right::Rename) && rights.may(Right::Delete));
    assert!(rights.may(Right::CreateChild));
}

#[test]
fn a_gmail_store_moves_mail_with_move_alone() {
    let mut store = row("[Gmail]/Spam", Some("junk"));
    store.facts.store = true;
    let uidplus = Moving {
        move_command: false,
        uidplus: true,
    };
    let rights = of(&store, &snapshot(&store, None, uidplus, true));
    assert!(!rights.may(Right::AddItems) && !rights.may(Right::RemoveItems));
    let folder = of(&store, &snapshot(&store, None, uidplus, false));
    assert!(folder.may(Right::AddItems) && folder.may(Right::RemoveItems));
}

#[test]
fn the_letters_of_a_server_without_rights_capability_never_grant_more_rfc4314_2_1_1() {
    let named = |letters: &[char]| {
        let found: Vec<AclRight> = letters
            .iter()
            .map(|letter| AclRight::from(*letter))
            .collect();
        Rights::of(&found)
    };
    assert_eq!(kept(&named(&['l', 'r', 'c']), true), "cklr");
    assert_eq!(kept(&named(&['l', 'r', 'd']), true), "delrt");
    assert_eq!(kept(&named(&['c', 'd']), true), "cdektx");
    assert_eq!(kept(&named(&['l', 'r', 'c', 'd']), false), "cdlr");
}

#[test]
fn every_right_is_what_kept_spells_for_the_full_set_rfc4314_2_1() {
    let all: Vec<AclRight> = "lrswipkxtea".chars().map(AclRight::from).collect();
    assert_eq!(kept(&Rights::of(&all), false), EVERY_RIGHT);
    let work = row("Work", None);
    let unknown = of(&work, &snapshot(&work, Some(EVERY_RIGHT), BOTH, false));
    assert_eq!(unknown, of(&work, &snapshot(&work, None, BOTH, false)));
}

#[test]
fn the_object_names_every_right_rfc8621_2() {
    let object = MailboxRights::default().object();
    let names: Vec<&String> = object.as_object().unwrap().keys().collect();
    assert_eq!(names.len(), Right::ALL.len());
    assert_eq!(object["maySubmit"], false);
}
