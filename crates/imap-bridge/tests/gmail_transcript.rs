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
    assert!(!logins.is_empty());
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
