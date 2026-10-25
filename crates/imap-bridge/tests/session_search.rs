// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The session's search against the scripted server in its folder mode
//! and its Gmail mode: every term a literal in the form the capability
//! names, the UIDs that match every key, the caps and a NUL refused
//! before anything is sent, a charset the server refuses, a SEARCH line
//! past the guard's bound and a continuation request that never comes.

mod session_rig;

use std::collections::BTreeSet;
use std::time::Duration;

use huliho_imap_bridge::session::{
    GmailSearch, ImapSession, MAX_STRUCTURED_BYTES, SEARCH_TERM_BYTES, SearchAsk, Searching,
    Session, SessionError, TlsMode,
};
use huliho_imap_bridge::testing::command::Continuation;
use huliho_imap_bridge::testing::imap::{FakeImap, HOST};
use huliho_imap_bridge::testing::mailboxes::ALL_MAIL;
use huliho_imap_bridge::testing::{
    Behavior, Extension, Folder, Mailboxes, Message, PASSWORD, USER,
};
use session_rig::{session, start};
use time::{Date, Month};

const INBOX: &str = "INBOX";

/// A step that a loopback exchange fits in and a test waits out.
const SHORT_STEP: Duration = Duration::from_millis(500);

/// How long a slow server sits on a SEARCH: past the short step.
const SLOW_ANSWER: Duration = Duration::from_millis(800);

/// A deadline that holds the slow answer, and one that does not.
const ROOMY_DEADLINE: Duration = Duration::from_secs(5);
const TIGHT_DEADLINE: Duration = Duration::from_millis(200);

/// A message from `sender` whose subject and body hold these words.
fn message(uid: u32, sender: &str, subject: &str, body: &str) -> Message {
    Message {
        header: format!(
            "From: {sender}\r\nTo: mo@example.test\r\nSubject: {subject}\r\nMessage-ID: <m{uid}@example.test>\r\n\r\n"
        ),
        body: body.to_owned(),
        ..Message::new(uid)
    }
}

/// Four messages: two from Anna about the plan, one of them read and
/// flagged, one unread lunch note and a newsletter.
fn mail() -> Vec<Message> {
    vec![
        message(
            1,
            "Anna Jansen <anna@example.test>",
            "Quarterly plan",
            "Zoë sends the plan.",
        ),
        message(2, "Bob <bob@example.test>", "Lunch", "Tomorrow?").flagged(&[]),
        message(3, "anna@example.test", "Re: Quarterly plan", "Thanks, Zoë!")
            .flagged(&["\\Seen", "\\Flagged"]),
        message(
            4,
            "Mo <mo@example.test>",
            "Newsletter",
            "Nothing about the plan.",
        )
        .labeled(&["Work"]),
    ]
}

/// The inbox behind these extensions.
fn inbox(extensions: BTreeSet<Extension>) -> Mailboxes {
    Mailboxes::new(vec![Folder::new(INBOX).with_mail(mail())], extensions)
}

/// The extensions of the scripted Dovecot without the two literal ones,
/// with the one named.
fn literals(kept: Option<Extension>) -> BTreeSet<Extension> {
    let mut extensions = Extension::all();
    extensions.remove(&Extension::LiteralPlus);
    extensions.extend(kept);
    extensions
}

/// A search of the text and the sender alone.
fn ask<'a>(text: &'a str, from: &'a str, server: Searching) -> SearchAsk<'a> {
    SearchAsk {
        text,
        from,
        unseen: false,
        flagged: false,
        since: None,
        before: None,
        gmail: None,
        server,
        until: None,
    }
}

/// A session with the folder examined and what the server advertises
/// for a search.
async fn examined(fake: &FakeImap, folder: &str) -> (ImapSession, Searching) {
    let mut session = session(fake).await;
    let searching = session.capabilities().await.unwrap().searching();
    session.examine(folder).await.unwrap();
    (session, searching)
}

