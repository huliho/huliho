// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! `Mailbox/set` onto a name another client took since the last pass: the
//! server refuses the CREATE or the RENAME, a pass shows the name and the
//! object answers `invalidProperties` on `name`, as the rows would have and
//! as a JMAP server does (RFC 8621 section 2.5). Any other refusal stays
//! `serverFail`, and so does one no pass could follow.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use huliho_imap_bridge::jmap::{CORE_CAPABILITY, MAIL_CAPABILITY, handle};
use huliho_imap_bridge::runtime::{ConnectError, Connector, Link};
use huliho_imap_bridge::session::{ImapSession, TlsMode};
use huliho_imap_bridge::store::AccountKey;
use huliho_imap_bridge::testing::imap::HOST;
use huliho_imap_bridge::testing::{Folder, Mailboxes, Message, Renaming, TestConnector};
use serde_json::{Value, json};

use crate::folders_rig::{dovecot, id_of, logins, refused, rows, sent, set, started, state};
use crate::sync_rig::ACCOUNT;

/// Room for a loopback exchange.
const STEP: Duration = Duration::from_secs(2);

/// What an object answers for a name that stands.
fn name_taken() -> Value {
    json!({ "type": "invalidProperties", "properties": ["name"] })
}

/// A CREATE the server refuses because the name stands: the pass writes
/// the row and the answer's state is the one after it. The next try is
/// answered from the rows with nothing sent.
#[tokio::test]
async fn a_create_of_a_name_another_client_took_answers_invalid_properties_rfc8621_2_5() {
    let rig = started(dovecot(), false).await;
    let before = state(&rig);
    let signed_in = logins(&rig);
    rig.fake.script().mailboxes.push(Folder::new("Taken"));
    let call = json!({ "create": { "k": { "name": "Taken" } } });
    let answer = set(&rig, call.clone()).await;
    assert_eq!(answer["notCreated"]["k"], name_taken(), "{answer}");
    assert_eq!(answer["created"], json!(null), "{answer}");
    assert_eq!(answer["oldState"], before.to_string());
    assert_eq!(answer["newState"], state(&rig).to_string());
    assert_ne!(state(&rig), before);
    assert_eq!(sent(&rig), ["CREATE \"Taken\""]);
    assert_eq!(
        logins(&rig),
        signed_in + 2,
        "the call and the pass after it"
    );
    let again = set(&rig, call).await;
    assert_eq!(again["notCreated"]["k"], name_taken(), "{again}");
    assert_eq!(sent(&rig), ["CREATE \"Taken\""], "the rows answer");
}

/// The name is read as the rows read it: under its parent, in NFC, from
/// the wire's modified UTF-7 (RFC 3501 section 5.1.3).
#[tokio::test]
async fn a_name_under_a_parent_and_outside_ascii_is_read_as_the_rows_read_it_rfc3501_5_1_3() {
    let rig = started(dovecot(), false).await;
    let work = id_of(&rig, "Work");
    let wire = "Work/Caf&AOk-";
    rig.fake.script().mailboxes.push(Folder::new(wire));
    let call = json!({ "create": { "k": { "name": "Cafe\u{301}", "parentId": work } } });
    let answer = set(&rig, call).await;
    assert_eq!(answer["notCreated"]["k"], name_taken(), "{answer}");
    assert_eq!(sent(&rig), [format!("CREATE \"{wire}\"")]);
}

/// A server that normalizes names refuses the NFC form of a name it lists
/// in NFD; the rows read both in NFC.
#[tokio::test]
async fn a_name_the_server_lists_in_another_normal_form_reads_as_taken() {
    let mut mailboxes = dovecot();
    mailboxes.behavior.folder_refusal = Some("ALREADYEXISTS");
    let rig = started(mailboxes, false).await;
    rig.fake.script().mailboxes.push(Folder::new("Cafe&AwE-"));
    let call = json!({ "create": { "k": { "name": "Caf\u{e9}" } } });
    let answer = set(&rig, call).await;
    assert_eq!(answer["notCreated"]["k"], name_taken(), "{answer}");
}

