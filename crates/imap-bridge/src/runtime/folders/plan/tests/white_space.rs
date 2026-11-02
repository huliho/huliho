// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! White space in a row, which Gmail refuses on every try and every other
//! server takes (RFC 8621 section 2).

use super::case::gmail;
use super::*;

/// Two spaces, then a no-break and an ideographic space beside a space.
const CROWDED: [&str; 3] = ["a  b", "c\u{a0} d", "e \u{3000}f"];

#[test]
fn white_space_in_a_row_is_refused_on_gmail_by_a_create_a_child_and_a_rename_rfc8621_2() {
    for typed in CROWDED {
        let ask = Ask {
            creates: vec![create("a", typed, None), create("b", typed, Some("mwork"))],
            updates: vec![Update {
                name: Some(typed.to_owned()),
                ..update("mlocked")
            }],
            ..Ask::default()
        };
        let (refused, steps) = outcome(&gmail(), &ask);
        assert_eq!(refused, [(0, name()), (1, name()), (2, name())], "{typed}");
        assert_eq!(steps, Vec::<usize>::new());
    }
}

#[test]
fn white_space_in_a_row_goes_out_on_any_other_account_rfc8621_2() {
    for typed in CROWDED {
        let ask = Ask {
            creates: vec![create("a", typed, None)],
            ..Ask::default()
        };
        assert_eq!(outcome(&dovecot(), &ask), (Vec::new(), vec![0]), "{typed}");
    }
}

#[test]
fn a_refused_rename_leaves_the_folders_own_name_held_on_gmail_rfc8620_5_3() {
    let ask = Ask {
        updates: vec![
            Update {
                name: Some("x  y".to_owned()),
                ..update("mwork")
            },
            Update {
                name: Some("WORK".to_owned()),
                ..update("mlocked")
            },
        ],
        ..Ask::default()
    };
    let (refused, steps) = outcome(&gmail(), &ask);
    assert_eq!(refused, [(0, name()), (1, name())]);
    assert_eq!(steps, Vec::<usize>::new());
}