/// Every UID SEARCH the server received, as the wire carried it,
/// without its tag.
fn searches(fake: &FakeImap) -> Vec<String> {
    fake.lines()
        .iter()
        .filter_map(|line| line.split_once(' ')?.1.split_once(' '))
        .map(|(_, command)| command.to_owned())
        .filter(|command| command.starts_with("UID SEARCH "))
        .collect()
}

#[tokio::test]
async fn a_search_answers_the_uids_that_match_every_term_with_each_term_a_literal_rfc3501_6_4_4() {
    let fake = start(inbox(Extension::all())).await;
    let (mut session, searching) = examined(&fake, INBOX).await;
    let found = session
        .uid_search(&ask("zoë plan", "anna", searching))
        .await
        .unwrap();
    assert_eq!(found, [3, 1]);
    assert_eq!(
        searches(&fake),
        ["UID SEARCH CHARSET UTF-8 1:4 TEXT {4+}\r\nzoë TEXT {4+}\r\nplan FROM {4+}\r\nanna"]
    );
    let phrase = session
        .uid_search(&ask("\"sends the\"", "", searching))
        .await
        .unwrap();
    assert_eq!(phrase, [1]);
}

#[tokio::test]
async fn each_literal_takes_the_form_the_server_advertises_rfc7888_1() {
    for (kept, marker) in [
        (Some(Extension::LiteralPlus), "{4+}\r\n"),
        (Some(Extension::LiteralMinus), "{4+}\r\n"),
        (None, "{4}\r\n"),
    ] {
        let fake = start(inbox(literals(kept))).await;
        let (mut session, searching) = examined(&fake, INBOX).await;
        let found = session
            .uid_search(&ask("plan", "anna", searching))
            .await
            .unwrap();
        assert_eq!(found, [3, 1], "{kept:?}");
        let sent = searches(&fake);
        assert_eq!(
            sent,
            [format!(
                "UID SEARCH CHARSET UTF-8 1:4 TEXT {marker}plan FROM {marker}anna"
            )],
            "{kept:?}"
        );
    }
}

#[tokio::test]
async fn the_flags_and_the_days_go_as_keys_of_their_own_rfc3501_6_4_4() {
    let fake = start(inbox(Extension::all())).await;
    let (mut session, searching) = examined(&fake, INBOX).await;
    let unseen = SearchAsk {
        unseen: true,
        ..ask("", "", searching)
    };
    assert_eq!(session.uid_search(&unseen).await.unwrap(), [2]);
    let flagged = SearchAsk {
        flagged: true,
        ..ask("", "anna", searching)
    };
    assert_eq!(session.uid_search(&flagged).await.unwrap(), [3]);
    let day = Date::from_calendar_date(2026, Month::January, 2).ok();
    let since = SearchAsk {
        since: day,
        ..ask("", "", searching)
    };
    assert_eq!(session.uid_search(&since).await.unwrap(), Vec::<u32>::new());
    let before = SearchAsk {
        before: day,
        ..ask("", "", searching)
    };
    assert_eq!(session.uid_search(&before).await.unwrap(), [4, 3, 2, 1]);
    let sent = searches(&fake);
    assert_eq!(sent[0], "UID SEARCH 1:4 UNSEEN");
    assert_eq!(sent[2], "UID SEARCH 1:4 SINCE 2-Jan-2026");
}

#[tokio::test]
async fn a_folder_past_one_window_gets_the_same_keys_in_every_window() {
    let mail = (1..=5003).map(Message::new).collect();
    let fake = start(Mailboxes::new(
        vec![Folder::new(INBOX).with_mail(mail)],
        Extension::all(),
    ))
    .await;
    let (mut session, searching) = examined(&fake, INBOX).await;
    let found = session
        .uid_search(&ask("message", "", searching))
        .await
        .unwrap();
    assert_eq!(found, (1..=5003).rev().collect::<Vec<u32>>());
    assert_eq!(
        searches(&fake),
        [
            "UID SEARCH CHARSET UTF-8 4:5003 TEXT {7+}\r\nmessage",
            "UID SEARCH CHARSET UTF-8 1:3 TEXT {7+}\r\nmessage",
        ]
    );
}

