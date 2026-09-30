// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! `Email/get` over the message on the server: the tree of RFC 8621
//! section 4.1.4, the values in windows, the header forms and the
//! passes of a request, against the scripted server in the folder mode
//! and the Gmail mode.

mod bodies_rig;
mod stable_ids;
mod sync_rig;

use bodies_rig::{
    BODY, CLIENT_CAP, CORPUS, MULTIBYTE, PLAIN, SIX, ask, fetches, get, six_windows_of_text,
    started,
};
use huliho_imap_bridge::jmap::{MAX_BODIES_IN_GET, MAX_BODY_VALUE_BYTES};
use huliho_imap_bridge::session::BODY_WINDOW_BYTES;
use huliho_imap_bridge::testing::Mailboxes;
use huliho_imap_bridge::testing::mailboxes::ALL_MAIL;
use huliho_imap_bridge::testing::parts::{
    CORPUS_AUTHENTICATION, CORPUS_HTML, CORPUS_PLAIN, corpus,
};
use serde_json::json;
use sync_rig::{ACCOUNT, Rig};

/// A cap under the base64 part, so its fetch stops after two windows.
const LOW_CAP: u64 = 1024 * 1024;

/// A cap inside the second quartet of the multibyte part, whose first
/// quartet ends inside a character.
const MID_CHARACTER_CAP: u64 = 6;

/// The `<offset.bytes>` of every window fetch.
fn windows(fetches: &[String]) -> Vec<(u32, u32)> {
    fetches
        .iter()
        .filter_map(|line| line.split_once("]<"))
        .filter_map(|(_, rest)| rest.split_once('>'))
        .filter_map(|(window, _)| window.split_once('.'))
        .filter_map(|(offset, bytes)| Some((offset.parse().ok()?, bytes.parse().ok()?)))
        .collect()
}

#[tokio::test]
async fn the_corpus_answers_its_tree_its_lists_its_values_and_its_header_forms_rfc8621_4_1_4() {
    let (rig, names, by_name) = started().await;
    let state = rig.cache.store.state(&rig.cache.key).unwrap();
    let mut properties = BODY.to_vec();
    properties.extend([
        "subject",
        "header:Authentication-Results:asRaw:all",
        "header:Authentication-Results:asText",
        "header:X-Missing",
        "header:x-missing:all",
    ]);
    let answer = get(&rig, ask(&[&by_name["e1"]], &properties, CLIENT_CAP)).await;
    assert_eq!(answer["notFound"], json!([]));
    let mut email = answer["list"][0].clone();
    stable_ids::rename(&mut email, &names);
    insta::assert_json_snapshot!(email);
    let raw = format!(" {CORPUS_AUTHENTICATION}");
    assert_eq!(
        email["header:Authentication-Results:asRaw:all"],
        json!([raw])
    );
    assert_eq!(
        email["header:Authentication-Results:asText"],
        json!(CORPUS_AUTHENTICATION)
    );
    assert_eq!(email["bodyValues"]["1.1"]["value"], CORPUS_PLAIN);
    assert_eq!(email["bodyValues"]["1.2.1"]["value"], CORPUS_HTML);
    assert_eq!(
        fetches(&rig),
        [
            format!(
                "{CORPUS} (UID BODYSTRUCTURE BODY.PEEK[HEADER.FIELDS (Authentication-Results X-Missing)]<0.65536>)"
            ),
            format!("{CORPUS} (UID BODY.PEEK[1.1]<0.{}>)", CORPUS_PLAIN.len()),
            format!("{CORPUS} (UID BODY.PEEK[1.2.1]<0.{}>)", CORPUS_HTML.len()),
        ]
    );
    // Nothing of a body is stored, so the state stands.
    assert_eq!(rig.cache.store.state(&rig.cache.key).unwrap(), state);
    assert_eq!(answer["state"], state.to_string());
}

#[tokio::test]
async fn a_message_of_one_part_reads_its_text_section_as_part_one() {
    let (rig, _, by_name) = started().await;
    let answer = get(&rig, ask(&[&by_name["e7"]], &BODY, CLIENT_CAP)).await;
    let email = &answer["list"][0];
    assert_eq!(email["textBody"][0]["partId"], "1");
    assert_eq!(email["htmlBody"][0]["partId"], "1");
    assert_eq!(email["attachments"], json!([]));
    assert_eq!(email["bodyStructure"]["partId"], "1");
    assert_eq!(email["bodyStructure"]["type"], "text/plain");
    assert_eq!(email["bodyValues"]["1"]["value"], "Body of message 7.");
    assert_eq!(email["bodyValues"]["1"]["isTruncated"], false);
    assert_eq!(
        fetches(&rig),
        [
            format!("{PLAIN} (UID BODYSTRUCTURE)"),
            format!("{PLAIN} (UID BODY.PEEK[TEXT]<0.18>)"),
        ]
    );
}

