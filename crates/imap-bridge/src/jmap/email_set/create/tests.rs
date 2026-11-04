// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

use proptest::prelude::*;
use serde_json::json;

use super::values::{MAX_ID_BYTES, MAX_MESSAGE_IDS};
use super::*;

/// A draft the client sends: text and HTML, one attachment by its blob.
fn object() -> Value {
    json!({
        "mailboxIds": { "m1": true },
        "keywords": { "$draft": true, "$seen": true },
        "from": [{ "name": "Sanne", "email": "sanne@example.test" }],
        "to": [{ "name": "Mo, the first", "email": "mo@example.test" }, { "email": "li@example.test" }],
        "bcc": [{ "email": "hidden@example.test" }],
        "subject": "Caf\u{e9} om drie",
        "inReplyTo": ["p1@example.test"],
        "references": ["p0@example.test", "p1@example.test"],
        "bodyStructure": {
            "type": "multipart/mixed",
            "subParts": [
                {
                    "type": "multipart/alternative",
                    "subParts": [
                        { "partId": "t", "header:Content-Type": " text/plain; format=flowed; delsp=yes" },
                        { "partId": "h", "type": "text/html" },
                    ],
                },
                {
                    "blobId": "e1-2",
                    "type": "application/pdf",
                    "name": "rapport caf\u{e9}.pdf",
                    "disposition": "attachment",
                },
            ],
        },
        "bodyValues": {
            "t": { "value": "Hallo \nwereld\n", "isTruncated": false },
            "h": { "value": "<p>Hallo wereld</p>" },
        },
    })
}

/// The object with one property changed.
fn with(path: &str, value: Value) -> Value {
    let mut object = object();
    *object.pointer_mut(path).unwrap_or_else(|| panic!("{path}")) = value;
    object
}

