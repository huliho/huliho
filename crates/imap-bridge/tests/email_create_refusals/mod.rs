// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What a draft is refused for: from the rows and the object with no
//! command sent, a NO after the literal with no row, a host out of reach
//! and a blob the account does not hold (RFC 8621 section 4.6). A destroy
//! that waits on such a create stays undone, so a failed save loses no
//! draft.

use std::sync::Arc;
use std::time::Duration;

use huliho_imap_bridge::runtime::{Link, Timing};
use huliho_imap_bridge::session::TlsMode;
use huliho_imap_bridge::store::EmailId;
use huliho_imap_bridge::testing::command::Continuation;
use huliho_imap_bridge::testing::imap::HOST;
use huliho_imap_bridge::testing::parts::corpus;
use huliho_imap_bridge::testing::{Extension, Folder, Message, MyRights, Storing, TestConnector};
use mail_parser::{MessageParser, MimeHeaders};
use serde_json::{Value, json};

use crate::draft_rig::{
    DRAFTS, INBOX, account, appends, create, created, drafts, held, object, refused, replace,
    sized, synced, without,
};
use crate::set_rig::{Set, started};
use crate::sync_rig::inbox;

/// The UID of the message whose parts the blobs name.
const SOURCE: u32 = 7;

/// A deadline a server that never answers runs past at once.
const SHORT_DEADLINE: Duration = Duration::from_millis(100);

/// A limit far under any message.
const TINY_LIMIT: u64 = 64;

/// A limit each of two parts of the corpus fits and the two together pass:
/// the first holds 6 bytes, the second states 90.
const BETWEEN_TWO_PARTS: u64 = 95;

/// A size past what a message may hold, in the text of a BODYSTRUCTURE.
const PAST_THE_BOUND: &str = "99999999";

#[tokio::test]
async fn a_refused_mailbox_names_it_and_sends_nothing_rfc8621_4_6() {
    let set = synced(account(Extension::all())).await;
    for mailbox in ["m-unknown", "#nothing"] {
        let answer = create(&set, object(mailbox)).await;
        assert_eq!(refused(&answer), "invalidProperties", "{answer}");
        let properties = &answer["notCreated"]["d"]["properties"];
        assert_eq!(*properties, json!(["mailboxIds"]), "{answer}");
    }
    assert_eq!(appends(&set), Vec::<String>::new());
}

#[tokio::test]
async fn a_folder_nothing_can_select_takes_no_draft_rfc8621_2() {
    let mailboxes = account(Extension::all());
    mailboxes.push(Folder::noselect("Parent"));
    let set = synced(mailboxes).await;
    let parent = set.rig.folder("Parent").id.to_string();
    let answer = create(&set, object(&parent)).await;
    assert_eq!(refused(&answer), "invalidProperties", "{answer}");
    assert_eq!(appends(&set), Vec::<String>::new());
}

#[tokio::test]
async fn a_mailbox_whose_rights_lack_insert_is_forbidden_rfc4314_4() {
    let mut mailboxes = account(Extension::all());
    mailboxes.extensions.insert(Extension::Acl);
    mailboxes.extensions.insert(Extension::ListMyrights);
    mailboxes.push(Folder {
        rights: MyRights::Letters("lrs"),
        ..Folder::new("Locked")
    });
    let set = synced(mailboxes).await;
    set.rig.pass().await.unwrap();
    let locked = set.rig.folder("Locked").id.to_string();
    let answer = create(&set, object(&locked)).await;
    assert_eq!(refused(&answer), "forbidden", "{answer}");
    assert_eq!(appends(&set), Vec::<String>::new());
}

#[tokio::test]
async fn a_no_after_the_literal_answers_server_fail_and_leaves_no_row_rfc3501_6_3_11() {
    let mut mailboxes = account(Extension::all());
    mailboxes.behavior.appending = Storing::Refuses;
    let set = synced(mailboxes).await;
    let before = set.state();
    let answer = create(&set, object(&drafts(&set))).await;
    assert_eq!(refused(&answer), "serverFail", "{answer}");
    assert_eq!(answer["notCreated"]["d"].get("description"), None);
    assert_eq!(set.state(), before);
    assert_eq!(set.rig.created(before), Vec::<String>::new());
    assert_eq!(appends(&set).len(), 1, "the literal went");
}

