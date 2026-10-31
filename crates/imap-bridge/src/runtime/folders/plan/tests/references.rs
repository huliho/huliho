// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A creation id inside the plan: a create of the call, a mailbox an
//! earlier call made and one that stands for nothing (RFC 8620 section
//! 5.3).

use super::*;

/// A `#` names a create of the call first, then a mailbox an earlier
/// call made; a create of the call is a row for the updates and
/// destroys after it.
#[test]
fn a_creation_id_names_a_create_of_the_call_then_one_of_an_earlier_call_rfc8620_5_3() {
    let earlier = CreatedIds::from([("w".to_owned(), "mwork".to_owned())]);
    let ask = Ask {
        creates: vec![
            create("a", "Own", None),
            create("b", "Kid", Some("#a")),
            create("c", "Under", Some("#w")),
            create("d", "Lost", Some("#gone")),
        ],
        updates: vec![
            Update {
                name: Some("Mine".to_owned()),
                ..update("#a")
            },
            Update {
                subscribed: Some(false),
                ..update("#w")
            },
            update("#gone"),
        ],
        destroys: vec!["#b".to_owned()],
        ..Ask::default()
    };
    let planned = planned(&dovecot(), &ask, &earlier);
    let mut refused = planned.refused;
    refused.sort_by_key(|(index, _)| *index);
    assert_eq!(refused, [(3, parent()), (6, Refusal::NotFound)]);
    let steps = planned.steps;
    let (own, kid) = match (&steps[0], &steps[1]) {
        (Step::Create { id: own, .. }, Step::Create { id: kid, .. }) => (own.clone(), kid.clone()),
        other => panic!("{other:?}"),
    };
    assert!(matches!(
        &steps[2],
        Step::Create { naming: Naming { under: Under::Mailbox(id), .. }, .. }
            if id.as_str() == "mwork"
    ));
    assert!(matches!(
        &steps[3],
        Step::Update { index: 4, id, rename: Some(_), followers, .. }
            if *id == own && *followers == [own.clone(), kid.clone()]
    ));
    assert!(matches!(
        &steps[4],
        Step::Update { index: 5, id, subscribe: Some(false), .. } if id.as_str() == "mwork"
    ));
    assert!(matches!(&steps[5], Step::Destroy { index: 7, id, .. } if *id == kid));
    assert_eq!(steps.len(), 6);
}
