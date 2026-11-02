// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! `Mailbox/set` of a name Gmail refuses on every try. White space in a row
//! and a whole path past its limits answer `invalidProperties` on `name` with
//! no folder command; a mark it reads as a space waits for its NO and the
//! pass (RFC 8621 section 2.5). Every other server takes these names.

use huliho_imap_bridge::testing::{Folder, Mailboxes, Message};
use serde_json::{Value, json};

use crate::folders_rig::{dovecot, id_of, logins, name_taken, refused, sent, set, started};
use crate::sync_rig::Rig;

/// The UTF-16 code units of a whole path Gmail takes.
const MAX_PATH_UNITS: usize = 225;

/// The bytes of UTF-8 of a whole path Gmail takes.
const MAX_PATH_BYTES: usize = 255;

/// A parent and its delimiter on the wire.
const UNDER_WORK: &str = "Work/";

/// An e with an acute accent, two bytes of UTF-8 and one UTF-16 unit.
const ACCENT: &str = "\u{e9}";

/// The bytes `ACCENT` takes.
const ACCENT_BYTES: usize = 2;

/// How often each name is tried: the rows do not read marks as a space, so
/// every try sends the command.
const TRIES: usize = 2;

/// A Gmail account with one message under the label `Work` and these labels
/// beside it.
fn gmail(labels: &[&str]) -> Mailboxes {
    let mail = vec![Message::new(1).labeled(&["Work"])];
    let mailboxes = Mailboxes::gmail(mail).with_label("Work");
    for label in labels {
        mailboxes.push(Folder::labeled(label, label));
    }
    mailboxes
}

fn create(name: &str) -> Value {
    json!({ "create": { "k": { "name": name } } })
}

/// Abuse cases first: white space in a row, by a create, under a parent and by
/// a rename, in any mix of kinds. No command goes out, so nothing signs in.
#[tokio::test]
async fn white_space_in_a_row_answers_invalid_properties_with_nothing_sent_on_gmail_rfc8621_2_5() {
    let rig = started(gmail(&["Other"]), true).await;
    let (work, other) = (id_of(&rig, "Work"), id_of(&rig, "Other"));
    let signed_in = logins(&rig);
    for typed in [
        "a  b",
        "a   b",
        "a\u{a0}\u{a0}b",
        "a \u{3000}b",
        "a\u{2028} b",
    ] {
        let answer = set(&rig, create(typed)).await;
        assert_eq!(answer["notCreated"]["k"], name_taken(), "{answer}");
    }
    let call = json!({ "create": { "k": { "name": "q  3", "parentId": work } } });
    let answer = set(&rig, call).await;
    assert_eq!(answer["notCreated"]["k"], name_taken(), "{answer}");
    let call = json!({ "update": { (&other): { "name": "x  y" } } });
    let answer = set(&rig, call).await;
    assert_eq!(answer["notUpdated"][&other], name_taken(), "{answer}");
    assert_eq!(sent(&rig), Vec::<String>::new());
    assert_eq!(logins(&rig), signed_in, "the rows answer");
}

/// White space that stands alone and what only looks like a space go to Gmail,
/// which takes them.
#[tokio::test]
async fn white_space_alone_and_a_look_alike_go_to_gmail_rfc8621_2_5() {
    let rig = started(gmail(&[]), true).await;
    for (typed, wire) in [
        ("a b c", "a b c"),
        ("d\u{a0}e", "d&AKA-e"),
        ("f\u{200b}\u{200b}g", "f&IAsgCw-g"),
        ("h \u{200b} i", "h &IAs- i"),
    ] {
        let answer = set(&rig, create(typed)).await;
        assert_eq!(answer["created"]["k"]["id"], id_of(&rig, wire), "{answer}");
    }
}

/// Before an account's first pass no row says it is a Gmail account, so the
/// command goes out once; the pass after its NO answers the next try.
#[tokio::test]
async fn white_space_in_a_row_before_the_first_pass_is_refused_once_by_the_server() {
    let mut rig = Rig::over(gmail(&[])).await;
    rig.cache.gmail = true;
    let call = create("a  b");
    let answer = set(&rig, call.clone()).await;
    assert_eq!(refused(&answer, "notCreated", "k"), "serverFail");
    assert_eq!(sent(&rig), ["CREATE \"a  b\""]);
    let again = set(&rig, call).await;
    assert_eq!(again["notCreated"]["k"], name_taken(), "{again}");
    assert_eq!(sent(&rig), ["CREATE \"a  b\""], "the rows answer");
}