#[tokio::test]
async fn a_no_to_the_literal_it_waits_for_answers_server_fail_rfc3501_7_5() {
    let mut mailboxes = account(without(&[Extension::LiteralPlus]));
    mailboxes.behavior.continuation = Continuation::Refuses;
    let set = synced(mailboxes).await;
    let answer = create(&set, object(&drafts(&set))).await;
    assert_eq!(refused(&answer), "serverFail", "{answer}");
    assert_eq!(held(&set, DRAFTS).len(), 0);
    // A second command is read after whatever followed the first on the wire.
    create(&set, object(&drafts(&set))).await;
    let lines = set.rig.fake.lines();
    assert!(
        !lines.iter().any(|line| line.contains("Message-ID")),
        "no byte of the message left without the request: {lines:?}"
    );
}

/// A set whose link gives up at once on a server that never answers.
fn short(mut set: Set) -> Set {
    let connector = TestConnector::scripted(
        set.rig.fake.trusting(),
        set.rig.fake.target(HOST, TlsMode::Implicit),
        Duration::from_secs(2),
    );
    let timing = Timing {
        deadline: SHORT_DEADLINE,
        ..Timing::default()
    };
    set.rig.link = Arc::new(Link::with_timing(connector, timing));
    set
}

#[tokio::test]
async fn a_server_that_never_answers_the_literal_ends_at_the_deadline_rfc8620_3_6_2() {
    let mut mailboxes = account(Extension::all());
    mailboxes.behavior.appending = Storing::Stalls;
    let set = short(synced(mailboxes).await);
    let mailbox = drafts(&set);
    let answer = create(&set, object(&mailbox)).await;
    assert_eq!(refused(&answer), "serverUnavailable", "{answer}");
}

#[tokio::test]
async fn a_connection_that_closes_on_the_literal_answers_server_unavailable_rfc8620_3_6_2() {
    let mut mailboxes = account(Extension::all());
    mailboxes.behavior.appending = Storing::Drops;
    let set = synced(mailboxes).await;
    let answer = create(&set, object(&drafts(&set))).await;
    assert_eq!(refused(&answer), "serverUnavailable", "{answer}");
}

#[tokio::test]
async fn a_bad_to_the_command_is_the_servers_failure_and_costs_the_session_rfc3501_7_1_5() {
    let mut mailboxes = account(Extension::all());
    mailboxes.behavior.appending = Storing::Bad;
    let set = synced(mailboxes).await;
    let answer = create(&set, object(&drafts(&set))).await;
    assert_eq!(refused(&answer), "serverFail", "{answer}");
}

#[tokio::test]
async fn a_refused_replace_leaves_the_old_draft_and_destroys_nothing_rfc8508_3_3() {
    for extensions in [Extension::all(), without(&[Extension::Replace])] {
        let mailboxes = account(extensions);
        let set = synced(mailboxes.clone()).await;
        let mailbox = drafts(&set);
        let first = created(&create(&set, object(&mailbox)).await);
        let mut refusing = mailboxes;
        refusing.behavior.appending = Storing::Refuses;
        let set = rig_over(set, refusing).await;
        let answer = replace(&set, object(&mailbox), &first).await;
        assert_eq!(refused(&answer), "serverFail", "{answer}");
        assert_eq!(
            answer["notDestroyed"][&first]["type"], "serverFail",
            "{answer}"
        );
        assert_eq!(answer["destroyed"], Value::Null);
        assert_eq!(held(&set, DRAFTS).len(), 1);
    }
}

#[tokio::test]
async fn a_replace_on_a_lost_connection_holds_the_destroy_out_of_reach_rfc8620_3_6_2() {
    let mailboxes = account(Extension::all());
    let set = synced(mailboxes.clone()).await;
    let mailbox = drafts(&set);
    let first = created(&create(&set, object(&mailbox)).await);
    let mut dropping = mailboxes;
    dropping.behavior.appending = Storing::Drops;
    let set = rig_over(set, dropping).await;
    let answer = replace(&set, object(&mailbox), &first).await;
    assert_eq!(refused(&answer), "serverUnavailable", "{answer}");
    assert_eq!(
        answer["notDestroyed"][&first]["type"], "serverUnavailable",
        "{answer}"
    );
}

#[tokio::test]
async fn a_create_the_rights_refuse_holds_the_destroy_it_replaces_rfc4314_4() {
    let mut mailboxes = account(Extension::all());
    mailboxes.extensions.insert(Extension::Acl);
    mailboxes.extensions.insert(Extension::ListMyrights);
    mailboxes.push(Folder {
        rights: MyRights::Letters("lrste"),
        ..Folder::new("Locked").with_mail(vec![Message::new(1)])
    });
    let set = Box::pin(started(mailboxes, "Locked", false)).await;
    set.rig.pass().await.unwrap();
    let locked = set.rig.folder("Locked").id.to_string();
    let old = &set.ids[&1];
    let answer = replace(&set, object(&locked), old).await;
    assert_eq!(refused(&answer), "forbidden", "{answer}");
    assert_eq!(
        answer["notDestroyed"][old]["type"], "serverFail",
        "{answer}"
    );
    assert_eq!(held(&set, "Locked").len(), 1);
}

