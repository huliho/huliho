// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The sanitizer pass on the request route of a native account: which
//! requests take it, which are refused before anything connects, what a
//! body answer looks like afterwards and what a header window looks
//! like without it. The bridge's own tests cover its path.

mod answers;
mod common;
mod fake_dns;
mod html_check;
mod jmap_upstream;
mod proxy_rig;
mod readers;
mod signin;
mod tls_server;

use axum::body::Body;
use axum::http::{Method, StatusCode, header};
use huliho_imap_bridge::testing::TOKEN;
use huliho_server::accounts::Credential;
use huliho_server::gate::RUN_WINDOW;
use jmap_upstream::{Script, UPSTREAM_ACCOUNT};
use proxy_rig::{Instance, Setup, query_request};
use serde_json::{Value, json};
use signin::{body_text, with_cookie};
use tower::ServiceExt;

const CORE: &str = "urn:ietf:params:jmap:core";
const MAIL: &str = "urn:ietf:params:jmap:mail";
const STATE: &str = "75128aab4b1b";

/// The body value bound the client sends: 4 MiB.
const MAX_BODY_VALUE_BYTES: usize = 4 * 1024 * 1024;

/// A mail as a sender wrote it: a script, an event handler, a form, an
/// iframe, a base, a meta refresh, a javascript link, a link into the
/// instance, a plain link, a remote image, an image at the API and a
/// table with a remote and a relative background.
const HOSTILE: &str = "<script>top.__x=1</script><p onclick=\"top.__x=1\">Hello</p>\
    <form action=\"https://evil.example/\"><input name=\"q\"></form>\
    <iframe src=\"https://evil.example/\"></iframe><base href=\"https://evil.example/\">\
    <meta http-equiv=\"refresh\" content=\"0;url=https://evil.example/\">\
    <a href=\"javascript:top.__x=1\">click</a><a href=\"/settings\">own</a>\
    <a href=\"https://x.example/\" target=\"_top\">link</a>\
    <img src=\"https://cdn.example/pixel.gif\"><img src=\"/api/session\">\
    <table background=\"https://cdn.example/bg.png\"><tr><td background=\"bg.png\">x</td></tr></table>";

/// What a browser builds from the sanitized mail: the text and the
/// harmless parts stay, every link opens in a new tab, the URLs into
/// the instance and the relative one are gone.
const CLEAN_SHAPE: &[&str] = &[
    "p",
    "a rel=noopener noreferrer target=_blank",
    "a rel=noopener noreferrer target=_blank",
    "a href=https://x.example/ rel=noopener noreferrer target=_blank",
    "img src=https://cdn.example/pixel.gif",
    "img",
    "table background=https://cdn.example/bg.png",
    "tbody",
    "tr",
    "td",
];

/// A plain part with the characters a sanitizer would rewrite.
const PLAIN: &str = "a < b & c <script>";

/// A Response object with a byte layout no serializer of this server
/// produces, so an answer that comes back unchanged proves the
/// pass-through path.
const ODD_BYTES: &str = "{ \"sessionState\" : \"75128aab4b1b\", \"methodResponses\" : [ [\"Email/get\", {\"list\": [ ]}, \"c1\"] ] }";

fn bearer() -> Credential {
    Credential::Bearer {
        token: TOKEN.to_owned(),
    }
}

async fn instance() -> Instance {
    Instance::start(Setup {
        window: RUN_WINDOW,
        servers: &[],
    })
    .await
}

fn using(calls: &Value) -> Value {
    json!({ "using": [CORE, MAIL], "methodCalls": calls })
}

/// The body request the client sends when a card opens.
fn body_request(properties: &Value) -> Value {
    using(&json!([["Email/get", {
        "accountId": UPSTREAM_ACCOUNT,
        "ids": ["e1"],
        "properties": properties,
        "fetchTextBodyValues": true,
        "fetchHTMLBodyValues": true,
        "maxBodyValueBytes": MAX_BODY_VALUE_BYTES
    }, "c1"]]))
}

fn full_properties() -> Value {
    json!([
        "id",
        "subject",
        "bodyStructure",
        "textBody",
        "htmlBody",
        "attachments",
        "bodyValues"
    ])
}

/// The answer a server gives that request for a mail with a text and an
/// HTML part.
fn body_answer() -> Vec<u8> {
    json!({
        "methodResponses": [
            ["Email/get", {
                "accountId": UPSTREAM_ACCOUNT,
                "state": "s1",
                "list": [{
                    "id": "e1",
                    "subject": "Hostile",
                    "textBody": [{ "partId": "1", "type": "text/plain" }],
                    "htmlBody": [{ "partId": "2", "type": "text/html" }],
                    "attachments": [],
                    "bodyValues": {
                        "1": { "value": PLAIN, "isEncodingProblem": false, "isTruncated": false },
                        "2": { "value": HOSTILE, "isEncodingProblem": false, "isTruncated": false }
                    }
                }],
                "notFound": []
            }, "c1"]
        ],
        "sessionState": STATE
    })
    .to_string()
    .into_bytes()
}

