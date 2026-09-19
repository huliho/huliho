// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! `Mailbox/changes` over two passes and the result references that
//! chain it into `Mailbox/get`.

mod jmap_rig;

use jmap_rig::{ACCOUNT, Rig, error_type, first, mailbox_get};
use serde_json::{Value, json};

fn changes(since: &str, max: Option<u64>) -> Value {
    json!([
        "Mailbox/changes",
        { "accountId": ACCOUNT, "sinceState": since, "maxChanges": max },
        "c1"
    ])
}

fn lengths(answer: &Value) -> (usize, usize, usize) {
    let count = |name: &str| answer[name].as_array().unwrap().len();
    (count("created"), count("updated"), count("destroyed"))
}

#[tokio::test]
async fn mailbox_changes_reads_the_log_and_folds_it_rfc8620_5_2() {
    let rig = Rig::start().await;
    rig.edit();
    assert_eq!(rig.pass().await, 2);
    let since_one = rig.mail(json!([changes("1", None)]));
    let answer = &first(&since_one)[1];
    assert_eq!(
        (&answer["oldState"], &answer["newState"]),
        (&json!("1"), &json!("2"))
    );
    assert_eq!(answer["hasMoreChanges"], false);
    assert_eq!(answer["updatedProperties"], Value::Null);
    assert_eq!(lengths(answer), (1, 1, 1));
    assert_eq!(answer["created"][0], rig.id_of("Work"));
    assert_eq!(answer["updated"][0], rig.id_of("INBOX"));
    assert_eq!(since_one["sessionState"], "2");
    let since_zero = rig.mail(json!([changes("0", None)]));
    assert_eq!(lengths(&first(&since_zero)[1]), (6, 0, 0));
    let current = rig.mail(json!([changes("2", None)]));
    assert_eq!(lengths(&first(&current)[1]), (0, 0, 0));
}

#[tokio::test]
async fn max_changes_is_never_exceeded_and_a_first_sequence_past_it_cannot_calculate_rfc8620_5_2() {
    let rig = Rig::start().await;
    rig.edit();
    rig.pass().await;
    for (since, max) in [("0", 1), ("0", 5), ("1", 2)] {
        let capped = rig.mail(json!([changes(since, Some(max))]));
        assert_eq!(
            error_type(first(&capped)),
            Some("cannotCalculateChanges"),
            "{since} {max}"
        );
    }
    let exact = rig.mail(json!([changes("0", Some(6))]));
    let answer = &first(&exact)[1];
    assert_eq!(lengths(answer), (6, 0, 0));
    assert_eq!(
        (&answer["newState"], &answer["hasMoreChanges"]),
        (&json!("2"), &json!(false))
    );
    let rest = rig.mail(json!([changes("1", Some(3))]));
    let answer = &first(&rest)[1];
    assert_eq!(lengths(answer), (1, 1, 1));
    assert_eq!(
        (&answer["newState"], &answer["hasMoreChanges"]),
        (&json!("2"), &json!(false))
    );
}

#[tokio::test]
async fn a_state_the_log_cannot_calculate_from_says_so() {
    let rig = Rig::start().await;
    for since in ["9", "abc", "-1", "+1", "01", "007", "18446744073709551615"] {
        let response = rig.mail(json!([changes(since, None)]));
        assert_eq!(
            error_type(first(&response)),
            Some("cannotCalculateChanges"),
            "{since}"
        );
    }
    let zero = rig.mail(json!([changes("1", Some(0))]));
    assert_eq!(error_type(first(&zero)), Some("invalidArguments"));
}

#[tokio::test]
async fn a_reference_chains_changes_into_get_and_a_broken_one_is_refused_rfc8620_3_7() {
    let rig = Rig::start().await;
    rig.edit();
    rig.pass().await;
    let chained = rig.mail(json!([
        changes("1", None),
        [
            "Mailbox/get",
            {
                "accountId": ACCOUNT,
                "#ids": { "resultOf": "c1", "name": "Mailbox/changes", "path": "/created" },
                "properties": ["name"]
            },
            "c2"
        ]
    ]));
    let got = &chained["methodResponses"][1];
    assert_eq!(got[0], "Mailbox/get");
    assert_eq!(
        got[1]["list"],
        json!([{ "id": rig.id_of("Work"), "name": "Work" }])
    );
    let broken = rig.mail(json!([
        changes("1", None),
        [
            "Mailbox/get",
            {
                "accountId": ACCOUNT,
                "#ids": { "resultOf": "c1", "name": "Mailbox/changes", "path": "/nope" }
            },
            "c2"
        ]
    ]));
    assert_eq!(
        error_type(&broken["methodResponses"][1]),
        Some("invalidResultReference")
    );
    let both = rig.mail(json!([
        changes("1", None),
        [
            "Mailbox/get",
            {
                "accountId": ACCOUNT,
                "ids": [],
                "#ids": { "resultOf": "c1", "name": "Mailbox/changes", "path": "/created" }
            },
            "c2"
        ]
    ]));
    assert_eq!(
        error_type(&both["methodResponses"][1]),
        Some("invalidArguments")
    );
    let plain = rig.mail(json!([mailbox_get("c1")]));
    assert_eq!(first(&plain)[1]["list"].as_array().unwrap().len(), 6);
}
