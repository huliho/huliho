// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The blobs of an account against the scripted server: a part with its
//! transfer encoding undone, the whole message raw, both in windows on
//! the account's conversation; the ids that name nothing, the limit and
//! the windows read ahead of a reader.

mod blobs_rig;
mod sync_rig;

use blobs_rig::{
    BASE64, Blobs, CORPUS, DEEP, EXACT, LONG, PLAIN, QUOTED, ROOMY, WIDE, base64_content, drained,
    fetches, linked, message, quoted, settled, started, window,
};
use huliho_imap_bridge::blob::{BLOB_BUFFER_WINDOWS, BlobError, open};
use huliho_imap_bridge::runtime::CONVERSATION_DEADLINE;
use huliho_imap_bridge::session::BODY_WINDOW_BYTES;
use huliho_imap_bridge::store::EmailId;
use huliho_imap_bridge::testing::Mailboxes;
use huliho_imap_bridge::testing::mailboxes::ALL_MAIL;
use huliho_imap_bridge::testing::messages::SIZE_ABOVE_UID;
use huliho_imap_bridge::testing::parts::{CORPUS_HTML, CORPUS_PLAIN, corpus};
use serde_json::{Value, json};
use sync_rig::{ACCOUNT, Rig};

/// The window fetches the whole of [`WIDE`] takes.
const WIDE_WINDOWS: usize = 9;

/// The octets the structure states for the PDF of the corpus.
const PDF_STATED: u64 = 8;

/// Whether the id names no blob of the account.
async fn not_found(blobs: &Blobs, blob_id: &str) -> bool {
    matches!(
        open(&blobs.rig.cache, &blobs.link, blob_id, ROOMY).await,
        Err(BlobError::NotFound)
    )
}

/// Whether the blob is refused under that limit.
async fn too_large(blobs: &Blobs, blob_id: &str, limit: u64) -> bool {
    matches!(
        open(&blobs.rig.cache, &blobs.link, blob_id, limit).await,
        Err(BlobError::TooLarge)
    )
}

#[tokio::test]
async fn a_part_streams_under_the_blob_id_email_get_names_with_its_encoding_undone_rfc8621_4_1_4() {
    let blobs = started().await;
    let email = &blobs.ids[&CORPUS];
    let lists = ["textBody", "htmlBody", "attachments"];
    let get =
        json!(["Email/get", { "accountId": ACCOUNT, "ids": [email], "properties": lists }, "c1"]);
    let answer = blobs.rig.call(&get).await;
    let parts: Vec<&Value> = lists
        .iter()
        .flat_map(|list| answer["list"][0][list].as_array().unwrap())
        .collect();
    let expected: [(&str, &[u8]); 5] = [
        ("1.1", CORPUS_PLAIN.as_bytes()),
        ("1.2.1", CORPUS_HTML.as_bytes()),
        ("1.2.2", b"\x89PNG\r\n\x1a\n"),
        ("2", b"%PDF-1"),
        ("3", b"Subject: Attached\r\n\r\nInner"),
    ];
    for (part_id, content) in expected {
        let part = parts.iter().find(|part| part["partId"] == part_id).unwrap();
        let blob_id = part["blobId"].as_str().unwrap();
        assert_eq!(blob_id, format!("{email}-{}", part_id.replace('.', "_")));
        let blob = open(&blobs.rig.cache, &blobs.link, blob_id, ROOMY)
            .await
            .unwrap();
        assert_eq!(drained(blob).await.unwrap(), content, "{part_id}");
    }
    let fetched = fetches(&blobs.rig);
    assert!(
        fetched.ends_with(&[
            format!("{CORPUS} (UID BODYSTRUCTURE)"),
            format!("{CORPUS} (UID BODY.PEEK[3]<0.{BODY_WINDOW_BYTES}>)"),
        ]),
        "{fetched:?}"
    );
}

#[tokio::test]
async fn the_whole_message_streams_raw_under_the_email_id_in_windows_rfc3501_6_4_5() {
    let blobs = started().await;
    // A message of exactly one window takes a second fetch to find its end.
    for (uid, windows) in [(LONG, 3), (EXACT, 2), (PLAIN, 1)] {
        let before = fetches(&blobs.rig).len();
        let blob = open(&blobs.rig.cache, &blobs.link, &blobs.ids[&uid], ROOMY)
            .await
            .unwrap();
        let raw = drained(blob).await.unwrap();
        assert!(raw == message(uid).raw().into_bytes(), "{uid}");
        let expected: Vec<String> = (0..windows)
            .map(|n| {
                format!(
                    "{uid} (UID BODY.PEEK[]<{}.{BODY_WINDOW_BYTES}>)",
                    n * window()
                )
            })
            .collect();
        assert_eq!(fetches(&blobs.rig)[before..], expected, "{uid}");
    }
}

#[tokio::test]
async fn a_base64_part_and_a_quoted_printable_part_decode_equal_across_window_edges_rfc2045_6() {
    let blobs = started().await;
    let cases = [
        (BASE64, base64_content()),
        (QUOTED, quoted().0.into_bytes()),
    ];
    for (uid, content) in cases {
        let before = fetches(&blobs.rig).len();
        let blob_id = format!("{}-1", blobs.ids[&uid]);
        let blob = open(&blobs.rig.cache, &blobs.link, &blob_id, ROOMY)
            .await
            .unwrap();
        assert!(drained(blob).await.unwrap() == content, "{uid}");
        let fetched = &fetches(&blobs.rig)[before..];
        assert_eq!(fetched[0], format!("{uid} (UID BODYSTRUCTURE)"));
        assert!(fetched.len() > 2, "{fetched:?}");
        for (n, line) in fetched[1..].iter().enumerate() {
            let expected = format!(
                "{uid} (UID BODY.PEEK[TEXT]<{}.{BODY_WINDOW_BYTES}>)",
                n * window()
            );
            assert_eq!(*line, expected);
        }
    }
}

