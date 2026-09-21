// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! `Email/query` and `Thread/get` over a synced folder: the windows, the
//! sorts, the collapsed threads and the total as one snapshot, every
//! refusal and one request that walks the three result references.

mod stable_ids;
mod sync_rig;

use std::collections::HashMap;

use huliho_imap_bridge::jmap::{MAX_OBJECTS_IN_GET, QUERY_LIMIT};
use huliho_imap_bridge::testing::Message;
use serde_json::{Map, Value, json};
use sync_rig::{ACCOUNT, Rig, inbox};

/// A reply to message `to`, in its thread.
fn reply(uid: u32, to: u32) -> Message {
    Message {
        header: format!(
            "From: Sanne <sanne@example.test>\r\nTo: mo@example.test\r\nSubject: Re: Message {to}\r\nMessage-ID: <m{uid}@example.test>\r\nIn-Reply-To: <m{to}@example.test>\r\n\r\n"
        ),
        ..Message::new(uid)
    }
}

/// Six messages in three threads: 1, 2 and 6 answer each other, 3 and 4
/// do, 5 stands alone.
fn dataset() -> Vec<Message> {
    vec![
        Message::new(1),
        reply(2, 1),
        Message::new(3),
        reply(4, 3),
        Message::new(5),
        reply(6, 1),
    ]
}

/// A synced INBOX holding the dataset and the names of its ids.
struct Synced {
    rig: Rig,
    names: HashMap<String, String>,
}

impl Synced {
    async fn start() -> Self {
        let rig = Rig::start(inbox(dataset())).await;
        rig.sync("INBOX").await;
        let ids = rig.created(0);
        let arguments =
            json!({ "accountId": ACCOUNT, "ids": ids, "properties": ["size", "threadId"] });
        let emails = rig.call(&json!(["Email/get", arguments, "c1"])).await["list"].take();
        let names = stable_ids::names(&rig, emails.as_array().unwrap());
        Self { rig, names }
    }

    /// The id behind a name.
    fn id(&self, name: &str) -> String {
        self.names
            .iter()
            .find(|(_, known)| *known == name)
            .map(|(id, _)| id.clone())
            .unwrap()
    }

    /// Arguments over the INBOX with these on top.
    fn over_inbox(&self, extra: Value) -> Value {
        let mut arguments =
            json!({ "accountId": ACCOUNT, "filter": { "inMailbox": self.id("INBOX") } });
        let Value::Object(extra) = extra else {
            unreachable!()
        };
        arguments.as_object_mut().unwrap().extend(extra);
        arguments
    }

    /// One call, its answer with every id renamed.
    async fn call(&self, method: &str, arguments: Value) -> Value {
        let mut answer = self.rig.call(&json!([method, arguments, "c1"])).await;
        stable_ids::rename(&mut answer, &self.names);
        answer
    }
}

#[tokio::test]
async fn email_query_windows_sorts_collapses_and_counts_rfc8621_4_4() {
    let synced = Synced::start().await;
    let e3 = synced.id("e3");
    let cases = [
        (
            "newest first with the total",
            json!({ "calculateTotal": true }),
        ),
        (
            "oldest first",
            json!({ "sort": [{ "property": "receivedAt", "isAscending": true }] }),
        ),
        ("a window by position", json!({ "position": 1, "limit": 2 })),
        ("a window from the end", json!({ "position": -2 })),
        (
            "a window by anchor",
            json!({ "anchor": e3, "anchorOffset": -1, "limit": 2 }),
        ),
        (
            "one email per thread",
            json!({ "collapseThreads": true, "calculateTotal": true }),
        ),
        (
            "one email per thread oldest first",
            json!({ "collapseThreads": true, "sort": [{ "property": "receivedAt" }] }),
        ),
        (
            "a limit past the server's",
            json!({ "limit": QUERY_LIMIT + 1 }),
        ),
    ];
    let mut answers = Map::new();
    for (name, extra) in cases {
        let answer = synced.call("Email/query", synced.over_inbox(extra)).await;
        answers.insert(name.to_owned(), answer);
    }
    insta::assert_json_snapshot!(Value::Object(answers));
}

