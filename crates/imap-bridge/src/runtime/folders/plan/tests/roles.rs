// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The roles of a `Mailbox/set` against the rows: known, free and on a
//! mailbox that holds mail, held by one mailbox that will exist and
//! never changed on a Gmail account (RFC 8621 section 2).

use super::*;
use crate::store::RoleOverride;

fn role(id: &str, role: Option<&str>) -> Update {
    Update {
        role: Some(Nullable::from(role.map(str::to_owned))),
        ..update(id)
    }
}

fn invalid_role() -> Refusal {
    Refusal::InvalidProperties(vec!["role"])
}

#[test]
fn a_role_must_be_known_free_and_on_a_mailbox_that_holds_mail_rfc8621_2() {
    let ask = Ask {
        updates: vec![
            role("mwork", Some("archive")),
            role("mlocked", Some("outbox")),
            role("mlists", Some("junk")),
            role("minbox", None),
            role("mq3", Some("inbox")),
        ],
        ..Ask::default()
    };
    let (refused, _) = outcome(&dovecot(), &ask);
    let expected = [
        (0, invalid_role()),
        (1, invalid_role()),
        (2, invalid_role()),
        (3, Refusal::Forbidden),
        (4, Refusal::Forbidden),
    ];
    assert_eq!(refused, expected);
    let moved = Ask {
        updates: vec![role("marchive", None), role("mwork", Some("archive"))],
        ..Ask::default()
    };
    let planned = planned(&dovecot(), &moved, &CreatedIds::new());
    assert_eq!(planned.refused, []);
    assert!(planned.steps.iter().all(Step::is_local));
}

/// A role is held by the rows, the creates and the updates that name
/// one of those; an update that names no mailbox holds none.
#[test]
fn a_role_counts_as_held_by_a_mailbox_alone_rfc8621_2() {
    let missing = Ask {
        updates: vec![
            role("marchive", None),
            role("mwork", Some("archive")),
            role("mnope", Some("archive")),
            role("#gone", Some("archive")),
        ],
        ..Ask::default()
    };
    let (refused, steps) = outcome(&dovecot(), &missing);
    assert_eq!(refused, [(2, Refusal::NotFound), (3, Refusal::NotFound)]);
    assert_eq!(steps, [0, 1]);
    let made = Ask {
        creates: vec![create("a", "New", None)],
        updates: vec![role("#a", Some("archive"))],
        ..Ask::default()
    };
    let (refused, steps) = outcome(&dovecot(), &made);
    assert_eq!(refused, [(1, invalid_role())]);
    assert_eq!(steps, [0]);
}

/// A create the call refuses holds no role, so an update that sets the
/// same role lands, whether the row holding it gives it up in the call
/// or none holds it; a create that passed holds its role for the
/// objects after it.
#[test]
fn a_refused_create_holds_no_role_rfc8621_2() {
    let archived = |creation_id: &str, name: &str, parent: Option<&str>| Create {
        role: Some(Nullable::Value("archive".to_owned())),
        ..create(creation_id, name, parent)
    };
    let given_up = Ask {
        creates: vec![
            archived("a", "inbox", None),
            archived("b", "New", Some("mnope")),
        ],
        updates: vec![role("marchive", None), role("mwork", Some("archive"))],
        ..Ask::default()
    };
    let (refused, steps) = outcome(&dovecot(), &given_up);
    assert_eq!(refused, [(0, name()), (1, parent())]);
    assert_eq!(steps, [2, 3]);
    let mut snapshot = dovecot();
    snapshot.rows.retain(|row| row.id.as_str() != "marchive");
    let free = Ask {
        creates: vec![archived("a", "", None)],
        updates: vec![role("mwork", Some("archive"))],
        ..Ask::default()
    };
    let (refused, steps) = outcome(&snapshot, &free);
    assert_eq!(refused, [(0, name())]);
    assert_eq!(steps, [1]);
    let twice = Ask {
        creates: vec![archived("a", "One", None), archived("b", "Two", None)],
        ..Ask::default()
    };
    let (refused, steps) = outcome(&snapshot, &twice);
    assert_eq!(refused, [(1, invalid_role())]);
    assert_eq!(steps, [0]);
}

/// A free role two landing objects name goes to an update over a create
/// and to neither of two updates: the roles of the landing updates are
/// read before any create lands.
#[test]
fn a_contested_role_goes_to_an_update_over_a_create_and_to_neither_of_two_updates_rfc8621_2() {
    let junk = |id: &str| role(id, Some("junk"));
    let two_updates = Ask {
        updates: vec![junk("mwork"), junk("mq3")],
        ..Ask::default()
    };
    let (refused, steps) = outcome(&dovecot(), &two_updates);
    assert_eq!(refused, [(0, invalid_role()), (1, invalid_role())]);
    assert_eq!(steps, Vec::<usize>::new());
    let create_and_update = Ask {
        creates: vec![Create {
            role: Some(Nullable::Value("junk".to_owned())),
            ..create("a", "Spam", None)
        }],
        updates: vec![junk("mwork")],
        ..Ask::default()
    };
    let (refused, steps) = outcome(&dovecot(), &create_and_update);
    assert_eq!(refused, [(0, invalid_role())]);
    assert_eq!(steps, [1]);
}