/// The same account and rows on a server that behaves as `mailboxes` says.
async fn rig_over(mut set: Set, mailboxes: huliho_imap_bridge::testing::Mailboxes) -> Set {
    use huliho_imap_bridge::testing::imap::{FakeImap, Script};
    let fake = FakeImap::start(Script {
        mailboxes,
        ..Script::tls()
    })
    .await;
    let connector = TestConnector::scripted(
        fake.trusting(),
        fake.target(HOST, TlsMode::Implicit),
        Duration::from_secs(2),
    );
    set.rig.link = Arc::new(Link::with_interval(connector, Duration::ZERO));
    set.rig.fake = fake;
    set
}

#[tokio::test]
async fn a_create_refused_before_a_command_holds_the_destroy_it_replaces() {
    let set = synced(account(Extension::all())).await;
    let mailbox = drafts(&set);
    let first = created(&create(&set, object(&mailbox)).await);
    let mut bad = object(&mailbox);
    bad["bodyStructure"] =
        json!({ "blobId": EmailId::generate().to_string(), "type": "image/png" });
    let answer = replace(&set, bad, &first).await;
    assert_eq!(refused(&answer), "blobNotFound", "{answer}");
    assert_eq!(
        answer["notDestroyed"][&first]["type"], "serverFail",
        "{answer}"
    );
    assert_eq!(held(&set, DRAFTS).len(), 1);
    let reading = set
        .set(json!({ "create": { "d": { "subject": "x" } }, "destroy": [first] }))
        .await;
    assert_eq!(refused(&reading), "invalidProperties");
    assert_eq!(
        reading["notDestroyed"][&first]["type"], "serverFail",
        "{reading}"
    );
    assert_eq!(reading["destroyed"], Value::Null, "{reading}");
    assert_eq!(held(&set, DRAFTS).len(), 1);
}

/// An account whose inbox holds the corpus message beside the rest.
async fn with_corpus() -> Set {
    let mailboxes = inbox(vec![corpus(SOURCE)]);
    mailboxes.push(Folder::special(DRAFTS, "\\Drafts"));
    let set = Box::pin(started(mailboxes, INBOX, false)).await;
    Box::pin(set.rig.sync(DRAFTS)).await;
    set
}

/// A tree of a text and one part by that blob.
fn attaching(mailbox: &str, blob: &Value) -> Value {
    let mut object = object(mailbox);
    object["bodyStructure"] = json!({
        "type": "multipart/mixed",
        "subParts": [{ "partId": "t", "type": "text/plain" }, blob],
    });
    object
}

#[tokio::test]
async fn a_part_of_a_held_email_travels_decoded_and_encoded_anew_rfc2045_6_8() {
    let set = with_corpus().await;
    let email = &set.ids[&SOURCE];
    let blob = json!({
        "blobId": format!("{email}-2"),
        "type": "application/pdf",
        "name": "rapport caf\u{e9}.pdf",
        "disposition": "attachment",
    });
    let answer = create(&set, attaching(&drafts(&set), &blob)).await;
    let id = created(&answer);
    let raw = held(&set, DRAFTS)[0].raw();
    let parsed = MessageParser::default().parse(raw.as_bytes()).unwrap();
    let attachment = parsed.attachments().next().unwrap();
    assert_eq!(attachment.contents(), b"%PDF-1");
    assert_eq!(attachment.attachment_name(), Some("rapport caf\u{e9}.pdf"));
    let shown = crate::draft_rig::email(&set, &id, &["hasAttachment"]).await;
    assert_eq!(shown["hasAttachment"], true);
}

#[tokio::test]
async fn a_whole_message_travels_as_a_message_part_rfc2046_5_2_1() {
    let set = with_corpus().await;
    let blob = json!({
        "blobId": set.ids[&SOURCE],
        "type": "message/rfc822",
        "name": "original.eml",
        "disposition": "attachment",
    });
    let answer = create(&set, attaching(&drafts(&set), &blob)).await;
    created(&answer);
    let raw = held(&set, DRAFTS)[0].raw();
    assert!(raw.contains("Content-Type: message/rfc822"), "{raw}");
    assert!(raw.contains("Subject: Message 7"), "{raw}");
}