/// A RENAME onto a name that stands, by a new name or by a new parent:
/// the mailbox keeps its id and its place (RFC 3501 section 6.3.5).
#[tokio::test]
async fn a_rename_or_a_move_onto_a_name_another_client_took_answers_invalid_properties_rfc8621_2_5()
{
    let rig = started(dovecot(), false).await;
    let (work, q3) = (id_of(&rig, "Work"), id_of(&rig, "Work/Q3"));
    rig.fake.script().mailboxes.push(Folder::new("Job"));
    let renamed = set(&rig, json!({ "update": { (&work): { "name": "Job" } } })).await;
    assert_eq!(renamed["notUpdated"][&work], name_taken(), "{renamed}");
    rig.fake.script().mailboxes.push(Folder::new("Q3"));
    let moved = set(&rig, json!({ "update": { (&q3): { "parentId": null } } })).await;
    assert_eq!(moved["notUpdated"][&q3], name_taken(), "{moved}");
    assert_eq!(id_of(&rig, "Work"), work);
    assert_eq!(id_of(&rig, "Work/Q3"), q3);
    assert_eq!(
        sent(&rig),
        ["RENAME \"Work\" \"Job\"", "RENAME \"Work/Q3\" \"Q3\""]
    );
}

/// A server may move a mailbox and still answer NO, as one that renames a
/// tree one mailbox at a time and stops on the way: the name stands because
/// the mailbox went there, so no other client took it.
#[tokio::test]
async fn a_rename_that_moved_its_mailbox_before_the_no_stays_server_fail() {
    let mut mailboxes = dovecot();
    mailboxes.behavior.renaming = Renaming::StopsPartway;
    let rig = started(mailboxes, false).await;
    let work = id_of(&rig, "Work");
    let answer = set(&rig, json!({ "update": { (&work): { "name": "Job" } } })).await;
    assert_eq!(refused(&answer, "notUpdated", &work), "serverFail");
    let names: Vec<String> = rows(&rig)
        .into_iter()
        .map(|row| row.facts.imap_name)
        .collect();
    assert!(names.contains(&"Job".to_owned()), "{names:?}");
}

/// A NO for any other reason: the pass runs and the answer stays, for a
/// name that stands under another parent and for one that stands nowhere.
#[tokio::test]
async fn a_name_that_stands_elsewhere_or_nowhere_stays_server_fail() {
    let mut mailboxes = dovecot();
    mailboxes.behavior.folder_refusal = Some("NOPERM");
    let rig = started(mailboxes, false).await;
    let work = id_of(&rig, "Work");
    rig.fake.script().mailboxes.push(Folder::new("Elsewhere"));
    let call = json!({ "create": {
        "under": { "name": "Elsewhere", "parentId": work },
        "nowhere": { "name": "New" },
    } });
    let answer = set(&rig, call).await;
    assert_eq!(refused(&answer, "notCreated", "under"), "serverFail");
    assert_eq!(refused(&answer, "notCreated", "nowhere"), "serverFail");
    let names: Vec<String> = rows(&rig)
        .into_iter()
        .map(|row| row.facts.imap_name)
        .collect();
    assert!(names.contains(&"Elsewhere".to_owned()), "{names:?}");
}

/// Every other object of the call ends as it would: a child of the create
/// the server refused names no parent and a create beside it lands, with
/// its id through the pass (RFC 8620 section 5.3).
#[tokio::test]
async fn the_objects_beside_a_taken_name_end_as_they_would_rfc8620_5_3() {
    let rig = started(dovecot(), false).await;
    rig.fake.script().mailboxes.push(Folder::new("Taken"));
    let call = json!({ "create": {
        "a": { "name": "Taken" },
        "b": { "name": "Below", "parentId": "#a" },
        "c": { "name": "Free" },
    } });
    let answer = set(&rig, call).await;
    assert_eq!(answer["notCreated"]["a"], name_taken(), "{answer}");
    assert_eq!(refused(&answer, "notCreated", "b"), "invalidProperties");
    assert_eq!(answer["notCreated"]["b"]["properties"], json!(["parentId"]));
    assert_eq!(
        answer["created"]["c"]["id"],
        id_of(&rig, "Free"),
        "{answer}"
    );
    assert_eq!(
        sent(&rig),
        ["CREATE \"Taken\"", "CREATE \"Free\"", "SUBSCRIBE \"Free\""]
    );
}

