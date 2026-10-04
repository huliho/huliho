// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What a patch reads as: the keywords it sets and clears, the whole
//! object, the paths and values named back and the texts that are no
//! keyword and so never reach a command line.

use proptest::prelude::*;

use super::*;

fn named(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn a_patch_sets_with_true_and_clears_with_null_rfc8620_5_3() {
    let read = edit(&json!({ "keywords/$Seen": true, "keywords/Work": null })).unwrap();
    assert_eq!(read.set, ["$seen"]);
    assert_eq!(read.clear, ["work"]);
    assert!(!read.whole);
    assert_eq!(edit(&json!({})).unwrap(), Edit::default());
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
fn a_value_that_is_neither_true_nor_null_and_any_other_property_are_named_back() {
    for (patch, paths) in [
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
        (json!({ "mailboxIds/m1": true }), named(&["mailboxIds/m1"])),
        (json!({ "keywordsx": true }), named(&["keywordsx"])),
        (
            json!({ "subject": "x", "keywords/$seen": true }),
            named(&["subject"]),
        ),
    ] {
        assert_eq!(
            edit(&patch),
            Err(SetError::InvalidProperties(paths)),
            "{patch}"
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
    for patch in [
        json!("keywords"),
        json!(null),
        json!({ "keywords": { "$seen": true }, "keywords/$flagged": true }),
        json!({ "keywords/$seen": true, "keywords/$Seen": null }),
    ] {
        assert_eq!(edit(&patch), Err(SetError::InvalidPatch), "{patch}");
    }
}

#[test]
fn keywords_past_the_byte_bound_are_too_many_rfc8621_4_6() {
    let each = 200;
    let count = MAX_UPDATE_KEYWORD_BYTES / (each + 1) + 1;
    let patch: Map<String, Value> = (0..count)
        .map(|index| {
            let keyword = format!("{index:0each$}");
            (format!("keywords/{keyword}"), Value::Bool(true))
        })
        .collect();
    assert_eq!(edit(&Value::Object(patch)), Err(SetError::TooManyKeywords));
}

/// A path as a client may write one: under `keywords` in printable
/// ASCII or in any byte of Latin-1 with the controls, the whole of it
/// or anything else.
fn path() -> impl Strategy<Value = String> {
    prop_oneof![
        "keywords/[ -~]{0,24}",
        "keywords/[\\x00-\\x{ff}]{0,12}",
        Just("keywords".to_owned()),
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
        let patch: Map<String, Value> = entries.into_iter().collect();
        if let Ok(read) = edit(&Value::Object(patch)) {
            for keyword in read.set.iter().chain(&read.clear) {
                prop_assert_eq!(canonical_keyword(keyword), Some(keyword.clone()));
                let specials = b"(){%*\"\\]";
                let atom = |byte: u8| byte.is_ascii_graphic() && !specials.contains(&byte);
                prop_assert!(keyword.bytes().all(atom), "{keyword:?}");
            }
            prop_assert!(!read.set.iter().any(|keyword| read.clear.contains(keyword)));
            prop_assert!(!read.whole || read.clear.is_empty());
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
        (SetError::ServerFail, "serverFail"),
        (SetError::ServerUnavailable, "serverUnavailable"),
    ] {
        assert_eq!(error.object()["type"], kind);
    }
}

/// A STORE of every keyword one update may name over a full set of
/// UIDs at their widest stays on one command line.
#[test]
fn the_keyword_bound_keeps_a_store_on_one_command_line() {
    use crate::session::{MAX_COMMAND_BYTES, MAX_STORE_UIDS};
    /// A UID of ten digits and its comma.
    const UID_WIRE_BYTES: usize = 11;
    let command = "UID STORE  +FLAGS.SILENT ()".len();
    let widest = command + MAX_STORE_UIDS * UID_WIRE_BYTES + MAX_UPDATE_KEYWORD_BYTES;
    assert!(widest <= MAX_COMMAND_BYTES, "{widest}");
}