fn posts(instance: &Instance) -> usize {
    instance
        .upstream
        .lines()
        .iter()
        .filter(|line| line.contains("/jmap/api"))
        .count()
}

/// The request route's answer as text, before any JSON parse.
async fn raw(instance: &Instance, cookie: &str, id: &str, body: &Value) -> (StatusCode, String) {
    let mut request = with_cookie(Method::POST, &format!("/api/jmap/{id}"), cookie);
    request
        .headers_mut()
        .insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
    *request.body_mut() = Body::from(body.to_string());
    let response = instance.router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    (status, body_text(response).await)
}

#[tokio::test]
async fn another_users_account_is_not_found_on_a_body_request_and_nothing_connects() {
    let instance = instance().await;
    let id = instance.add_account(bearer());
    let other = instance.sign_in_other().await;
    let (status, body) = instance.session(&other, &id).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (status, body) = instance
        .request(&other, &id, &body_request(&full_properties()))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert!(instance.upstream.lines().is_empty());
    assert_eq!(
        instance.account_events(),
        [(
            "account.linked".to_owned(),
            instance.user_id().as_str().to_owned()
        )]
    );
}

#[tokio::test]
async fn a_body_answer_comes_back_with_its_html_values_clean_and_the_rest_untouched() {
    let instance = instance().await;
    let id = instance.add_account(bearer());
    let cookie = instance.sign_in().await;
    instance.upstream.set(Script {
        answer: Some(body_answer()),
        ..Script::echo(instance.upstream.port())
    });
    let (status, answer) = instance
        .request(&cookie, &id, &body_request(&full_properties()))
        .await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let email = &answer["methodResponses"][0][1]["list"][0];
    let html = email["bodyValues"]["2"]["value"].as_str().unwrap();
    html_check::check(html, None).unwrap_or_else(|problem| panic!("{problem}\n{html}"));
    assert_eq!(html_check::shape(html), CLEAN_SHAPE, "{html}");
    assert!(html.contains("<p>Hello</p>"), "{html}");
    for gone in [
        "top.__x",
        "javascript:",
        "/settings",
        "/api/session",
        "bg.png\">x",
    ] {
        assert!(!html.contains(gone), "{gone} in {html}");
    }
    assert_eq!(email["bodyValues"]["1"]["value"], PLAIN);
    assert_eq!(email["bodyValues"]["2"]["isTruncated"], false);
    assert_eq!(email["subject"], "Hostile");
    assert_eq!(answer["methodResponses"][0][1]["state"], "s1");
    assert_eq!(answer["sessionState"], STATE);
    assert_eq!(posts(&instance), 1);
}

#[tokio::test]
async fn a_body_ask_without_html_body_or_with_fetch_all_is_refused_before_anything_connects() {
    let instance = instance().await;
    let id = instance.add_account(bearer());
    let cookie = instance.sign_in().await;
    let mut fetch_all = body_request(&full_properties());
    fetch_all["methodCalls"][0][1]["fetchAllBodyValues"] = json!(true);
    for request in [
        body_request(&json!(["id", "textBody", "bodyValues"])),
        body_request(&json!(["id", "subject"])),
        fetch_all,
    ] {
        let (status, body) = instance.request(&cookie, &id, &request).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{request}: {body}");
        assert_eq!(body["error"], "invalid_request", "{request}");
    }
    assert!(instance.upstream.lines().is_empty());
}

#[tokio::test]
async fn an_omitted_properties_list_passes_and_a_header_window_passes_through_untouched() {
    let instance = instance().await;
    let id = instance.add_account(bearer());
    let cookie = instance.sign_in().await;
    instance.upstream.set(Script {
        answer: Some(body_answer()),
        ..Script::echo(instance.upstream.port())
    });
    let mut omitted = body_request(&Value::Null);
    omitted["methodCalls"][0][1]
        .as_object_mut()
        .unwrap()
        .remove("properties");
    let (status, answer) = instance.request(&cookie, &id, &omitted).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let html = answer["methodResponses"][0][1]["list"][0]["bodyValues"]["2"]["value"]
        .as_str()
        .unwrap();
    assert!(html_check::check(html, None).is_ok(), "{html}");
    assert_eq!(posts(&instance), 1);
    instance.upstream.set(Script {
        answer: Some(ODD_BYTES.as_bytes().to_vec()),
        ..Script::echo(instance.upstream.port())
    });
    let (status, text) = raw(&instance, &cookie, &id, &query_request()).await;
    assert_eq!(status, StatusCode::OK, "{text}");
    assert_eq!(text, ODD_BYTES);
    assert_eq!(posts(&instance), 2);
}

#[tokio::test]
async fn an_answer_that_is_no_response_object_to_a_body_ask_is_not_usable() {
    let instance = instance().await;
    let id = instance.add_account(bearer());
    let cookie = instance.sign_in().await;
    instance.upstream.set(Script {
        answer: Some(b"[\"not\", \"a\", \"response\"]".to_vec()),
        ..Script::echo(instance.upstream.port())
    });
    let (status, body) = instance
        .request(&cookie, &id, &body_request(&full_properties()))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "upstream_unsupported");
    assert_eq!(instance.stopped_cause(&id), None);
}
