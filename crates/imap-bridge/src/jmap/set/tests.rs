// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What a patch reads as: the keywords it sets and clears, the mailboxes
//! it adds and removes, the whole objects, the paths and values named
//! back and the texts that are no keyword and so never reach a command
//! line.

use proptest::prelude::*;

use super::patch::patch;
use super::*;
use crate::sync::mapping::canonical_keyword;

fn named(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

/// The keyword part of a patch.
fn edit(value: &Value) -> Result<Edit, SetError> {
    patch(value).map(|read| read.keywords)
}

#[test]
fn a_patch_sets_with_true_and_clears_with_null_rfc8620_5_3() {
    let read = edit(&json!({ "keywords/$Seen": true, "keywords/Work": null })).unwrap();
    assert_eq!(read.set, ["$seen"]);
    assert_eq!(read.clear, ["work"]);
    assert!(!read.whole);
    assert_eq!(patch(&json!({})).unwrap(), Patch::default());
}

#[test]
fn a_whole_keywords_object_names_what_the_email_is_left_with_rfc8621_4_1_1() {
    let read = edit(&json!({ "keywords": { "$seen": true, "$Flagged": true } })).unwrap();
    assert!(read.whole && read.clear.is_empty());
    let mut set = read.set.clone();
    set.sort();
    assert_eq!(set, ["$flagged", "$seen"]);
    assert_eq!(read.path("$seen"), "keywords");
    assert_eq!(edit(&json!({ "keywords": {} })).unwrap().set, [""; 0]);
}

#[test]
fn a_mailbox_path_adds_with_true_and_removes_with_null_rfc8621_4_6() {
    let read = patch(&json!({ "mailboxIds/m1": null, "mailboxIds/m~12": true })).unwrap();
    let mailboxes = read.mailboxes.unwrap();
    assert_eq!(mailboxes.add, ["m/2"]);
    assert_eq!(mailboxes.remove, ["m1"]);
    assert!(!mailboxes.whole);
    assert_eq!(mailboxes.path("m/2"), "mailboxIds/m~12");
    assert_eq!(read.keywords, Edit::default());
    let whole = patch(&json!({ "mailboxIds": { "m3": true } })).unwrap();
    let whole = whole.mailboxes.unwrap();
    assert!(whole.whole);
    assert_eq!(whole.add, ["m3"]);
    assert_eq!(whole.path("m3"), "mailboxIds");
    assert_eq!(
        patch(&json!({ "keywords/$seen": true })).unwrap().mailboxes,
        None
    );
}

#[test]
fn an_email_left_in_no_mailbox_by_a_whole_object_is_invalid_rfc8621_4_1_1() {
    assert_eq!(
        patch(&json!({ "mailboxIds": {} })),
        Err(SetError::InvalidProperties(named(&["mailboxIds"])))
    );
}

#[test]
fn a_value_that_is_neither_true_nor_null_and_any_other_property_are_named_back() {
    for (value, paths) in [
        (
            json!({ "keywords/$seen": false }),
            named(&["keywords/$seen"]),
        ),
        (json!({ "keywords/$seen": 1 }), named(&["keywords/$seen"])),
        (
            json!({ "keywords": { "$seen": false } }),
            named(&["keywords"]),
        ),
        (json!({ "keywords": null }), named(&["keywords"])),
        (json!({ "keywords": ["$seen"] }), named(&["keywords"])),
        (json!({ "mailboxIds/m1": false }), named(&["mailboxIds/m1"])),
        (json!({ "mailboxIds/": true }), named(&["mailboxIds/"])),
        (
            json!({ "mailboxIds/a/b": true }),
            named(&["mailboxIds/a/b"]),
        ),
        (json!({ "mailboxIds/a~": true }), named(&["mailboxIds/a~"])),
        (
            json!({ "mailboxIds": { "m1": false } }),
            named(&["mailboxIds"]),
        ),
        (json!({ "mailboxIds": null }), named(&["mailboxIds"])),
        (json!({ "keywordsx": true }), named(&["keywordsx"])),
        (
            json!({ "subject": "x", "keywords/$seen": true }),
            named(&["subject"]),
        ),
    ] {
        assert_eq!(
            patch(&value),
            Err(SetError::InvalidProperties(paths)),
            "{value}"
        );
    }
}

#[test]
fn a_keyword_that_is_no_keyword_never_becomes_an_edit_rfc8621_4_1_1() {
    let long = format!("keywords/{}", "k".repeat(256));
    for path in [
        "keywords/",
        "keywords/a b",
        "keywords/a)",
        "keywords/(a",
        "keywords/a{3}",
        "keywords/a]",
        "keywords/a%",
        "keywords/a*",
        "keywords/a\"b",
        "keywords/\\Deleted",
        "keywords/\\Seen",
        "keywords/caf\u{e9}",
        "keywords/a\r\nA1 LOGOUT",
        "keywords/a/b",
        "keywords/a~2b",
        "keywords/a~",
        long.as_str(),
    ] {
        assert_eq!(
            edit(&json!({ path: true })),
            Err(SetError::InvalidProperties(named(&[path]))),
            "{path:?}"
        );
    }
    let whole = json!({ "keywords": { "\\Deleted": true } });
    assert_eq!(
        edit(&whole),
        Err(SetError::InvalidProperties(named(&["keywords"])))
    );
}

#[test]
fn a_keyword_is_named_back_by_the_path_the_patch_wrote_rfc6901_3() {
    let read = edit(&json!({ "keywords/a~1b~0c": true, "keywords/Project": null })).unwrap();
    assert_eq!(read.set, ["a/b~c"]);
    assert_eq!(read.clear, ["project"]);
    assert_eq!(read.path("a/b~c"), "keywords/a~1b~0c");
    assert_eq!(read.path("project"), "keywords/Project");
}

#[test]
fn a_patch_that_is_no_patch_is_refused_as_such_rfc8620_5_3() {
    for value in [
        json!("keywords"),
        json!(null),
        json!({ "keywords": { "$seen": true }, "keywords/$flagged": true }),
        json!({ "keywords/$seen": true, "keywords/$Seen": null }),
        json!({ "mailboxIds": { "m1": true }, "mailboxIds/m2": true }),
    ] {
        assert_eq!(patch(&value), Err(SetError::InvalidPatch), "{value}");
    }
}

#[test]
fn keywords_past_the_byte_bound_are_too_many_rfc8621_4_6() {
    let each = 200;
    let count = MAX_UPDATE_KEYWORD_BYTES / (each + 1) + 1;
    let value: Map<String, Value> = (0..count)
        .map(|index| {
            let keyword = format!("{index:0each$}");
            (format!("keywords/{keyword}"), Value::Bool(true))
        })
        .collect();
    assert_eq!(edit(&Value::Object(value)), Err(SetError::TooManyKeywords));
}

/// A path as a client may write one: under `keywords` or `mailboxIds`
/// in printable ASCII or in any byte of Latin-1 with the controls, the
/// whole of either or anything else.
fn path() -> impl Strategy<Value = String> {
    prop_oneof![
        "keywords/[ -~]{0,24}",
        "keywords/[\\x00-\\x{ff}]{0,12}",
        "mailboxIds/[ -~]{0,24}",
        Just("keywords".to_owned()),
        Just("mailboxIds".to_owned()),
        "[ -~]{0,12}",
    ]
}

/// A value as a client may write one.
fn value() -> impl Strategy<Value = Value> {
    prop_oneof![
        Just(Value::Bool(true)),
        Just(Value::Null),
        Just(Value::Bool(false)),
        Just(json!({ "$Seen": true, "a b": true })),
        Just(json!({ "Work": true })),
    ]
}

proptest! {
    #[test]
    fn whatever_a_patch_holds_an_edit_names_keywords_that_are_atoms(
        entries in prop::collection::vec((path(), value()), 0..6),
    ) {
        let value: Map<String, Value> = entries.into_iter().collect();
        if let Ok(read) = patch(&Value::Object(value)) {
            let keywords = &read.keywords;
            for keyword in keywords.set.iter().chain(&keywords.clear) {
                prop_assert_eq!(canonical_keyword(keyword), Some(keyword.clone()));
                let specials = b"(){%*\"\\]";
                let atom = |byte: u8| byte.is_ascii_graphic() && !specials.contains(&byte);
                prop_assert!(keyword.bytes().all(atom), "{keyword:?}");
            }
            prop_assert!(!keywords.set.iter().any(|keyword| keywords.clear.contains(keyword)));
            prop_assert!(!keywords.whole || keywords.clear.is_empty());
            if let Some(mailboxes) = &read.mailboxes {
                prop_assert!(!mailboxes.add.iter().any(|id| mailboxes.remove.contains(id)));
                prop_assert!(!mailboxes.whole || !mailboxes.add.is_empty());
                prop_assert!(mailboxes.add.iter().chain(&mailboxes.remove).all(|id| !id.is_empty()));
            }
        }
    }
}

#[test]
fn a_set_error_renders_its_type_and_the_properties_it_names() {
    assert_eq!(SetError::Forbidden.object(), json!({ "type": "forbidden" }));
    assert_eq!(
        SetError::InvalidProperties(named(&["keywords/x"])).object(),
        json!({ "type": "invalidProperties", "properties": ["keywords/x"] })
    );
    for (error, kind) in [
        (SetError::NotFound, "notFound"),
        (SetError::InvalidPatch, "invalidPatch"),
        (SetError::TooManyKeywords, "tooManyKeywords"),
        (SetError::TooManyMailboxes, "tooManyMailboxes"),
        (SetError::WillDestroy, "willDestroy"),
        (SetError::ServerFail, "serverFail"),
        (SetError::ServerUnavailable, "serverUnavailable"),
    ] {
        assert_eq!(error.object(), json!({ "type": kind }));
    }
}

/// A STORE of every keyword one update may name, or of labels as wide,
/// over a full set of UIDs at their widest stays on one command line.
#[test]
fn the_keyword_bound_keeps_a_store_on_one_command_line() {
    use crate::session::{MAX_COMMAND_BYTES, MAX_STORE_UIDS};
    /// A UID of ten digits and its comma.
    const UID_WIRE_BYTES: usize = 11;
    // The wider of the two items a STORE changes.
    let command = "UID STORE  +X-GM-LABELS.SILENT ()".len();
    let widest = command + MAX_STORE_UIDS * UID_WIRE_BYTES + MAX_UPDATE_KEYWORD_BYTES;
    assert!(widest <= MAX_COMMAND_BYTES, "{widest}");
}
