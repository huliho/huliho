// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The reference budget of one Request object: a chain inside it, a
//! reference past it and a full budget per request.

use std::sync::Arc;

use serde_json::json;

use super::super::CORE_CAPABILITY;
use super::*;
use crate::store::{AccountKey, Store};
use crate::testing::TestConnector;
use crate::testing::seal::TestSealer;

/// `Core/echo` calls: the first echoes 64 KiB of text, every later
/// one echoes the call before it twice through the path `""`.
fn doubling(calls: usize) -> Vec<Value> {
    let mut out = vec![json!(["Core/echo", { "text": "x".repeat(64 * 1024) }, "c0"])];
    for call in 1..calls {
        let before = json!({
            "resultOf": format!("c{}", call - 1),
            "name": "Core/echo",
            "path": ""
        });
        let twice = json!({ "#a": before, "#b": before });
        out.push(json!(["Core/echo", twice, format!("c{call}")]));
    }
    out
}

fn cache() -> Cache {
    Cache {
        store: Arc::new(Store::in_memory().unwrap()),
        sealer: Arc::new(TestSealer::default()),
        key: AccountKey::new("a1"),
        gmail: false,
    }
}

async fn responses(cache: &Cache, calls: &[Value]) -> Vec<Invocation> {
    let body = json!({ "using": [CORE_CAPABILITY], "methodCalls": calls });
    let body = serde_json::to_vec(&body).unwrap();
    let link = Link::new(TestConnector::Refusing);
    let answer = handle(cache, &link, &body, "s1").await.unwrap();
    let mut answer: Value = serde_json::from_slice(&answer).unwrap();
    assert_eq!(answer["sessionState"], "s1");
    serde_json::from_value(answer["methodResponses"].take()).unwrap()
}

#[tokio::test]
async fn references_past_the_budget_fail_their_call_alone_rfc8620_3_6_2() {
    let cache = cache();
    let mut calls = doubling(5);
    calls.push(json!(["Core/echo", { "after": true }, "c5"]));
    let responses = responses(&cache, &calls).await;
    for Invocation(name, _, _) in &responses[..4] {
        assert_eq!(name, "Core/echo");
    }
    let Invocation(name, error, id) = &responses[4];
    assert_eq!((name.as_str(), id.as_str()), ("error", "c4"));
    assert_eq!(error["type"], "invalidResultReference");
    assert!(error["description"].is_string());
    let Invocation(name, after, id) = &responses[5];
    assert_eq!((name.as_str(), id.as_str()), ("Core/echo", "c5"));
    assert_eq!(after["after"], true);
}

#[tokio::test]
async fn a_chain_of_references_inside_the_budget_resolves_rfc8620_3_7() {
    let cache = cache();
    let ids = |call: &str| json!({ "resultOf": call, "name": "Core/echo", "path": "/ids" });
    let calls = [
        json!(["Core/echo", { "ids": ["e1", "e2"] }, "c0"]),
        json!(["Core/echo", { "#ids": ids("c0") }, "c1"]),
        json!(["Core/echo", { "#ids": ids("c1") }, "c2"]),
    ];
    for Invocation(name, arguments, _) in responses(&cache, &calls).await {
        assert_eq!(name, "Core/echo");
        assert_eq!(arguments["ids"], json!(["e1", "e2"]));
    }
}

#[tokio::test]
async fn every_request_starts_with_a_full_budget() {
    let cache = cache();
    for _ in 0..2 {
        for Invocation(name, _, _) in responses(&cache, &doubling(4)).await {
            assert_eq!(name, "Core/echo");
        }
    }
}
