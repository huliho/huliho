// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The filter, the sort and the limit as `Email/query` reads them.

use serde_json::json;

use super::*;

fn comparators(value: Value) -> Vec<Comparator> {
    serde_json::from_value(value).unwrap()
}

fn read(filter: &Value) -> Result<Filter, MethodError> {
    filter_of(Some(filter))
}

#[test]
fn the_filter_takes_the_scope_the_keywords_the_dates_and_the_text_rfc8621_4_4_1() {
    let both = json!({ "inMailbox": "m1", "hasKeyword": "$Flagged", "notKeyword": "$seen" });
    assert_eq!(
        read(&both),
        Ok(Filter {
            in_mailbox: Some("m1".to_owned()),
            has_keyword: Some("$flagged".to_owned()),
            not_keyword: Some("$seen".to_owned()),
            ..Filter::default()
        })
    );
    let search = json!({
        "inMailboxOtherThan": ["m8", "m9"],
        "text": "zoë \"q3 plan\"",
        "from": "anna",
        "after": "2026-10-01T00:00:00Z",
        "before": "2026-10-24T00:00:00.5Z",
        "hasAttachment": true,
    });
    assert_eq!(
        read(&search),
        Ok(Filter {
            other_than: Some(vec!["m8".to_owned(), "m9".to_owned()]),
            text: "zoë \"q3 plan\"".to_owned(),
            from: "anna".to_owned(),
            after: Some(1_790_812_800),
            before: Some(1_792_800_001),
            has_attachment: Some(true),
            ..Filter::default()
        })
    );
    assert_eq!(read(&json!({})), Ok(Filter::default()));
    assert_eq!(filter_of(None), Ok(Filter::default()));
}

#[test]
fn an_operator_another_property_or_a_wrong_type_is_unsupported_rfc8620_5_5() {
    for refused in [
        json!({ "inMailbox": "m1", "someInThreadHaveKeyword": "$seen" }),
        json!({ "inMailbox": "m1", "hasKeyword": ["$seen"] }),
        json!({ "operator": "AND", "conditions": [{ "inMailbox": "m1" }] }),
        json!({ "inMailbox": 7 }),
        json!({ "subject": "lunch" }),
        json!({ "minSize": 10 }),
        json!({ "text": 7 }),
        json!("m1"),
    ] {
        assert_eq!(
            read(&refused),
            Err(MethodError::UnsupportedFilter(UNSERVED)),
            "{refused}"
        );
    }
}

#[test]
fn a_keyword_that_is_none_or_a_date_that_is_no_utc_date_is_invalid_rfc8620_1_4() {
    for keyword in ["a b", "\\Seen", "", "caf\u{e9}"] {
        let filter = json!({ "inMailbox": "m1", "notKeyword": keyword });
        assert_eq!(
            read(&filter),
            Err(MethodError::InvalidArguments(NO_KEYWORD)),
            "{keyword:?}"
        );
    }
    for date in [
        "2026-10-01",
        "2026-10-01T00:00:00",
        "2026-10-01T02:00:00+02:00",
        "2026-10-01T00:00:00+00:00",
        "yesterday",
    ] {
        let filter = json!({ "after": date });
        assert_eq!(
            read(&filter),
            Err(MethodError::InvalidArguments(NO_DATE)),
            "{date}"
        );
    }
    // RFC 3339 section 5.6: the `Z` may be lower case.
    let lower = read(&json!({ "after": "2026-10-01T00:00:00z" })).unwrap();
    assert_eq!(lower.after, Some(1_790_812_800));
}

#[test]
fn only_a_mailbox_with_its_keywords_is_the_window_index_s() {
    let window = |filter: Value| read(&filter).unwrap().is_window();
    assert!(window(json!({ "inMailbox": "m1", "notKeyword": "$seen" })));
    assert!(window(json!({ "inMailbox": "m1", "text": "\"\" " })));
    for searched in [
        json!({}),
        json!({ "inMailbox": "m1", "text": "zoë" }),
        json!({ "inMailbox": "m1", "from": "anna" }),
        json!({ "inMailbox": "m1", "hasAttachment": false }),
        json!({ "inMailbox": "m1", "after": "2026-10-01T00:00:00Z" }),
        json!({ "inMailbox": "m1", "inMailboxOtherThan": ["m2"] }),
        json!({ "inMailboxOtherThan": ["m2"] }),
    ] {
        assert!(!window(searched.clone()), "{searched}");
    }
}

#[test]
fn a_term_past_the_caps_or_holding_a_nul_reads_as_invalid_in_the_session_s_words() {
    let long = "a".repeat(crate::session::SEARCH_TERM_BYTES + 1);
    for (text, from) in [(long.as_str(), ""), ("a b c d e", "f g h i"), ("a\0b", "")] {
        let filter = Filter {
            text: text.to_owned(),
            from: from.to_owned(),
            ..Filter::default()
        };
        assert!(filter.searches());
        let refusal = filter.ask().check().map_err(|error| refused(&error));
        assert!(
            matches!(refusal, Err(MethodError::InvalidArguments(_))),
            "{refusal:?}"
        );
    }
}

#[test]
fn debug_output_prints_neither_the_text_nor_the_sender() {
    let filter = read(&json!({ "text": "xylophone", "from": "anna@example.org" })).unwrap();
    let printed = format!("{filter:?}");
    for word in ["xylophone", "anna", "example"] {
        assert!(!printed.contains(word), "{printed}");
    }
}

#[test]
fn a_search_that_found_nothing_answers_by_why() {
    assert_eq!(failed(Failure::Unavailable), MethodError::ServerUnavailable);
    assert_eq!(
        failed(Failure::Charset),
        MethodError::UnsupportedFilter(NO_CHARSET)
    );
    assert_eq!(failed(Failure::Unreadable), MethodError::ServerFail);
}

#[test]
fn the_sort_is_received_at_either_way_rfc8620_5_5() {
    assert!(!ascending(None).unwrap());
    assert!(!ascending(Some(&[])).unwrap());
    let newest = comparators(json!([{ "property": "receivedAt", "isAscending": false }]));
    assert!(!ascending(Some(&newest)).unwrap());
    let oldest = comparators(json!([{ "property": "receivedAt" }]));
    assert!(ascending(Some(&oldest)).unwrap());
    for refused in [
        json!([{ "property": "subject" }]),
        json!([{ "property": "receivedAt" }, { "property": "size" }]),
        json!([{ "property": "receivedAt", "collation": "i;unicode-casemap" }]),
    ] {
        let sort = comparators(refused.clone());
        assert_eq!(
            ascending(Some(&sort)),
            Err(MethodError::UnsupportedSort),
            "{refused}"
        );
    }
    assert!(serde_json::from_value::<Comparator>(json!({ "isAscending": true })).is_err());
}

#[test]
fn the_limit_is_lowered_to_the_server_s_and_the_answer_says_so_rfc8620_5_5() {
    assert_eq!(limit(Some(50)), (50, false));
    assert_eq!(limit(Some(u64::from(QUERY_LIMIT))), (QUERY_LIMIT, false));
    assert_eq!(limit(Some(u64::from(QUERY_LIMIT) + 1)), (QUERY_LIMIT, true));
    assert_eq!(limit(None), (QUERY_LIMIT, true));
    assert_eq!(limit(Some(0)), (0, false));
}
