// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! `Mailbox/set` of a name that differs in case from a label on a Gmail
//! account, which Gmail refuses on every try. A difference in ASCII case
//! answers `invalidProperties` on `name` from the rows; any other waits for
//! Gmail's NO and the pass after it, since Gmail takes some names a Unicode
//! fold reads as one (RFC 8621 section 2.5).

use huliho_imap_bridge::testing::{Folder, Mailboxes, Message};
use serde_json::json;

use crate::folders_rig::{dovecot, id_of, logins, name_taken, refused, rows, sent, set, started};
use crate::sync_rig::Rig;

/// `CAF` and a capital e with an acute accent, on the wire.
const CAFE_CAPITAL: &str = "CAF&AMk-";

/// `Papka` in Cyrillic, on the wire.
const PAPKA_CAPITAL: &str = "&BB8EMAQ,BDoEMA-";

/// A full-width capital A, on the wire.
const WIDE_A: &str = "&,yE-";

/// A label holding a Greek capital sigma, on the wire.
const SIGMA_CAPITAL: &str = "Greek &A6M-";

/// A label holding a capital sharp s, on the wire.
const SHARP_CAPITAL: &str = "Street &Hp4-";

/// How often each name is tried: the rows do not fold outside ASCII, so
/// every try sends the command.
const TRIES: usize = 2;

/// A Gmail account with one message under the label `Work` and these
/// labels beside it.
fn gmail(labels: &[&str]) -> Mailboxes {
    let mail = vec![Message::new(1).labeled(&["Work"])];
    let mailboxes = Mailboxes::gmail(mail).with_label("Work");
    for label in labels {
        mailboxes.push(Folder::labeled(label, label));
    }
    mailboxes
}

/// Before an account's first pass no row says it is a Gmail account. The
/// pass after Gmail's NO does, so the name reads as on any other try.
#[tokio::test]
async fn a_name_refused_before_the_first_pass_reads_as_that_pass_leaves_the_account() {
    let mut rig = Rig::over(gmail(&[])).await;
    rig.cache.gmail = true;
    let answer = set(&rig, json!({ "create": { "k": { "name": "work" } } })).await;
    assert_eq!(answer["notCreated"]["k"], name_taken(), "{answer}");
    assert_eq!(sent(&rig), ["CREATE \"work\""]);
}

/// A difference in ASCII case is refused from the rows, by a create, a
/// rename and under a parent, with letters outside ASCII held equal. No
/// command goes out, so nothing signs in again.
#[tokio::test]
async fn a_label_in_another_ascii_case_answers_invalid_properties_with_nothing_sent_rfc8621_2_5() {
    let rig = started(gmail(&["Other", "Work/Q3", CAFE_CAPITAL]), true).await;
    let (work, other) = (id_of(&rig, "Work"), id_of(&rig, "Other"));
    let signed_in = logins(&rig);
    let call = json!({ "create": {
        "a": { "name": "work" },
        "b": { "name": "q3", "parentId": work },
        "c": { "name": "caf\u{c9}" },
    } });
    let answer = set(&rig, call).await;
    for key in ["a", "b", "c"] {
        assert_eq!(answer["notCreated"][key], name_taken(), "{answer}");
    }
    let call = json!({ "update": { (&other): { "name": "WORK" } } });
    let answer = set(&rig, call).await;
    assert_eq!(answer["notUpdated"][&other], name_taken(), "{answer}");
    assert_eq!(sent(&rig), Vec::<String>::new());
    assert_eq!(logins(&rig), signed_in, "the rows answer");
}

/// The creates of one call read each other as they read the rows.
#[tokio::test]
async fn a_second_create_that_differs_in_case_answers_beside_the_first_rfc8620_5_3() {
    let rig = started(gmail(&[]), true).await;
    let call = json!({ "create": { "a": { "name": "Plan" }, "b": { "name": "plan" } } });
    let answer = set(&rig, call).await;
    assert_eq!(answer["created"]["a"]["id"], id_of(&rig, "Plan"));
    assert_eq!(answer["notCreated"]["b"], name_taken(), "{answer}");
    assert_eq!(sent(&rig), ["CREATE \"Plan\"", "SUBSCRIBE \"Plan\""]);
}

/// Gmail takes a small sigma beside its capital and a sharp s beside its
/// capital, which a Unicode fold reads as one name, so these still go out
/// and land.
#[tokio::test]
async fn a_name_gmail_takes_beside_one_a_unicode_fold_reads_the_same_goes_to_the_server() {
    let rig = started(gmail(&[SIGMA_CAPITAL, SHARP_CAPITAL]), true).await;
    let call = json!({ "create": {
        "a": { "name": "Greek \u{3c3}" },
        "b": { "name": "Street \u{df}" },
    } });
    let answer = set(&rig, call).await;
    assert_eq!(answer["created"]["a"]["id"], id_of(&rig, "Greek &A8M-"));
    assert_eq!(answer["created"]["b"]["id"], id_of(&rig, "Street &AN8-"));
}