/// The paths a refused object names.
fn refused(object: &Value) -> Vec<String> {
    match read(object) {
        Err(SetError::InvalidProperties(paths)) => paths,
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_object_of_the_client_reads_into_a_draft_rfc8621_4_6() {
    let draft = read(&object()).unwrap();
    assert_eq!(draft.mailbox, "m1");
    assert_eq!(draft.keywords, ["$draft", "$seen"]);
    assert_eq!(draft.personal.to.as_ref().unwrap().len(), 2);
    assert_eq!(draft.personal.subject.as_deref(), Some("Caf\u{e9} om drie"));
    let Content::Parts(parts) = &draft.body.content else {
        panic!("{:?}", draft.body);
    };
    let Content::Parts(texts) = &parts[0].content else {
        panic!("{:?}", parts[0]);
    };
    assert_eq!(texts[0].media_type, "text/plain");
    assert_eq!(
        texts[0].parameters,
        [
            ("format".to_owned(), "flowed".to_owned()),
            ("delsp".to_owned(), "yes".to_owned())
        ]
    );
    assert_eq!(
        texts[1].content,
        Content::Value("<p>Hallo wereld</p>".to_owned())
    );
    assert_eq!(parts[1].content, Content::Blob("e1-2".to_owned()));
    assert_eq!(parts[1].disposition.as_deref(), Some("attachment"));
}

#[test]
fn a_property_the_server_sets_or_a_body_list_is_named_back_rfc8621_4_6() {
    for name in [
        "messageId",
        "sentAt",
        "receivedAt",
        "size",
        "blobId",
        "threadId",
        "headers",
        "textBody",
        "htmlBody",
        "attachments",
        "header:X-Other:asText",
    ] {
        let mut object = object();
        object[name] = json!([]);
        assert_eq!(refused(&object), [name], "{name}");
    }
}

#[test]
fn a_body_is_given_by_body_structure_alone() {
    let mut object = object();
    object.as_object_mut().unwrap().remove("bodyStructure");
    assert_eq!(refused(&object), ["bodyStructure"]);
}

#[test]
fn a_mailbox_list_names_one_mailbox_rfc8621_4_6() {
    for mailboxes in [
        json!({}),
        json!({ "m1": true, "m2": true }),
        json!({ "m1": false }),
        json!(["m1"]),
        json!({ "": true }),
    ] {
        assert_eq!(refused(&with("/mailboxIds", mailboxes)), ["mailboxIds"]);
    }
    let mut object = object();
    object.as_object_mut().unwrap().remove("mailboxIds");
    assert_eq!(refused(&object), ["mailboxIds"]);
}

#[test]
fn a_keyword_outside_the_four_system_flags_is_refused_rfc8621_4_1_1() {
    for keywords in [
        json!({ "$custom": true }),
        json!({ "$junk": true }),
        json!({ "$draft": false }),
        json!({ "$draft": null }),
        json!(["$draft"]),
    ] {
        assert_eq!(refused(&with("/keywords", keywords)), ["keywords"]);
    }
    assert_eq!(
        read(&with("/keywords", json!(null))).unwrap().keywords,
        Vec::<String>::new()
    );
    assert_eq!(
        read(&with("/keywords", json!({ "$FLAGGED": true })))
            .unwrap()
            .keywords,
        ["$flagged"]
    );
}

#[test]
fn a_header_value_with_a_line_break_or_a_nul_is_refused_before_anything_is_built() {
    for bad in ["a\r\nBcc: evil@example.test", "a\nb", "a\rb", "a\0b"] {
        assert_eq!(
            refused(&with("/subject", json!(bad))),
            ["subject"],
            "{bad:?}"
        );
        let name = with("/to/0/name", json!(bad));
        assert_eq!(refused(&name), ["to"], "{bad:?}");
        let email = with("/to/0/email", json!(format!("{bad}@example.test")));
        assert_eq!(refused(&email), ["to"], "{bad:?}");
        let attachment = with("/bodyStructure/subParts/1/name", json!(bad));
        assert_eq!(
            refused(&attachment),
            ["bodyStructure/subParts/1/name"],
            "{bad:?}"
        );
    }
}

#[test]
fn an_address_that_reads_as_two_is_refused_rfc5322_3_4() {
    for bad in [
        "a>,<evil@example.test",
        "not an address",
        "a@b",
        "a@b@c.test",
    ] {
        assert_eq!(
            refused(&with("/from/0/email", json!(bad))),
            ["from"],
            "{bad}"
        );
    }
    let unknown = with("/from/0", json!({ "email": "a@b.test", "other": 1 }));
    assert_eq!(refused(&unknown), ["from"]);
}

#[test]
fn an_id_field_holds_ids_without_their_brackets_rfc8621_4_1_2_3() {
    for bad in [
        "<a@b.test>",
        "a b@c.test",
        "",
        "a\r\n@b.test",
        "caf\u{e9}@b.test",
    ] {
        assert_eq!(
            refused(&with("/inReplyTo", json!([bad]))),
            ["inReplyTo"],
            "{bad:?}"
        );
        assert_eq!(
            refused(&with("/references", json!([bad]))),
            ["references"],
            "{bad:?}"
        );
    }
    let many = vec!["a@b.test"; MAX_MESSAGE_IDS + 1];
    assert_eq!(refused(&with("/references", json!(many))), ["references"]);
    let long = "a".repeat(MAX_ID_BYTES + 1);
    assert_eq!(refused(&with("/references", json!([long]))), ["references"]);
    assert!(read(&with("/references", json!([&long[1..]]))).is_ok());
}

#[test]
fn a_value_goes_under_one_part_alone() {
    let twice = json!({
        "type": "multipart/mixed",
        "subParts": [{ "partId": "t" }, { "partId": "t" }],
    });
    assert_eq!(
        refused(&with("/bodyStructure", twice)),
        ["bodyStructure/subParts/1/partId"]
    );
}

#[test]
fn a_type_is_one_token_on_each_side_in_lower_case_rfc2045_5_1() {
    for bad in ["text/plain; x=y", "text/pl\"ain", "text", "/plain", "text/"] {
        let part = json!({ "partId": "t", "type": bad });
        let paths = refused(&with("/bodyStructure", part));
        assert_eq!(paths, ["bodyStructure/type"], "{bad:?}");
    }
    let part = json!({ "partId": "t", "type": "TEXT/PLAIN" });
    let draft = read(&with("/bodyStructure", part)).unwrap();
    assert_eq!(draft.body.media_type, "text/plain");
}

#[test]
fn a_body_value_holds_no_nul_and_no_flag_set_rfc8621_4_6() {
    let nul = with("/bodyValues/t", json!({ "value": "a\0b" }));
    assert_eq!(refused(&nul), ["bodyValues/t"]);
    for flag in ["isTruncated", "isEncodingProblem"] {
        let set = with("/bodyValues/t", json!({ "value": "a", flag: true }));
        assert_eq!(refused(&set), ["bodyValues/t"], "{flag}");
    }
    let lines = with("/bodyValues/t", json!({ "value": "a\r\nb\nc\rd" }));
    assert!(read(&lines).is_ok());
}

#[test]
fn a_part_carries_a_part_id_or_a_blob_id_and_not_both_rfc8621_4_6() {
    let both = with("/bodyStructure", json!({ "partId": "t", "blobId": "e1" }));
    assert_eq!(refused(&both), ["bodyStructure/blobId"]);
    let neither = with("/bodyStructure", json!({ "type": "text/plain" }));
    assert_eq!(refused(&neither), ["bodyStructure/partId"]);
    let outside = with("/bodyStructure", json!({ "partId": "other" }));
    assert_eq!(refused(&outside), ["bodyStructure/partId"]);
    let tree = with("/bodyStructure", json!({ "partId": "t", "subParts": [] }));
    assert_eq!(refused(&tree), ["bodyStructure/partId"]);
    let empty = with("/bodyStructure", json!({ "subParts": [] }));
    assert_eq!(refused(&empty), ["bodyStructure/subParts"]);
}

#[test]
fn what_a_part_may_give_follows_what_it_holds_rfc8621_4_6() {
    let cases = [
        (
            json!({ "partId": "t", "charset": "utf-8" }),
            "bodyStructure/charset",
        ),
        (json!({ "partId": "t", "size": 3 }), "bodyStructure/size"),
        (
            json!({ "partId": "t", "type": "image/png" }),
            "bodyStructure/type",
        ),
        (
            json!({ "blobId": "e1", "type": "multipart/mixed" }),
            "bodyStructure/type",
        ),
        (
            json!({ "subParts": [{ "partId": "t" }], "type": "text/plain" }),
            "bodyStructure/type",
        ),
        (
            json!({ "subParts": [{ "partId": "t" }], "name": "a" }),
            "bodyStructure/name",
        ),
        (
            json!({ "partId": "t", "disposition": "other" }),
            "bodyStructure/disposition",
        ),
        (
            json!({ "partId": "t", "cid": "<a@b>" }),
            "bodyStructure/cid",
        ),
        (
            json!({ "partId": "t", "headers": [] }),
            "bodyStructure/headers",
        ),
        (
            json!({ "partId": "t", "type": "text/plain", "header:Content-Type": " text/plain" }),
            "bodyStructure/type",
        ),
        (
            json!({ "partId": "t", "header:Content-Type": " text/plain; charset=utf-8" }),
            "bodyStructure/charset",
        ),
        (
            json!({ "partId": "t", "header:Content-Type": " text/plain; boundary=x" }),
            "bodyStructure/header:Content-Type",
        ),
        (
            json!({ "partId": "t", "type": "text plain" }),
            "bodyStructure/type",
        ),
    ];
    for (part, path) in cases {
        assert_eq!(
            refused(&with("/bodyStructure", part.clone())),
            [path],
            "{part}"
        );
    }
    let blob = with(
        "/bodyStructure",
        json!({ "blobId": "e1", "header:Content-Type": " text/plain; charset=utf-8; format=flowed", "size": 9 }),
    );
    assert_eq!(
        read(&blob).unwrap().body.parameters,
        [
            ("charset".to_owned(), "utf-8".to_owned()),
            ("format".to_owned(), "flowed".to_owned())
        ]
    );
}

#[test]
fn a_tree_past_its_depth_or_its_parts_is_refused() {
    let mut deep = json!({ "partId": "t" });
    for _ in 0..MAX_DEPTH {
        deep = json!({ "subParts": [deep] });
    }
    let paths = refused(&with("/bodyStructure", deep));
    assert!(!paths.is_empty(), "{paths:?}");
    let leaf = json!({ "blobId": "e1-1", "type": "image/png" });
    let fits = vec![leaf.clone(); MAX_PARTS - 1];
    assert!(read(&with("/bodyStructure", json!({ "subParts": fits }))).is_ok());
    let wide = vec![leaf; MAX_PARTS + 1];
    let wide = with("/bodyStructure", json!({ "subParts": wide }));
    let paths = refused(&wide);
    let first_past = format!("bodyStructure/subParts/{}", MAX_PARTS - 1);
    assert!(paths.contains(&first_past), "{paths:?}");
}

#[test]
fn an_object_that_is_no_object_names_nothing() {
    assert_eq!(refused(&json!("draft")), Vec::<String>::new());
}

proptest! {
    /// Any text reads as a header value or is refused, and what reads
    /// holds no line break.
    #[test]
    fn a_subject_that_reads_holds_no_line_break(text in any::<String>()) {
        if let Ok(draft) = read(&with("/subject", json!(text))) {
            let subject = draft.personal.subject.unwrap();
            prop_assert!(!subject.contains(['\r', '\n', '\0']));
        }
    }

    /// Any shape of a part reads or is named back; none panics.
    #[test]
    fn any_part_reads_or_is_refused(raw in any::<String>(), kind in any::<String>()) {
        let part = json!({ "partId": "t", "header:Content-Type": raw });
        let _ = read(&with("/bodyStructure", part));
        let part = json!({ "blobId": "e1", "type": kind });
        let _ = read(&with("/bodyStructure", part));
    }
}