#[tokio::test]
async fn a_base64_part_of_three_mib_comes_in_six_windows_and_a_lower_cap_stops_early() {
    let (rig, _, by_name) = started().await;
    let answer = get(
        &rig,
        ask(&[&by_name["e3"]], &["bodyValues", "htmlBody"], CLIENT_CAP),
    )
    .await;
    let value = &answer["list"][0]["bodyValues"]["1"];
    assert_eq!(value["value"], six_windows_of_text().replace("\r\n", "\n"));
    assert_eq!(value["isTruncated"], false);
    assert_eq!(value["isEncodingProblem"], false);
    let fetched = fetches(&rig);
    assert_eq!(fetched.len(), 1 + SIX, "{fetched:?}");
    let expected: Vec<(u32, u32)> = (0..SIX)
        .map(|window| {
            (
                u32::try_from(window).unwrap() * BODY_WINDOW_BYTES,
                BODY_WINDOW_BYTES,
            )
        })
        .collect();
    assert_eq!(windows(&fetched), expected);
    let lower = get(
        &rig,
        ask(&[&by_name["e3"]], &["bodyValues", "htmlBody"], LOW_CAP),
    )
    .await;
    let value = &lower["list"][0]["bodyValues"]["1"];
    assert_eq!(value["isTruncated"], true);
    assert!(value["value"].as_str().unwrap().len() <= usize::try_from(LOW_CAP).unwrap());
    assert!(
        six_windows_of_text()
            .replace("\r\n", "\n")
            .starts_with(value["value"].as_str().unwrap())
    );
    let fetched = fetches(&rig);
    assert_eq!(fetched.len(), 1 + SIX + 1 + 2, "{fetched:?}");
    assert!(
        windows(&fetched)
            .iter()
            .all(|(_, bytes)| *bytes <= BODY_WINDOW_BYTES)
    );
}

#[tokio::test]
async fn a_cap_inside_an_encoded_character_answers_the_characters_before_it_rfc8621_4_1_4() {
    let (rig, _, by_name) = started().await;
    let answer = get(
        &rig,
        ask(
            &[&by_name["e9"]],
            &["bodyValues", "textBody"],
            MID_CHARACTER_CAP,
        ),
    )
    .await;
    let value = &answer["list"][0]["bodyValues"]["1"];
    assert_eq!(value["value"], "\u{e9}");
    assert_eq!(value["isTruncated"], true);
    assert_eq!(value["isEncodingProblem"], false);
    assert_eq!(
        fetches(&rig),
        [
            format!("{MULTIBYTE} (UID BODYSTRUCTURE)"),
            format!("{MULTIBYTE} (UID BODY.PEEK[TEXT]<0.{MID_CHARACTER_CAP}>)"),
        ]
    );
}

#[tokio::test]
async fn a_part_past_the_bridge_cap_is_cut_at_the_cap_whatever_the_client_asks() {
    let (rig, _, by_name) = started().await;
    let answer = get(&rig, ask(&[&by_name["e4"]], &["bodyValues", "textBody"], 0)).await;
    let value = &answer["list"][0]["bodyValues"]["1"];
    assert_eq!(
        value["value"].as_str().unwrap().len(),
        usize::try_from(MAX_BODY_VALUE_BYTES).unwrap()
    );
    assert_eq!(value["isTruncated"], true);
    let fetched = fetches(&rig);
    let cap_windows = usize::try_from(MAX_BODY_VALUE_BYTES / BODY_WINDOW_BYTES).unwrap();
    assert_eq!(fetched.len(), 1 + cap_windows, "{fetched:?}");
}