#[tokio::test]
async fn an_id_that_names_no_blob_is_not_found_and_one_without_a_row_costs_no_command() {
    let blobs = started().await;
    let corpus = &blobs.ids[&CORPUS];
    let lines = blobs.rig.fake.lines().len();
    let unknown = EmailId::generate().to_string();
    for blob_id in [
        "",
        "nonsense",
        &unknown,
        &format!("{corpus}-1x"),
        &format!("{corpus}.1"),
    ] {
        assert!(not_found(&blobs, blob_id).await, "{blob_id}");
    }
    assert_eq!(blobs.rig.fake.lines().len(), lines);
    // A multipart has no content of its own and a number past the tree
    // names nothing.
    for part in ["1", "1_2", "9", "1_1_1"] {
        assert!(
            not_found(&blobs, &format!("{corpus}-{part}")).await,
            "{part}"
        );
    }
    let plain = &blobs.ids[&PLAIN];
    assert!(not_found(&blobs, &format!("{plain}-2")).await);
    // A structure past what the bridge reads describes no part; the
    // message itself still streams.
    let deep = &blobs.ids[&DEEP];
    assert!(not_found(&blobs, &format!("{deep}-1")).await);
    let whole = open(&blobs.rig.cache, &blobs.link, deep, ROOMY)
        .await
        .unwrap();
    assert_eq!(
        drained(whole).await.unwrap(),
        message(DEEP).raw().into_bytes()
    );
    // A message that left since the sync.
    blobs
        .rig
        .fake
        .script()
        .mailboxes
        .expunge_uid("INBOX", PLAIN);
    assert!(not_found(&blobs, plain).await);
}

#[tokio::test]
async fn a_blob_past_the_limit_is_refused_before_a_window_and_breaks_off_when_more_arrives() {
    let blobs = started().await;
    let long = &blobs.ids[&LONG];
    let lines = blobs.rig.fake.lines().len();
    let stated = u64::from(SIZE_ABOVE_UID + LONG);
    assert!(too_large(&blobs, long, stated - 1).await);
    assert_eq!(blobs.rig.fake.lines().len(), lines);
    let pdf = format!("{}-2", blobs.ids[&CORPUS]);
    assert!(too_large(&blobs, &pdf, PDF_STATED - 1).await);
    assert_eq!(
        fetches(&blobs.rig),
        [format!("{CORPUS} (UID BODYSTRUCTURE)")]
    );
    let fits = open(&blobs.rig.cache, &blobs.link, &pdf, PDF_STATED)
        .await
        .unwrap();
    assert_eq!(drained(fits).await.unwrap(), b"%PDF-1");
    // The message holds more than its row states, so the stream breaks
    // off once the windows pass the limit.
    let limit = u64::from(BODY_WINDOW_BYTES) + 1;
    let blob = open(&blobs.rig.cache, &blobs.link, long, limit)
        .await
        .unwrap();
    assert!(matches!(drained(blob).await, Err(BlobError::TooLarge)));
}

#[tokio::test]
async fn a_stream_reads_four_windows_ahead_of_its_reader_and_ends_when_the_reader_leaves() {
    let blobs = started().await;
    let wide = &blobs.ids[&WIDE];
    let ahead = 1 + BLOB_BUFFER_WINDOWS;
    let blob = open(&blobs.rig.cache, &blobs.link, wide, ROOMY)
        .await
        .unwrap();
    assert!(
        settled(&blobs.rig, ahead).await,
        "{:?}",
        fetches(&blobs.rig)
    );
    let raw = drained(blob).await.unwrap();
    assert!(raw == message(WIDE).raw().into_bytes(), "{}", raw.len());
    assert_eq!(fetches(&blobs.rig).len(), WIDE_WINDOWS);
    let left = open(&blobs.rig.cache, &blobs.link, wide, ROOMY)
        .await
        .unwrap();
    assert!(settled(&blobs.rig, WIDE_WINDOWS + ahead).await);
    drop(left);
    assert!(settled(&blobs.rig, WIDE_WINDOWS + ahead).await);
}

#[tokio::test]
async fn a_gmail_account_reads_its_blobs_from_the_store_folder() {
    let mut rig = Rig::over(Mailboxes::gmail(vec![corpus(CORPUS)])).await;
    rig.cache.gmail = true;
    rig.pass().await.unwrap();
    rig.sync(ALL_MAIL).await;
    let email = rig.created(0).remove(0);
    let link = linked(&rig, CONVERSATION_DEADLINE);
    let pdf = open(&rig.cache, &link, &format!("{email}-2"), ROOMY)
        .await
        .unwrap();
    assert_eq!(drained(pdf).await.unwrap(), b"%PDF-1");
    let whole = open(&rig.cache, &link, &email, ROOMY).await.unwrap();
    assert_eq!(
        drained(whole).await.unwrap(),
        corpus(CORPUS).raw().into_bytes()
    );
    let examined: Vec<String> = rig
        .fake
        .lines()
        .into_iter()
        .filter(|line| line.contains("EXAMINE"))
        .collect();
    assert!(
        examined
            .iter()
            .all(|line| line.contains(&format!("\"{ALL_MAIL}\""))),
        "{examined:?}"
    );
}