#[tokio::test]
async fn email_query_refuses_what_it_does_not_serve_rfc8621_4_4() {
    let synced = Synced::start().await;
    let inbox = synced.id("INBOX");
    let cases = [
        (json!({ "accountId": ACCOUNT }), "unsupportedFilter"),
        (
            json!({ "accountId": ACCOUNT, "filter": null }),
            "unsupportedFilter",
        ),
        (
            json!({ "accountId": ACCOUNT, "filter": { "inMailbox": inbox, "hasKeyword": "$seen" } }),
            "unsupportedFilter",
        ),
        (
            json!({
                "accountId": ACCOUNT,
                "filter": { "operator": "AND", "conditions": [{ "inMailbox": inbox }] }
            }),
            "unsupportedFilter",
        ),
        (
            synced.over_inbox(json!({ "sort": [{ "property": "subject" }] })),
            "unsupportedSort",
        ),
        (
            synced.over_inbox(
                json!({ "sort": [{ "property": "receivedAt", "collation": "i;unicode-casemap" }] }),
            ),
            "unsupportedSort",
        ),
        (
            synced.over_inbox(json!({ "anchor": "e-unknown" })),
            "anchorNotFound",
        ),
        (
            synced.over_inbox(json!({ "limit": -1 })),
            "invalidArguments",
        ),
        (
            synced.over_inbox(json!({ "flavor": "vanilla" })),
            "invalidArguments",
        ),
        (
            json!({ "accountId": "other", "filter": { "inMailbox": inbox } }),
            "accountNotFound",
        ),
    ];
    for (arguments, expected) in cases {
        let answer = synced.call("Email/query", arguments).await;
        assert_eq!(answer["type"], expected, "{expected}");
    }
    let unknown = json!({
        "accountId": ACCOUNT,
        "filter": { "inMailbox": "m-nope" },
        "calculateTotal": true
    });
    let empty = synced.call("Email/query", unknown).await;
    assert_eq!((&empty["ids"], &empty["total"]), (&json!([]), &json!(0)));
}

#[tokio::test]
async fn one_request_walks_the_three_reference_paths_and_refuses_a_fourth_rfc8620_3_7() {
    let synced = Synced::start().await;
    let reference = |result_of: &str, name: &str, path: &str| json!({ "resultOf": result_of, "name": name, "path": path });
    let calls = [
        json!([
            "Email/query",
            synced.over_inbox(json!({ "collapseThreads": true })),
            "q"
        ]),
        json!([
            "Email/get",
            {
                "accountId": ACCOUNT,
                "#ids": reference("q", "Email/query", "/ids"),
                "properties": ["threadId"]
            },
            "g"
        ]),
        json!([
            "Thread/get",
            {
                "accountId": ACCOUNT,
                "#ids": reference("g", "Email/get", "/list/*/threadId")
            },
            "t"
        ]),
        json!([
            "Email/get",
            {
                "accountId": ACCOUNT,
                "#ids": reference("t", "Thread/get", "/list/*/emailIds"),
                "properties": ["keywords"]
            },
            "k"
        ]),
        json!([
            "Email/get",
            {
                "accountId": ACCOUNT,
                "#ids": reference("g", "Email/get", "/list/*/subject")
            },
            "x"
        ]),
    ];
    let mut response = synced.rig.calls(&calls).await;
    stable_ids::rename(&mut response, &synced.names);
    let responses = response["methodResponses"].as_array().unwrap();
    assert_eq!(responses[0][1]["ids"], json!(["e6", "e5", "e4"]));
    let threads: Vec<&Value> = responses[1][1]["list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|email| &email["threadId"])
        .collect();
    assert_eq!(threads, [&json!("t1"), &json!("t5"), &json!("t3")]);
    assert_eq!(
        responses[2][1]["list"],
        json!([
            { "id": "t1", "emailIds": ["e1", "e2", "e6"] },
            { "id": "t5", "emailIds": ["e5"] },
            { "id": "t3", "emailIds": ["e3", "e4"] }
        ])
    );
    assert_eq!(responses[3][1]["list"].as_array().unwrap().len(), 6);
    assert_eq!(responses[4][0], "error");
    assert_eq!(responses[4][1]["type"], "invalidResultReference");
    assert_eq!(responses[4][2], "x");
}

#[tokio::test]
async fn thread_get_answers_the_emails_oldest_first_and_names_what_it_lacks_rfc8621_3_1() {
    let synced = Synced::start().await;
    let t1 = synced.id("t1");
    let asked = json!({ "accountId": ACCOUNT, "ids": [t1, "t-nope", t1] });
    let answer = synced.call("Thread/get", asked).await;
    assert_eq!(
        answer["list"],
        json!([{ "id": "t1", "emailIds": ["e1", "e2", "e6"] }])
    );
    assert_eq!(answer["notFound"], json!(["t-nope"]));
    assert_eq!(answer["state"], "2");
    let named = json!({ "accountId": ACCOUNT, "ids": [t1], "properties": ["emailIds"] });
    let cut = synced.call("Thread/get", named).await;
    let fields: Vec<&String> = cut["list"][0].as_object().unwrap().keys().collect();
    assert_eq!(fields, ["emailIds", "id"]);
    let cases = [
        (
            json!({ "accountId": ACCOUNT, "ids": null }),
            "requestTooLarge",
        ),
        (
            json!({ "accountId": ACCOUNT, "ids": vec!["t"; MAX_OBJECTS_IN_GET + 1] }),
            "requestTooLarge",
        ),
        (
            json!({ "accountId": ACCOUNT, "ids": [t1], "properties": ["subject"] }),
            "invalidArguments",
        ),
        (
            json!({ "accountId": "other", "ids": [t1] }),
            "accountNotFound",
        ),
    ];
    for (arguments, expected) in cases {
        let answer = synced.call("Thread/get", arguments).await;
        assert_eq!(answer["type"], expected, "{expected}");
    }
}