/// An update refused for another reason neither holds the role it names
/// nor frees the one its mailbox holds: beside the first kind a taker
/// lands, a create too; beside the second the holder keeps its role.
#[test]
fn an_update_refused_for_another_reason_neither_holds_nor_frees_a_role_rfc8621_2() {
    let locked_as_junk = Update {
        name: Some("Free".to_owned()),
        ..role("mlocked", Some("junk"))
    };
    let taken = Ask {
        updates: vec![locked_as_junk.clone(), role("mwork", Some("junk"))],
        ..Ask::default()
    };
    let (refused, steps) = outcome(&dovecot(), &taken);
    assert_eq!(refused, [(0, invalid_role())]);
    assert_eq!(steps, [1]);
    let made = Ask {
        creates: vec![Create {
            role: Some(Nullable::Value("junk".to_owned())),
            ..create("a", "Spam", None)
        }],
        updates: vec![locked_as_junk],
        ..Ask::default()
    };
    let (refused, steps) = outcome(&dovecot(), &made);
    assert_eq!(refused, [(1, invalid_role())]);
    assert_eq!(steps, [0]);
    let mut snapshot = dovecot();
    snapshot
        .rights
        .insert(MailboxId::from("msent".to_owned()), "lrs".to_owned());
    let held = Ask {
        updates: vec![
            Update {
                name: Some("Verzonden".to_owned()),
                ..role("msent", None)
            },
            role("mwork", Some("sent")),
        ],
        ..Ask::default()
    };
    let (refused, steps) = outcome(&snapshot, &held);
    assert_eq!(refused, [(0, Refusal::Forbidden), (1, invalid_role())]);
    assert_eq!(steps, Vec::<usize>::new());
}

/// The subscription follows the role the mailbox holds once the call is
/// done: the one the patch sets, else the row's.
#[test]
fn a_mailbox_is_never_unsubscribed_under_the_role_the_call_leaves_it_rfc8621_2() {
    let hidden_as_junk = Update {
        subscribed: Some(false),
        ..role("mwork", Some("junk"))
    };
    let hidden_without_role = Update {
        subscribed: Some(false),
        ..role("msent", None)
    };
    let ask = Ask {
        updates: vec![hidden_as_junk, hidden_without_role],
        ..Ask::default()
    };
    let planned = planned(&dovecot(), &ask, &CreatedIds::new());
    assert_eq!(planned.refused, [(0, Refusal::Forbidden)]);
    assert!(matches!(
        planned.steps.as_slice(),
        [Step::Update {
            index: 1,
            subscribe: Some(false),
            role: Some(RoleOverride::NoRole),
            ..
        }]
    ));
}

#[test]
fn every_role_change_of_a_gmail_account_is_refused() {
    let mut snapshot = dovecot();
    snapshot.gmail = true;
    let ask = Ask {
        updates: vec![role("mwork", Some("archive"))],
        creates: vec![
            Create {
                role: Some(Nullable::Value("junk".to_owned())),
                ..create("a", "Spam two", None)
            },
            Create {
                role: Some(Nullable::Null),
                ..create("b", "Plain", None)
            },
        ],
        ..Ask::default()
    };
    let (refused, _) = outcome(&snapshot, &ask);
    let forbidden = Refusal::Forbidden;
    assert_eq!(
        refused,
        [
            (0, forbidden.clone()),
            (1, forbidden.clone()),
            (2, forbidden)
        ]
    );
}

/// A create with `role: null` plans an override of no role, so the next
/// pass hands the folder none; one without a role plans no override.
#[test]
fn a_create_with_a_null_role_plans_an_override_of_no_role_rfc8621_2() {
    let ask = Ask {
        creates: vec![
            Create {
                role: Some(Nullable::Null),
                ..create("a", "Archives", None)
            },
            create("b", "Spam", None),
        ],
        ..Ask::default()
    };
    let planned = planned(&dovecot(), &ask, &CreatedIds::new());
    assert_eq!(planned.refused, []);
    let roles: Vec<&Option<RoleOverride>> = planned
        .steps
        .iter()
        .filter_map(|step| match step {
            Step::Create { role, .. } => Some(role),
            Step::Update { .. } | Step::Destroy { .. } => None,
        })
        .collect();
    assert_eq!(roles, [&Some(RoleOverride::NoRole), &None]);
}

#[test]
fn the_sort_order_follows_the_role_the_user_set() {
    assert_eq!(sort_order_of(&RoleOverride::Role("archive".to_owned())), 3);
    assert_eq!(sort_order_of(&RoleOverride::NoRole), 6);
}
