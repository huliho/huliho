// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The Gmail scenario suite in its three runs: against the checked-in
//! transcript in every test run, against the scripted server's Gmail
//! mode through the recorder and back through the replayer, and against
//! the live account when the environment names one.

mod transcript_rig;

use std::path::{Path, PathBuf};
use std::time::Duration;

use huliho_imap_bridge::session::TlsMode;
use huliho_imap_bridge::sync::preview::PREVIEW_PLAIN_FETCH_BYTES;
use huliho_imap_bridge::testing::imap::{FakeImap, HOST, Script};
use huliho_imap_bridge::testing::record::fixed::{Section, section};
use huliho_imap_bridge::testing::transcript::Segment;
use huliho_imap_bridge::testing::{Mailboxes, PASSWORD, Transcript, USER};
use transcript_rig::corpus::{LABEL, Seed, seeds};
use transcript_rig::gmail::{self, Account, Stores};
use transcript_rig::scenario::{Scenario, Second};
use transcript_rig::{Connection, Suite};

/// Gmail's transcript, replayed in every run.
const TRANSCRIPT: &str = "tests/transcripts/gmail.json";

/// Room for a loopback exchange.
const STEP: Duration = Duration::from_secs(2);

/// The scripted server in its Gmail mode over the corpus, with the
/// user label.
fn model() -> Mailboxes {
    let mail = seeds().iter().map(Seed::model).collect();
    Mailboxes::gmail(mail).with_label(LABEL)
}

fn transcript_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(TRANSCRIPT)
}

/// Every command of the transcript that signs in.
fn logins(transcript: &Transcript) -> Vec<&str> {
    transcript
        .sessions
        .iter()
        .flat_map(|session| session.exchanges.iter())
        .filter_map(|exchange| exchange.client.as_deref())
        .filter(|line| line.contains(" LOGIN "))
        .collect()
}

/// The text of a server line, its literals left out.
fn text_of(line: &huliho_imap_bridge::testing::transcript::Line) -> String {
    let texts = line.0.iter().filter_map(|segment| match segment {
        Segment::Text(text) => Some(text.as_str()),
        Segment::Literal { .. } => None,
    });
    texts.collect()
}

/// The last quoted string of a line, its escapes undone.
fn last_quoted(text: &str) -> Option<String> {
    let end = text.trim_end().strip_suffix('"')?;
    let start = end.rfind(" \"")?;
    Some(end[start + 2..].replace("\\\\", "\\"))
}

/// Every command of the transcript that stores `\Deleted` or expunges,
/// each with whether a Spam or Trash store was selected when it went out.
fn expunging(transcript: &Transcript) -> Vec<(bool, String)> {
    let mut bins = Vec::new();
    let mut found = Vec::new();
    for session in &transcript.sessions {
        let mut selected = None;
        for exchange in &session.exchanges {
            if let Some(client) = &exchange.client {
                if client.contains(" SELECT ") || client.contains(" EXAMINE ") {
                    selected = last_quoted(client);
                }
                if client.contains("\\Deleted") || client.contains("EXPUNGE") {
                    let in_bin = selected.as_ref().is_some_and(|name| bins.contains(name));
                    found.push((in_bin, client.clone()));
                }
            }
            for text in exchange.server.iter().map(text_of) {
                let bin = text.contains("\\Junk") || text.contains("\\Trash");
                if text.starts_with("* LIST (") && bin {
                    bins.extend(last_quoted(&text));
                }
            }
        }
    }
    found
}

/// No `\Deleted` and no expunge outside the Spam and Trash stores, so
/// archive never expunges whatever the account's expunge settings say;
/// a destroy in Trash does both there.
fn expunges_in_spam_and_trash_alone(transcript: &Transcript) {
    let found = expunging(transcript);
    assert!(found.iter().all(|(in_bin, _)| *in_bin), "{found:#?}");
    assert!(
        found.iter().any(|(_, line)| line.contains("UID EXPUNGE ")),
        "{found:#?}"
    );
}

/// The length of every body text the transcript holds.
fn body_lengths(transcript: &Transcript) -> Vec<usize> {
    let mut lengths = Vec::new();
    for line in transcript
        .sessions
        .iter()
        .flat_map(|session| session.exchanges.iter())
        .flat_map(|exchange| exchange.server.iter())
    {
        let mut before = "";
        for segment in &line.0 {
            match segment {
                Segment::Text(text) => before = text,
                Segment::Literal { literal } if section(before) == Section::Body => {
                    lengths.push(literal.len());
                }
                Segment::Literal { .. } => {}
            }
        }
    }
    lengths
}

/// The suite against the scripted server through the recorder, then the
/// same suite against what was recorded: the recorder, the redaction and
/// the replayer prove themselves without any live account.
#[tokio::test]
async fn the_suite_recorded_off_the_scripted_server_replays_the_same_way() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("scripted.json");
    let mailboxes = model();
    let fake = FakeImap::start(Script {
        mailboxes: mailboxes.clone(),
        ..Script::tls()
    })
    .await;
    let connection = Connection {
        tls: fake.trusting(),
        target: fake.target(HOST, TlsMode::Implicit),
        user: USER.to_owned(),
        password: PASSWORD.to_owned(),
        step: STEP,
    };
    let recorded = Suite::against(connection, Some(path.clone())).await;
    Scenario::new(recorded, Second::Model(mailboxes))
        .run()
        .await;
    let transcript = Transcript::from_json(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let logins = logins(&transcript);
    assert!(!logins.is_empty(), "no LOGIN was recorded");
    assert!(
        logins
            .iter()
            .all(|line| line.ends_with("LOGIN \"sanne@example.test\" \"correct horse\"")),
        "{logins:?}"
    );
    assert!(
        !body_lengths(&transcript).is_empty(),
        "the previews were fetched"
    );
    expunges_in_spam_and_trash_alone(&transcript);
    let replayed = Suite::replaying(transcript, STEP).await;
    Scenario::new(replayed, Second::Nobody).run().await;
}

/// Gmail's transcript drives the suite in every run; every body text
/// the recording holds is inside the partial fetch that asked for it.
#[tokio::test]
async fn the_gmail_transcript_replays_the_scenario_suite() {
    let json = std::fs::read_to_string(transcript_path())
        .unwrap_or_else(|error| panic!("{TRANSCRIPT} is not there ({error}); record it first"));
    let transcript = Transcript::from_json(&json).unwrap();
    let asked = usize::try_from(PREVIEW_PLAIN_FETCH_BYTES).unwrap();
    for length in body_lengths(&transcript) {
        assert!(length <= asked, "a body of {length} bytes passes the ask");
    }
    expunges_in_spam_and_trash_alone(&transcript);
    let replayed = Suite::replaying(transcript, STEP).await;
    Scenario::new(replayed, Second::Nobody).run().await;
}

/// The suite against the live test account, when the environment names
/// one; with a path in `HULIHO_RECORD_TRANSCRIPT` the run writes the
/// transcript there.
#[tokio::test]
async fn the_gmail_account_answers_the_scenario_suite() {
    let Some(account) = Account::from_env() else {
        eprintln!(
            "skipped: {} and {} are not set",
            gmail::ADDRESS,
            gmail::APP_PASSWORD
        );
        return;
    };
    let connection = account.connection().await;
    let stores = Stores::discover(&connection).await;
    let mut editor = account.editor().await;
    gmail::clear(&mut editor, &stores).await;
    gmail::seed_account(&mut editor).await;
    let live = Suite::against(connection, Account::record_path()).await;
    let second = Second::Account {
        editor: Box::new(editor),
        stores,
    };
    Scenario::new(live, second).run().await;
}