#[tokio::test]
async fn header_forms_alone_cost_one_fetch_of_the_fields_and_come_in_the_order_written() {
    let (rig, _, by_name) = started().await;
    let properties = [
        "header:Authentication-Results:asRaw:all",
        "header:authentication-results",
        "header:Subject:asText",
    ];
    let answer = get(
        &rig,
        json!({ "accountId": ACCOUNT, "ids": [by_name["e1"]], "properties": properties }),
    )
    .await;
    let email = &answer["list"][0];
    let raw = format!(" {CORPUS_AUTHENTICATION}");
    assert_eq!(
        email["header:Authentication-Results:asRaw:all"],
        json!([raw])
    );
    assert_eq!(email["header:authentication-results"], json!(raw));
    assert_eq!(email["header:Subject:asText"], json!("Message 1"));
    assert_eq!(email.as_object().unwrap().len(), 4);
    assert_eq!(
        fetches(&rig),
        [format!(
            "{CORPUS} (UID BODY.PEEK[HEADER.FIELDS (Authentication-Results Subject)]<0.65536>)"
        )]
    );
}

#[tokio::test]
async fn a_call_after_a_body_ask_reads_its_answer_through_a_reference_rfc8620_3_7() {
    let (rig, _, _) = started().await;
    let reference = |result_of: &str, name: &str, path: &str| json!({ "resultOf": result_of, "name": name, "path": path });
    let inbox = rig.folder("INBOX").id;
    // The default property set reads the server, so the window stays
    // within the messages one call may read.
    let calls = [
        json!([
            "Email/query",
            { "accountId": ACCOUNT, "filter": { "inMailbox": inbox.as_str() }, "limit": MAX_BODIES_IN_GET },
            "q"
        ]),
        json!([
            "Email/get",
            { "accountId": ACCOUNT, "#ids": reference("q", "Email/query", "/ids") },
            "g"
        ]),
        json!([
            "Thread/get",
            { "accountId": ACCOUNT, "#ids": reference("g", "Email/get", "/list/*/threadId") },
            "t"
        ]),
        // A body ask by reference into a body call resolves on a third pass.
        json!([
            "Email/get",
            { "accountId": ACCOUNT, "#ids": reference("g", "Email/get", "/list/*/id") },
            "b"
        ]),
    ];
    let response = rig.calls(&calls).await;
    let responses = response["methodResponses"].as_array().unwrap();
    let queried = responses[0][1]["ids"].as_array().unwrap().len();
    assert_eq!(queried, MAX_BODIES_IN_GET);
    assert_eq!(responses[1][0], "Email/get", "{}", responses[1]);
    let emails = responses[1][1]["list"].as_array().unwrap();
    assert_eq!(emails.len(), queried);
    assert!(emails.iter().all(|email| email["textBody"].is_array()));
    assert_eq!(responses[2][0], "Thread/get", "{}", responses[2]);
    assert_eq!(responses[2][1]["list"].as_array().unwrap().len(), queried);
    assert_eq!(responses[3][0], "Email/get", "{}", responses[3]);
    assert_eq!(responses[3][1]["list"], responses[1][1]["list"]);
}

#[tokio::test]
async fn every_text_part_comes_under_fetch_all_body_values_rfc8621_4_2() {
    let (rig, _, by_name) = started().await;
    let arguments = json!({
        "accountId": ACCOUNT,
        "ids": [by_name["e1"]],
        "properties": ["bodyValues"],
        "fetchAllBodyValues": true,
        "bodyProperties": ["partId", "type"],
    });
    let answer = get(&rig, arguments).await;
    let values = answer["list"][0]["bodyValues"].as_object().unwrap();
    let mut parts: Vec<&String> = values.keys().collect();
    parts.sort();
    assert_eq!(parts, ["1.1", "1.2.1"]);
}

#[tokio::test]
async fn a_gmail_account_reads_its_bodies_from_the_store_folder() {
    let mut rig = Rig::over(Mailboxes::gmail(vec![corpus(CORPUS)])).await;
    rig.cache.gmail = true;
    rig.pass().await.unwrap();
    rig.sync(ALL_MAIL).await;
    let ids = rig.created(0);
    let answer = get(&rig, ask(&[&ids[0]], &BODY, CLIENT_CAP)).await;
    let email = &answer["list"][0];
    assert_eq!(email["bodyValues"]["1.2.1"]["value"], CORPUS_HTML);
    assert_eq!(email["attachments"].as_array().unwrap().len(), 3);
    let examined: Vec<String> = rig
        .fake
        .lines()
        .into_iter()
        .filter(|line| line.contains("EXAMINE"))
        .collect();
    assert!(!examined.is_empty());
    assert!(
        examined
            .iter()
            .all(|line| line.contains(&format!("\"{ALL_MAIL}\""))),
        "{examined:?}"
    );
}