#[tokio::test]
async fn a_term_past_the_caps_or_holding_a_nul_sends_nothing_and_the_session_stands() {
    let fake = start(inbox(Extension::all())).await;
    let (mut session, searching) = examined(&fake, INBOX).await;
    let long = "a".repeat(SEARCH_TERM_BYTES + 1);
    let gmail = SearchAsk {
        gmail: Some(GmailSearch::default()),
        ..ask("plan", "", searching)
    };
    for refused in [
        ask(&long, "", searching),
        ask("a b c d e", "f g h i", searching),
        ask("plan\0\r\nA9 LOGOUT", "", searching),
        ask("", "", searching),
        gmail,
    ] {
        assert!(
            matches!(
                session.uid_search(&refused).await,
                Err(SessionError::Protocol(_))
            ),
            "{refused:?}"
        );
    }
    assert_eq!(searches(&fake), Vec::<String>::new());
    let found = session
        .uid_search(&ask("plan", "", searching))
        .await
        .unwrap();
    assert_eq!(found, [4, 3, 1]);
}

#[tokio::test]
async fn a_charset_the_server_refuses_reads_as_bad_charset_and_the_session_stands_rfc3501_7_1() {
    let mut mailboxes = inbox(Extension::all());
    mailboxes.behavior = Behavior {
        only_charset: Some("US-ASCII"),
        ..Behavior::default()
    };
    let fake = start(mailboxes).await;
    let (mut session, searching) = examined(&fake, INBOX).await;
    let refused = session.uid_search(&ask("plan", "", searching)).await;
    assert!(
        matches!(refused, Err(SessionError::BadCharset)),
        "{refused:?}"
    );
    session.noop().await.unwrap();
    let unseen = SearchAsk {
        unseen: true,
        ..ask("", "", searching)
    };
    assert_eq!(session.uid_search(&unseen).await.unwrap(), [2]);
}

#[tokio::test]
async fn a_search_line_past_the_structure_bound_fails_the_connection_for_good() {
    let mut mailboxes = inbox(Extension::all());
    mailboxes.behavior = Behavior {
        search_bytes: Some(MAX_STRUCTURED_BYTES + 1),
        ..Behavior::default()
    };
    let fake = start(mailboxes).await;
    let (mut session, searching) = examined(&fake, INBOX).await;
    let words = "the answer passes the structure limit";
    let failed = session.uid_search(&ask("plan", "", searching)).await;
    assert!(
        matches!(failed, Err(SessionError::Protocol(found)) if found == words),
        "{failed:?}"
    );
    // The client library reads nothing more off a stream that failed.
    let after = session.noop().await;
    assert!(matches!(after, Err(SessionError::Closed)), "{after:?}");
}

/// The inbox with a server that reads no literal without its
/// continuation request and answers a literal so.
fn continued(continuation: Continuation) -> Mailboxes {
    let mut mailboxes = inbox(literals(None));
    mailboxes.behavior = Behavior {
        continuation,
        ..Behavior::default()
    };
    mailboxes
}

/// A session with the short step and the inbox examined, with what the
/// server advertises for a search.
async fn short_step(fake: &FakeImap) -> (ImapSession, Searching) {
    let target = fake.target(HOST, TlsMode::Implicit);
    let mut session = ImapSession::connect(fake.trusting(), &target, SHORT_STEP)
        .await
        .unwrap();
    session.login(USER, PASSWORD).await.unwrap();
    let searching = session.capabilities().await.unwrap().searching();
    session.examine(INBOX).await.unwrap();
    (session, searching)
}