/// A whole path past either limit answers from the rows and the listing, by a
/// create, under a parent and by a rename or a move. The objects after it run:
/// the session stands.
#[tokio::test]
async fn a_whole_path_past_what_gmail_takes_answers_invalid_properties_with_no_folder_command_rfc8621_2_5()
 {
    let rig = started(gmail(&["Other"]), true).await;
    let (work, other) = (id_of(&rig, "Work"), id_of(&rig, "Other"));
    let past_units = "x".repeat(MAX_PATH_UNITS + 1);
    let leaf_room = MAX_PATH_UNITS - UNDER_WORK.len();
    let units_under = "y".repeat(leaf_room + 1);
    let bytes_room = (MAX_PATH_BYTES - UNDER_WORK.len()) / ACCENT_BYTES;
    let bytes_under = ACCENT.repeat(bytes_room + 1);
    let answer = set(
        &rig,
        json!({ "create": {
            "top": { "name": past_units },
            "units": { "name": units_under, "parentId": work },
            "bytes": { "name": bytes_under, "parentId": work },
            "fits": { "name": "Fits" },
        } }),
    )
    .await;
    for key in ["top", "units", "bytes"] {
        assert_eq!(answer["notCreated"][key], name_taken(), "{answer}");
    }
    assert_eq!(answer["created"]["fits"]["id"], id_of(&rig, "Fits"));
    assert_eq!(sent(&rig), ["CREATE \"Fits\"", "SUBSCRIBE \"Fits\""]);

    let long = "z".repeat(leaf_room + 1);
    let named = json!({ "create": { "k": { "name": long } } });
    let made = set(&rig, named).await;
    let id = made["created"]["k"]["id"].as_str().unwrap().to_owned();
    let call = json!({ "update": { (&other): { "name": "x".repeat(MAX_PATH_UNITS + 1) } } });
    let answer = set(&rig, call).await;
    assert_eq!(answer["notUpdated"][&other], name_taken(), "{answer}");
    let call = json!({ "update": { (&id): { "parentId": work } } });
    let answer = set(&rig, call).await;
    assert_eq!(answer["notUpdated"][&id], name_taken(), "a move: {answer}");
    let renames = sent(&rig);
    let renames = renames.iter().filter(|line| line.starts_with("RENAME "));
    assert_eq!(renames.count(), 0);
}

/// A path that stands at both limits goes out and lands.
#[tokio::test]
async fn a_whole_path_at_what_gmail_takes_goes_out_and_lands_rfc8621_2_5() {
    let rig = started(gmail(&[]), true).await;
    let work = id_of(&rig, "Work");
    let at_units = "x".repeat(MAX_PATH_UNITS);
    let answer = set(&rig, create(&at_units)).await;
    assert_eq!(answer["created"]["k"]["id"], id_of(&rig, &at_units));
    let leaf_room = MAX_PATH_UNITS - UNDER_WORK.len();
    let units_under = "y".repeat(leaf_room);
    let call = json!({ "create": { "k": { "name": units_under, "parentId": work } } });
    let answer = set(&rig, call).await;
    let wire = format!("{UNDER_WORK}{units_under}");
    assert_eq!(answer["created"]["k"]["id"], id_of(&rig, &wire), "{answer}");
    let at_bytes = ACCENT.repeat((MAX_PATH_BYTES - UNDER_WORK.len()) / ACCENT_BYTES);
    let call = json!({ "create": { "k": { "name": at_bytes, "parentId": work } } });
    let answer = set(&rig, call).await;
    assert_eq!(answer["notCreated"], json!(null), "{answer}");
}

/// A mark Gmail reads as a space, by a create and by a rename: the rows do not
/// read it so, the command goes out on every try and its NO and the pass answer
/// as the rows would have.
#[tokio::test]
async fn a_mark_gmail_reads_as_a_space_answers_invalid_properties_after_the_no_rfc8621_2_5() {
    let rig = started(gmail(&["a b", "Other"]), true).await;
    let other = id_of(&rig, "Other");
    let names = ["a-b", "A&B", "a(b", "a|b", "a\u{ff0d}b"];
    for typed in names {
        for _try in 0..TRIES {
            let answer = set(&rig, create(typed)).await;
            assert_eq!(answer["notCreated"]["k"], name_taken(), "{typed}: {answer}");
        }
    }
    let creates = sent(&rig);
    assert_eq!(creates.len(), names.len() * TRIES, "{creates:?}");
    let call = json!({ "update": { (&other): { "name": "a-b" } } });
    let answer = set(&rig, call).await;
    assert_eq!(answer["notUpdated"][&other], name_taken(), "{answer}");
    assert_eq!(id_of(&rig, "Other"), other);
}

/// What Gmail takes beside a label stays a create: a mark it does not read as
/// a space and marks that do not stand alone between the letters.
#[tokio::test]
async fn a_mark_gmail_takes_beside_a_label_goes_out_and_lands() {
    let rig = started(gmail(&["a b", "x-y"]), true).await;
    for typed in ["a+b", "a_b", "ab", "x - y", "x--y"] {
        let answer = set(&rig, create(typed)).await;
        assert_eq!(answer["created"]["k"]["id"], id_of(&rig, typed), "{answer}");
    }
}

/// A rename onto a form of its own name that Gmail reads as one stays a rename.
#[tokio::test]
async fn a_rename_onto_a_form_of_its_own_name_stays_a_rename_on_gmail_rfc3501_6_3_5() {
    let rig = started(gmail(&["p q"]), true).await;
    let id = id_of(&rig, "p q");
    let call = json!({ "update": { (&id): { "name": "p-q" } } });
    let answer = set(&rig, call).await;
    assert_eq!(answer["notUpdated"], json!(null), "{answer}");
    assert_eq!(id_of(&rig, "p-q"), id);
}

/// A server that takes these names gets the command, and a refusal there says
/// nothing about the name.
#[tokio::test]
async fn a_server_that_takes_these_names_gets_the_command() {
    let rig = started(dovecot(), false).await;
    let long = "x".repeat(MAX_PATH_UNITS + 1);
    for typed in ["a  b", "a-b", long.as_str()] {
        let answer = set(&rig, create(typed)).await;
        assert_eq!(answer["created"]["k"]["id"], id_of(&rig, typed), "{answer}");
    }
    let answer = set(&rig, create("a b")).await;
    assert_eq!(answer["created"]["k"]["id"], id_of(&rig, "a b"), "{answer}");

    let mut mailboxes = dovecot();
    mailboxes.behavior.folder_refusal = Some("NOPERM");
    mailboxes.push(Folder::new("a b"));
    let rig = started(mailboxes, false).await;
    let answer = set(&rig, create("a-b")).await;
    assert_eq!(refused(&answer, "notCreated", "k"), "serverFail");
}
