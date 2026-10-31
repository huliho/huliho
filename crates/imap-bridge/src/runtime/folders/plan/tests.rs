// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Every object of a `Mailbox/set` against the rows: abuse cases first,
//! each refusal with nothing planned for the server (RFC 8620 section
//! 5.3, RFC 8621 section 2.5).

mod references;
mod roles;

use std::collections::{HashMap, HashSet};

use super::*;
use crate::runtime::folders::step::sort_order_of;
use crate::runtime::folders::{Create, Nullable, Update};
use crate::session::Moving;
use crate::store::{Counts, MailboxFacts};

/// A server that offers MOVE and UIDPLUS.
const BOTH: Moving = Moving {
    move_command: true,
    uidplus: true,
};

fn row(id: &str, imap_name: &str, parent: Option<&str>, role: Option<&str>) -> MailboxRow {
    MailboxRow {
        id: MailboxId::from(id.to_owned()),
        parent_id: parent.map(|parent| MailboxId::from(parent.to_owned())),
        facts: MailboxFacts::folder(imap_name, role),
    }
}

/// INBOX, Sent, Archive, `Work` with `Q3` under it and one email,
/// `Locked` held as `lrs` and `Lists`, which nothing selects.
fn dovecot() -> MailboxSnapshot {
    let mut work = row("mwork", "Work", None, None);
    work.facts.total_emails = 1;
    let mut lists = row("mlists", "Lists", None, None);
    lists.facts.selectable = false;
    MailboxSnapshot {
        state: 7,
        rows: vec![
            row("minbox", "INBOX", None, Some("inbox")),
            row("msent", "Sent", None, Some("sent")),
            row("marchive", "Archive", None, Some("archive")),
            work,
            row("mq3", "Work/Q3", Some("mwork"), None),
            row("mlocked", "Locked", None, None),
            lists,
        ],
        counts: HashMap::new(),
        done: HashSet::new(),
        gmail: false,
        moving: BOTH,
        rights: HashMap::from([(MailboxId::from("mlocked".to_owned()), "lrs".to_owned())]),
    }
}

fn create(creation_id: &str, name: &str, parent: Option<&str>) -> Create {
    Create {
        creation_id: creation_id.to_owned(),
        name: name.to_owned(),
        parent: parent.map(str::to_owned),
        role: None,
        subscribed: None,
    }
}

fn update(id: &str) -> Update {
    Update {
        id: id.to_owned(),
        name: None,
        parent: None,
        role: None,
        subscribed: None,
    }
}

/// The refusal of each object by its place; the places planned.
fn outcome(snapshot: &MailboxSnapshot, ask: &Ask) -> (Vec<(usize, Refusal)>, Vec<usize>) {
    let planned = planned(snapshot, ask, &CreatedIds::new());
    let mut refused = planned.refused;
    refused.sort_by_key(|(index, _)| *index);
    let steps = planned
        .steps
        .iter()
        .map(|step| match step {
            Step::Create { index, .. }
            | Step::Update { index, .. }
            | Step::Destroy { index, .. } => *index,
        })
        .collect();
    (refused, steps)
}

fn name() -> Refusal {
    Refusal::InvalidProperties(vec!["name"])
}

fn parent() -> Refusal {
    Refusal::InvalidProperties(vec!["parentId"])
}

#[test]
fn a_name_that_cannot_be_sent_is_refused_with_nothing_planned_rfc3501_5_1() {
    let long = "x".repeat(256);
    let typed = [
        "",
        " lead",
        "trail ",
        "line\r\nbreak",
        "tab\t",
        long.as_str(),
    ];
    let creates = typed
        .iter()
        .enumerate()
        .map(|(index, typed)| create(&index.to_string(), typed, None))
        .collect();
    let ask = Ask {
        creates,
        ..Ask::default()
    };
    let (refused, steps) = outcome(&dovecot(), &ask);
    assert_eq!(refused.len(), 6);
    assert!(refused.iter().all(|(_, refusal)| *refusal == name()));
    assert_eq!(steps, Vec::<usize>::new());
}

#[test]
fn a_name_its_parent_holds_already_is_refused_in_any_normal_form_rfc8621_2() {
    let ask = Ask {
        creates: vec![
            create("a", "Q3", Some("mwork")),
            create("b", "inbox", None),
            create("c", "Caf\u{e9}", None),
            create("d", "Cafe\u{301}", None),
            create("e", "Q3", None),
        ],
        ..Ask::default()
    };
    let (refused, steps) = outcome(&dovecot(), &ask);
    assert_eq!(refused, [(0, name()), (1, name()), (3, name())]);
    assert_eq!(steps, [2, 4]);
}

#[test]
fn a_parent_must_exist_take_a_child_and_stay_rfc8620_5_3() {
    let ask = Ask {
        creates: vec![
            create("a", "One", Some("mnope")),
            create("b", "Two", Some("#nope")),
            create("c", "Three", Some("mlocked")),
            create("d", "Four", Some("mq3")),
            create("e", "Five", Some("mlists")),
        ],
        destroys: vec!["mq3".to_owned()],
        ..Ask::default()
    };
    let (refused, steps) = outcome(&dovecot(), &ask);
    let expected = [
        (0, parent()),
        (1, parent()),
        (2, Refusal::Forbidden),
        (5, Refusal::MailboxHasChild),
    ];
    assert_eq!(refused, expected);
    assert_eq!(steps, [3, 4]);
}