#[tokio::test]
async fn a_search_waits_for_its_answer_until_its_deadline_in_place_of_the_step() {
    let mut mailboxes = inbox(Extension::all());
    mailboxes.behavior = Behavior {
        search_delay: Some(SLOW_ANSWER),
        ..Behavior::default()
    };
    let fake = start(mailboxes).await;
    let (mut stepped, searching) = short_step(&fake).await;
    let waited = stepped.uid_search(&ask("plan", "", searching)).await;
    assert!(matches!(waited, Err(SessionError::Timeout)), "{waited:?}");
    let (mut roomy, _) = short_step(&fake).await;
    let until = SearchAsk {
        until: Some(tokio::time::Instant::now() + ROOMY_DEADLINE),
        ..ask("plan", "", searching)
    };
    assert_eq!(roomy.uid_search(&until).await.unwrap(), [4, 3, 1]);
    let (mut tight, _) = short_step(&fake).await;
    let past = SearchAsk {
        until: Some(tokio::time::Instant::now() + TIGHT_DEADLINE),
        ..ask("plan", "", searching)
    };
    let cut = tight.uid_search(&past).await;
    assert!(matches!(cut, Err(SessionError::Timeout)), "{cut:?}");
}

#[tokio::test]
async fn a_continuation_request_that_never_comes_ends_at_the_step_rfc3501_7_5() {
    let fake = start(continued(Continuation::Withholds)).await;
    let (mut session, searching) = short_step(&fake).await;
    let waited = session.uid_search(&ask("plan", "", searching)).await;
    assert!(matches!(waited, Err(SessionError::Timeout)), "{waited:?}");
    assert_eq!(searches(&fake), ["UID SEARCH CHARSET UTF-8 1:4 TEXT {4}"]);
}

#[tokio::test]
async fn a_literal_refused_in_place_of_its_request_sends_nothing_more_of_the_command_rfc3501_2_2_1()
{
    let fake = start(continued(Continuation::Refuses)).await;
    let (mut session, searching) = examined(&fake, INBOX).await;
    let refused = session.uid_search(&ask("plan", "anna", searching)).await;
    assert!(matches!(refused, Err(SessionError::Refused)), "{refused:?}");
    session.noop().await.unwrap();
    let received: Vec<String> = fake.lines();
    let after = received
        .iter()
        .skip_while(|line| !line.contains(" UID SEARCH "))
        .filter_map(|line| line.split_once(' ')?.1.split_once(' '))
        .map(|(_, command)| command)
        .collect::<Vec<_>>();
    assert_eq!(after, ["UID SEARCH CHARSET UTF-8 1:4 TEXT {4}", "NOOP"]);
}

#[tokio::test]
async fn gmail_reads_one_raw_string_and_a_label_as_its_web_search_would() {
    let fake = start(Mailboxes::gmail(mail())).await;
    let (mut session, searching) = examined(&fake, ALL_MAIL).await;
    assert!(searching.gmail);
    let inbox = SearchAsk {
        gmail: Some(GmailSearch {
            label: Some("\\Inbox"),
        }),
        ..ask("plan", "Anna Jansen", searching)
    };
    assert_eq!(session.uid_search(&inbox).await.unwrap(), [1]);
    let work = SearchAsk {
        gmail: Some(GmailSearch {
            label: Some("Work"),
        }),
        ..ask("plan", "", searching)
    };
    assert_eq!(session.uid_search(&work).await.unwrap(), [4]);
    let raw = "plan from:\"Anna Jansen\"";
    assert_eq!(
        searches(&fake),
        [
            format!(
                "UID SEARCH CHARSET UTF-8 1:4 X-GM-RAW {{{}+}}\r\n{raw} X-GM-LABELS {{6+}}\r\n\\Inbox",
                raw.len()
            ),
            "UID SEARCH CHARSET UTF-8 1:4 X-GM-RAW {4+}\r\nplan X-GM-LABELS {4+}\r\nWork"
                .to_owned(),
        ]
    );
}