/// Gmail names the label in raw UTF-8 in its NO, which the parser refuses,
/// so the answer reads as a failure of the session: the name still decides
/// (RFC 3501 section 7.1). A label made elsewhere shows only on a fresh
/// connection, which the pass is.
#[tokio::test]
async fn a_label_another_client_took_answers_invalid_properties_though_gmail_names_it_in_raw_utf8()
{
    let mail = vec![Message::new(1).labeled(&["Work"])];
    let rig = started(Mailboxes::gmail(mail).with_label("Work"), true).await;
    let work = id_of(&rig, "Work");
    let model = &rig.fake.script().mailboxes;
    model.push(Folder::labeled("Plan &APw-", "Plan &APw-"));
    model.push(Folder::labeled("Job &APY-", "Job &APY-"));
    let created = set(
        &rig,
        json!({ "create": { "k": { "name": "Plan \u{fc}" } } }),
    )
    .await;
    assert_eq!(created["notCreated"]["k"], name_taken(), "{created}");
    let renamed = set(
        &rig,
        json!({ "update": { (&work): { "name": "Job \u{f6}" } } }),
    )
    .await;
    assert_eq!(renamed["notUpdated"][&work], name_taken(), "{renamed}");
    assert_eq!(id_of(&rig, "Work"), work);
}

/// Signs in once, then refuses every connection.
#[derive(Clone)]
struct SignsInOnce {
    server: TestConnector,
    used: Arc<AtomicBool>,
}

impl Connector for SignsInOnce {
    type Session = ImapSession;

    async fn connect(&self, key: &AccountKey) -> Result<ImapSession, ConnectError> {
        if self.used.swap(true, Ordering::SeqCst) {
            return TestConnector::Refusing.connect(key).await;
        }
        self.server.connect(key).await
    }
}

/// No pass could run, so the rows know nothing new and the answer stays.
#[tokio::test]
async fn a_refusal_no_pass_could_follow_stays_server_fail() {
    let rig = started(dovecot(), false).await;
    rig.fake.script().mailboxes.push(Folder::new("Taken"));
    let server = TestConnector::scripted(
        rig.fake.trusting(),
        rig.fake.target(HOST, TlsMode::Implicit),
        STEP,
    );
    let connector = SignsInOnce {
        server,
        used: Arc::new(AtomicBool::new(false)),
    };
    let link = Link::with_interval(connector, Duration::ZERO);
    let arguments = json!({ "accountId": ACCOUNT, "create": { "k": { "name": "Taken" } } });
    let body = json!({
        "using": [CORE_CAPABILITY, MAIL_CAPABILITY],
        "methodCalls": [["Mailbox/set", arguments, "c1"]],
    });
    let bytes = serde_json::to_vec(&body).unwrap();
    let answered = handle(&rig.cache, &link, &bytes, ACCOUNT).await.unwrap();
    let answered: Value = serde_json::from_slice(&answered).unwrap();
    let answer = &answered["methodResponses"][0][1];
    assert_eq!(refused(answer, "notCreated", "k"), "serverFail");
    assert!(rows(&rig).iter().all(|row| row.facts.imap_name != "Taken"));
}

/// A refused SUBSCRIBE shows nothing a pass would add, after a CREATE the
/// server took and on an update of its own: the folder is a row already.
#[tokio::test]
async fn a_refused_subscribe_runs_no_pass() {
    let mut mailboxes = Mailboxes::dovecot();
    mailboxes.behavior.subscribe_refusal = Some("NOPERM");
    let rig = started(mailboxes, false).await;
    let signed_in = logins(&rig);
    let answer = set(&rig, json!({ "create": { "k": { "name": "New" } } })).await;
    let id = answer["created"]["k"]["id"].as_str().unwrap().to_owned();
    assert_eq!(logins(&rig), signed_in + 1, "the call alone");
    let shown = json!({ "update": { (&id): { "isSubscribed": true } } });
    let answer = set(&rig, shown).await;
    assert_eq!(refused(&answer, "notUpdated", &id), "serverFail");
    assert_eq!(logins(&rig), signed_in + 2, "the next call alone");
}