#[tokio::test]
async fn a_blob_the_account_does_not_hold_is_named_back_with_no_literal_sent_rfc8621_4_6() {
    let set = with_corpus().await;
    let unknown = EmailId::generate().to_string();
    let no_part = format!("{}-9", set.ids[&SOURCE]);
    for blob_id in [unknown.as_str(), no_part.as_str(), "never handed out"] {
        let blob = json!({ "blobId": blob_id, "type": "application/pdf" });
        let answer = create(&set, attaching(&drafts(&set), &blob)).await;
        assert_eq!(refused(&answer), "blobNotFound", "{answer}");
        assert_eq!(
            answer["notCreated"]["d"]["notFound"],
            json!([blob_id]),
            "{answer}"
        );
    }
    let both = json!({
        "type": "multipart/mixed",
        "subParts": [
            { "blobId": unknown, "type": "image/png" },
            { "blobId": no_part, "type": "image/png" },
        ],
    });
    let mut object = object(&drafts(&set));
    object["bodyStructure"] = both;
    let answer = create(&set, object).await;
    assert_eq!(
        answer["notCreated"]["d"]["notFound"],
        json!([unknown, no_part]),
        "{answer}"
    );
    assert_eq!(appends(&set), Vec::<String>::new());
}

#[tokio::test]
async fn a_blob_of_another_account_is_not_found_rfc8620_6() {
    let set = with_corpus().await;
    let other_rig = Box::pin(started(inbox(vec![corpus(SOURCE)]), INBOX, false)).await;
    let blob = json!({ "blobId": other_rig.ids[&SOURCE], "type": "application/pdf" });
    let answer = create(&set, attaching(&drafts(&set), &blob)).await;
    assert_eq!(refused(&answer), "blobNotFound", "{answer}");
}

#[tokio::test]
async fn a_message_past_the_limit_of_the_server_sends_no_literal_rfc7889_4() {
    let mut mailboxes = account(Extension::all());
    mailboxes.append_limit = Some(TINY_LIMIT);
    let set = synced(mailboxes).await;
    let answer = create(&set, sized(&drafts(&set), 1)).await;
    assert_eq!(refused(&answer), "tooLarge", "{answer}");
    assert_eq!(appends(&set), Vec::<String>::new());
}

#[tokio::test]
async fn a_blob_past_what_the_blob_before_it_left_is_never_fetched_rfc8621_4_6() {
    let mut mailboxes = inbox(vec![corpus(SOURCE)]);
    mailboxes.push(Folder::special(DRAFTS, "\\Drafts"));
    mailboxes.append_limit = Some(BETWEEN_TWO_PARTS);
    let set = Box::pin(started(mailboxes, INBOX, false)).await;
    Box::pin(set.rig.sync(DRAFTS)).await;
    let email = &set.ids[&SOURCE];
    let mut object = object(&drafts(&set));
    object["bodyStructure"] = json!({
        "type": "multipart/mixed",
        "subParts": [
            { "blobId": format!("{email}-2"), "type": "application/pdf" },
            { "blobId": format!("{email}-3"), "type": "message/rfc822" },
        ],
    });
    let answer = create(&set, object).await;
    assert_eq!(refused(&answer), "tooLarge", "{answer}");
    let fetched = set.rig.fake.lines();
    let windows = fetched.iter().filter(|line| line.contains("BODY.PEEK[3]"));
    assert_eq!(windows.count(), 0, "{fetched:?}");
}

#[tokio::test]
async fn a_part_the_server_states_past_the_bound_is_never_fetched_rfc8621_4_6() {
    let mut message = corpus(SOURCE);
    message.structure = message
        .structure
        .replace("\"BASE64\" 8", &format!("\"BASE64\" {PAST_THE_BOUND}"));
    let mailboxes = inbox(vec![message]);
    mailboxes.push(Folder::special(DRAFTS, "\\Drafts"));
    let set = Box::pin(started(mailboxes, INBOX, false)).await;
    Box::pin(set.rig.sync(DRAFTS)).await;
    let blob = json!({ "blobId": format!("{}-2", set.ids[&SOURCE]), "type": "application/pdf" });
    let answer = create(&set, attaching(&drafts(&set), &blob)).await;
    assert_eq!(refused(&answer), "tooLarge", "{answer}");
    assert_eq!(appends(&set), Vec::<String>::new());
    let fetched = set.rig.fake.lines();
    let windows = fetched.iter().filter(|line| line.contains("BODY.PEEK[2]"));
    assert_eq!(windows.count(), 0, "{fetched:?}");
}