#[test]
fn a_destroy_needs_the_right_no_child_and_no_mail_unless_the_call_removes_it_rfc8621_2_5() {
    let snapshot = dovecot();
    let refusals = |destroys: &[&str], remove_emails: bool| {
        let ask = Ask {
            destroys: destroys.iter().map(|id| (*id).to_owned()).collect(),
            remove_emails,
            ..Ask::default()
        };
        outcome(&snapshot, &ask)
    };
    let (refused, steps) = refusals(&["minbox", "msent", "mlocked", "mwork", "mnope"], false);
    let expected = [
        (0, Refusal::Forbidden),
        (1, Refusal::Forbidden),
        (2, Refusal::Forbidden),
        (3, Refusal::MailboxHasChild),
        (4, Refusal::NotFound),
    ];
    assert_eq!(refused, expected);
    assert_eq!(steps, Vec::<usize>::new());
    let (refused, steps) = refusals(&["mq3", "mwork"], false);
    assert_eq!(refused, [(1, Refusal::MailboxHasEmail)]);
    assert_eq!(steps, [0]);
    let (refused, steps) = refusals(&["mq3", "mwork"], true);
    assert_eq!((refused, steps), (Vec::new(), vec![0, 1]));
}

/// Two creates that name each other as parent never have a row to go
/// under and fail on it.
#[test]
fn a_create_runs_after_the_create_its_parent_names_whatever_the_order_written_rfc8620_5_3() {
    let ask = Ask {
        creates: vec![
            create("c", "Kid", Some("#p")),
            create("p", "Top", None),
            create("x", "Loop", Some("#y")),
            create("y", "Pool", Some("#x")),
        ],
        ..Ask::default()
    };
    let (refused, steps) = outcome(&dovecot(), &ask);
    assert_eq!(refused, [(2, parent()), (3, parent())]);
    assert_eq!(steps, [1, 0]);
}

#[test]
fn a_child_made_in_the_call_keeps_its_parent_from_a_destroy_rfc8621_2_5() {
    let ask = Ask {
        creates: vec![create("a", "New", Some("mq3"))],
        destroys: vec!["mq3".to_owned()],
        ..Ask::default()
    };
    let (refused, steps) = outcome(&dovecot(), &ask);
    assert_eq!(refused, [(1, Refusal::MailboxHasChild)]);
    assert_eq!(steps, [0]);
}

#[test]
fn the_mail_of_a_done_folder_is_counted_from_the_memberships_rfc8621_2() {
    let mut snapshot = dovecot();
    let work = MailboxId::from("mwork".to_owned());
    snapshot.rows[3].facts.total_emails = 0;
    snapshot.done.insert(work.clone());
    let counts = Counts {
        synced_emails: 2,
        ..Counts::default()
    };
    snapshot.counts.insert(work, counts);
    let ask = Ask {
        destroys: vec!["mq3".to_owned(), "mwork".to_owned()],
        ..Ask::default()
    };
    let (refused, _) = outcome(&snapshot, &ask);
    assert_eq!(refused, [(1, Refusal::MailboxHasEmail)]);
}

#[test]
fn inbox_and_a_role_stay_in_the_tree_and_inbox_keeps_its_name_rfc3501_6_3_5() {
    let rename_inbox = Update {
        name: Some("Post".to_owned()),
        ..update("minbox")
    };
    let hide_sent = Update {
        subscribed: Some(false),
        ..update("msent")
    };
    let rename_sent = Update {
        name: Some("Verzonden".to_owned()),
        ..update("msent")
    };
    let ask = Ask {
        updates: vec![rename_inbox, hide_sent],
        ..Ask::default()
    };
    let (refused, _) = outcome(&dovecot(), &ask);
    assert_eq!(refused, [(0, Refusal::Forbidden), (1, Refusal::Forbidden)]);
    let ask = Ask {
        updates: vec![rename_sent],
        ..Ask::default()
    };
    assert_eq!(outcome(&dovecot(), &ask), (Vec::new(), vec![0]));
}

#[test]
fn a_move_under_itself_or_a_name_taken_there_is_refused_rfc8621_2() {
    let under_itself = Update {
        parent: Some(Nullable::Value("mq3".to_owned())),
        ..update("mwork")
    };
    let onto_a_sibling = Update {
        name: Some("Archive".to_owned()),
        ..update("mwork")
    };
    let unchanged = Update {
        name: Some("Work".to_owned()),
        ..update("mwork")
    };
    for (index, (rename, refusal)) in [(under_itself, parent()), (onto_a_sibling, name())]
        .into_iter()
        .enumerate()
    {
        let ask = Ask {
            updates: vec![rename],
            ..Ask::default()
        };
        assert_eq!(outcome(&dovecot(), &ask).0, [(0, refusal)], "{index}");
    }
    let ask = Ask {
        updates: vec![unchanged],
        ..Ask::default()
    };
    let planned = planned(&dovecot(), &ask, &CreatedIds::new());
    assert_eq!(planned.refused, []);
    assert!(matches!(
        planned.steps.as_slice(),
        [Step::Update { rename: None, .. }]
    ));
}

#[test]
fn an_object_named_twice_or_missing_answers_as_rfc8620_5_3_has_it() {
    let ask = Ask {
        updates: vec![update("mnope"), update("mq3")],
        destroys: vec!["mq3".to_owned()],
        ..Ask::default()
    };
    let (refused, steps) = outcome(&dovecot(), &ask);
    assert_eq!(refused, [(0, Refusal::NotFound), (1, Refusal::WillDestroy)]);
    assert_eq!(steps, [2]);
}