/// Outside ASCII the rows do not fold, so the command goes out on every
/// try; Gmail's NO and the pass after it answer as the rows would have.
/// Its NO to a RENAME names the name in raw UTF-8, which fails the parse.
#[tokio::test]
async fn a_name_outside_ascii_that_differs_in_case_answers_invalid_properties_after_the_no_rfc8621_2_5()
 {
    let labels = ["Other", CAFE_CAPITAL, PAPKA_CAPITAL, WIDE_A];
    let rig = started(gmail(&labels), true).await;
    let other = id_of(&rig, "Other");
    let names = [
        "CAF\u{e9}",
        "\u{43f}\u{430}\u{43f}\u{43a}\u{430}",
        "a",
        "\u{ff41}",
    ];
    for typed in names {
        let call = json!({ "create": { "k": { "name": typed } } });
        for _try in 0..TRIES {
            let answer = set(&rig, call.clone()).await;
            assert_eq!(answer["notCreated"]["k"], name_taken(), "{typed}: {answer}");
        }
    }
    let creates = sent(&rig);
    assert_eq!(creates.len(), names.len() * TRIES, "{creates:?}");
    let call = json!({ "update": { (&other): { "name": "CAF\u{e9}" } } });
    let answer = set(&rig, call).await;
    assert_eq!(answer["notUpdated"][&other], name_taken(), "{answer}");
    assert_eq!(id_of(&rig, "Other"), other);
}

/// A label another client made in another case since the last pass: the
/// rows do not know it, so the command goes out, the pass shows it and the
/// next try is answered from the rows.
#[tokio::test]
async fn a_label_another_client_made_in_another_case_answers_invalid_properties_after_the_pass() {
    let rig = started(gmail(&[]), true).await;
    let signed_in = logins(&rig);
    rig.fake
        .script()
        .mailboxes
        .push(Folder::labeled("Taken", "Taken"));
    let call = json!({ "create": { "k": { "name": "taken" } } });
    let answer = set(&rig, call.clone()).await;
    assert_eq!(answer["notCreated"]["k"], name_taken(), "{answer}");
    assert_eq!(sent(&rig), ["CREATE \"taken\""]);
    assert_eq!(
        logins(&rig),
        signed_in + 2,
        "the call and the pass after it"
    );
    let again = set(&rig, call).await;
    assert_eq!(again["notCreated"]["k"], name_taken(), "{again}");
    assert_eq!(sent(&rig), ["CREATE \"taken\""], "the rows answer");
}

/// A rename that changes the case alone is not a name beside the folder.
/// Gmail takes it inside ASCII and outside it. The id stays (RFC 3501
/// section 6.3.5).
#[tokio::test]
async fn a_rename_that_changes_the_case_alone_keeps_the_id_on_gmail_rfc3501_6_3_5() {
    let rig = started(gmail(&[CAFE_CAPITAL]), true).await;
    let (work, cafe) = (id_of(&rig, "Work"), id_of(&rig, CAFE_CAPITAL));
    let call = json!({ "update": {
        (&work): { "name": "work" },
        (&cafe): { "name": "CAF\u{e9}" },
    } });
    let answer = set(&rig, call).await;
    assert_eq!(answer["notUpdated"], json!(null), "{answer}");
    assert_eq!(id_of(&rig, "work"), work);
    assert_eq!(id_of(&rig, "CAF&AOk-"), cafe);
    let row = rig.folder("work");
    assert_eq!(row.facts.gmail_label.as_deref(), Some("work"));
    let commands = sent(&rig);
    let of_work = commands.iter().filter(|line| line.contains("ork\""));
    assert_eq!(
        of_work.collect::<Vec<_>>(),
        [
            "RENAME \"Work\" \"work\"",
            "SUBSCRIBE \"work\"",
            "UNSUBSCRIBE \"Work\""
        ]
    );
}

/// A refused rename of the case alone stays a refusal: the mailbox's own
/// row is no other client's name.
#[tokio::test]
async fn a_refused_rename_of_the_case_alone_stays_server_fail() {
    let mut mailboxes = gmail(&[]);
    mailboxes.behavior.folder_refusal = Some("NOPERM");
    let rig = started(mailboxes, true).await;
    let work = id_of(&rig, "Work");
    let call = json!({ "update": { (&work): { "name": "work" } } });
    let answer = set(&rig, call).await;
    assert_eq!(refused(&answer, "notUpdated", &work), "serverFail");
    assert_eq!(id_of(&rig, "Work"), work);
}

/// A refusal for another reason stays a refusal: the same letters under
/// another parent and letters that differ.
#[tokio::test]
async fn a_refusal_with_the_name_under_another_parent_or_nowhere_stays_server_fail() {
    let mut mailboxes = gmail(&["Work/Q3"]);
    mailboxes.behavior.folder_refusal = Some("NOPERM");
    let rig = started(mailboxes, true).await;
    let call = json!({ "create": {
        "elsewhere": { "name": "Q3" },
        "nowhere": { "name": "New" },
    } });
    let answer = set(&rig, call).await;
    assert_eq!(refused(&answer, "notCreated", "elsewhere"), "serverFail");
    assert_eq!(refused(&answer, "notCreated", "nowhere"), "serverFail");
}

/// A server that takes both cases gets the command and a refusal there says
/// nothing about the name.
#[tokio::test]
async fn a_server_that_takes_both_cases_gets_the_command() {
    let rig = started(dovecot(), false).await;
    let answer = set(&rig, json!({ "create": { "k": { "name": "work" } } })).await;
    assert_eq!(
        answer["created"]["k"]["id"],
        id_of(&rig, "work"),
        "{answer}"
    );
    assert_eq!(sent(&rig), ["CREATE \"work\"", "SUBSCRIBE \"work\""]);
    let names: Vec<String> = rows(&rig)
        .into_iter()
        .map(|row| row.facts.imap_name)
        .collect();
    assert!(names.contains(&"Work".to_owned()), "{names:?}");

    let mut mailboxes = dovecot();
    mailboxes.behavior.folder_refusal = Some("NOPERM");
    let rig = started(mailboxes, false).await;
    let answer = set(&rig, json!({ "create": { "k": { "name": "work" } } })).await;
    assert_eq!(refused(&answer, "notCreated", "k"), "serverFail");
}
