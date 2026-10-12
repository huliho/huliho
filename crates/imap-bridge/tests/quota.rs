// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! `Quota/get` and `Quota/changes` over what the mailbox pass reads of
//! GETQUOTAROOT: a quota the server cannot describe and a refusal first,
//! then the quotas as RFC 9425 has them, the state that moves with a
//! reading and the session object that lists the capability where the
//! server advertises QUOTA.

use std::sync::Arc;
use std::time::Duration;

use huliho_imap_bridge::jmap::{
    CORE_CAPABILITY, MAIL_CAPABILITY, QUOTA_CAPABILITY, Urls, handle, session_object,
};
use huliho_imap_bridge::mailboxes::{self, SyncError};
use huliho_imap_bridge::runtime::{Link, Registration};
use huliho_imap_bridge::session::{ImapSession, Session, TlsMode};
use huliho_imap_bridge::store::{AccountKey, Store};
use huliho_imap_bridge::sync::Cache;
use huliho_imap_bridge::testing::imap::{FakeImap, HOST, Script};
use huliho_imap_bridge::testing::mailboxes::{QuotaAnswer, quota_lines};
use huliho_imap_bridge::testing::seal::TestSealer;
use huliho_imap_bridge::testing::{Extension, Folder, Mailboxes, PASSWORD, TestConnector, USER};
use serde_json::{Value, json};

/// Room for a loopback exchange.
const STEP: Duration = Duration::from_secs(2);

/// The one account of the rig.
const ACCOUNT: &str = "a1";

/// The octets in one unit of STORAGE (RFC 9208 section 5.1).
const KIB: u64 = 1024;

/// The state the host hands the rig's requests.
const HOST_STATE: &str = "host-state";

/// The scripted server, the cache its passes fill and the link a
/// request reaches the server through, refreshing on every `/changes`.
struct Rig {
    cache: Cache,
    link: Link<TestConnector>,
    fake: FakeImap,
}

impl Rig {
    /// The server over the model with an empty cache.
    async fn over(mailboxes: Mailboxes) -> Self {
        let script = Script {
            mailboxes,
            ..Script::tls()
        };
        let fake = FakeImap::start(script).await;
        let target = fake.target(HOST, TlsMode::Implicit);
        let connector = TestConnector::scripted(fake.trusting(), target, STEP);
        let store = Arc::new(Store::in_memory().unwrap());
        let sealer = Arc::new(TestSealer::default());
        let key = AccountKey::new(ACCOUNT);
        Self {
            cache: Cache {
                store,
                sealer,
                key,
                gmail: false,
            },
            link: Link::with_interval(connector, Duration::ZERO),
            fake,
        }
    }

    /// The server over the model, listed once.
    async fn start(mailboxes: Mailboxes) -> Self {
        let rig = Self::over(mailboxes).await;
        rig.pass().await.unwrap();
        rig
    }

    /// One mailbox pass on a session of its own.
    async fn pass(&self) -> Result<u64, SyncError> {
        let target = self.fake.target(HOST, TlsMode::Implicit);
        let mut session = ImapSession::connect(self.fake.trusting(), &target, STEP)
            .await
            .unwrap();
        session.login(USER, PASSWORD).await.unwrap();
        mailboxes::sync(&mut session, &self.cache).await
    }

    /// Whether the rows hold a mailbox of that wire name.
    fn lists(&self, imap_name: &str) -> bool {
        let snapshot = self.cache.store.mailbox_snapshot(&self.cache.key);
        let rows = snapshot.unwrap().rows;
        rows.iter().any(|row| row.facts.imap_name == imap_name)
    }
}

/// A Dovecot-shaped server behind QUOTA that answers these lines.
fn quota_server(lines: String) -> Mailboxes {
    let mut mailboxes = Mailboxes::dovecot();
    mailboxes.extensions.insert(Extension::Quota);
    mailboxes.set_quota(QuotaAnswer::Lines(lines));
    mailboxes
}

/// A root of a GiB and ten thousand messages with this storage used,
/// in units of 1024 octets.
fn user_quota(used: u64) -> String {
    quota_lines("User quota", (used, KIB * KIB), (17, 10_000))
}

/// The Response object to these calls under these capabilities.
async fn request(rig: &Rig, using: &[&str], calls: Value) -> Value {
    let body = json!({ "using": using, "methodCalls": calls });
    let bytes = handle(
        &rig.cache,
        &rig.link,
        &serde_json::to_vec(&body).unwrap(),
        HOST_STATE,
    )
    .await
    .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

/// The first response under core, mail and quota.
async fn quota_call(rig: &Rig, method: &str, arguments: Value) -> Value {
    let using = [CORE_CAPABILITY, MAIL_CAPABILITY, QUOTA_CAPABILITY];
    let mut response = request(rig, &using, json!([[method, arguments, "c1"]])).await;
    response["methodResponses"][0].take()
}

async fn get(rig: &Rig) -> Value {
    let arguments = json!({ "accountId": ACCOUNT, "ids": null });
    let found = quota_call(rig, "Quota/get", arguments).await;
    assert_eq!(found[0], "Quota/get", "{found}");
    found[1].clone()
}

/// The session object of the rig's account as the host would serve it.
fn session(rig: &Rig) -> Value {
    let registration = Registration {
        key: AccountKey::new(ACCOUNT),
        gmail: false,
        session_state: HOST_STATE.to_owned(),
    };
    let urls = Urls {
        api: "/api/jmap/a1".to_owned(),
        download: "/api/jmap/a1/download".to_owned(),
        upload: "/api/jmap/a1/upload".to_owned(),
        event_source: "/api/jmap/a1/events".to_owned(),
    };
    let bytes = session_object(&registration, "sanne@example.test", &urls, &rig.cache.store);
    serde_json::from_slice(&bytes.unwrap()).unwrap()
}

#[tokio::test]
async fn a_quota_past_its_bound_fails_the_pass_after_its_rows_and_keeps_the_last_reading_rfc9208_7()
{
    let model = quota_server(user_quota(420));
    let rig = Rig::start(model.clone()).await;
    let before = get(&rig).await;
    let past = "* QUOTAROOT INBOX \"\"\r\n* QUOTA \"\" (STORAGE 1 9223372036854775808)\r\n";
    model.set_quota(QuotaAnswer::Lines(past.to_owned()));
    model.push(Folder::new("Work"));
    assert!(rig.pass().await.is_err());
    // The rows of the pass stand; the quota stands as the last good read.
    assert!(rig.lists("Work"));
    assert_eq!(get(&rig).await, before);
    // The refresh reads the quota after its folders and leaves it as well.
    let calls = json!([["Mailbox/changes", { "accountId": ACCOUNT, "sinceState": "0" }, "c1"]]);
    let using = [CORE_CAPABILITY, MAIL_CAPABILITY];
    assert_eq!(
        request(&rig, &using, calls).await["methodResponses"][0][0],
        "Mailbox/changes"
    );
    assert_eq!(get(&rig).await, before);
}

#[tokio::test]
async fn a_refused_getquotaroot_reads_as_an_inbox_under_no_root() {
    let model = quota_server(user_quota(1));
    model.set_quota(QuotaAnswer::Refused);
    let rig = Rig::start(model).await;
    assert_eq!(get(&rig).await["list"], json!([]));
    let account = &session(&rig)["accounts"][ACCOUNT]["accountCapabilities"];
    assert_eq!(account[QUOTA_CAPABILITY], json!({}));
}

#[tokio::test]
async fn an_account_whose_server_has_no_quota_answers_account_not_supported_rfc8620_3_6_2() {
    let rig = Rig::start(Mailboxes::dovecot()).await;
    for (method, arguments) in [
        ("Quota/get", json!({ "accountId": ACCOUNT, "ids": null })),
        (
            "Quota/changes",
            json!({ "accountId": ACCOUNT, "sinceState": "x" }),
        ),
    ] {
        let answer = quota_call(&rig, method, arguments).await;
        assert_eq!(answer[0], "error", "{answer}");
        assert_eq!(answer[1]["type"], "accountNotSupportedByMethod");
    }
    let session = session(&rig);
    assert_eq!(session["capabilities"][QUOTA_CAPABILITY], json!({}));
    let account = &session["accounts"][ACCOUNT]["accountCapabilities"];
    assert_eq!(account.get(QUOTA_CAPABILITY), None);
    assert_eq!(session["state"], HOST_STATE);
    let answer = request(&rig, &[CORE_CAPABILITY], json!([["Core/echo", {}, "c1"]])).await;
    assert_eq!(answer["sessionState"], HOST_STATE);
}

#[tokio::test]
async fn the_quota_methods_need_their_capability_and_the_two_unserved_ones_are_unknown() {
    let rig = Rig::start(quota_server(user_quota(1))).await;
    let arguments = json!({ "accountId": ACCOUNT, "ids": null });
    let calls = json!([["Quota/get", arguments, "c1"]]);
    let answer = request(&rig, &[CORE_CAPABILITY, MAIL_CAPABILITY], calls).await;
    assert_eq!(answer["methodResponses"][0][1]["type"], "unknownMethod");
    for method in ["Quota/query", "Quota/queryChanges"] {
        let answer = quota_call(&rig, method, json!({ "accountId": ACCOUNT })).await;
        assert_eq!(answer[1]["type"], "unknownMethod", "{method}");
    }
}

#[tokio::test]
async fn every_root_and_resource_is_a_quota_of_the_account_over_its_email_rfc9425_4_1() {
    let rig = Rig::start(quota_server(user_quota(420))).await;
    let found = get(&rig).await;
    assert_eq!(found["accountId"], ACCOUNT);
    assert_eq!(found["notFound"], json!([]));
    let list = found["list"].as_array().unwrap();
    assert_eq!(list.len(), 2, "{found}");
    let (octets, count) = (&list[0], &list[1]);
    assert_eq!(octets["resourceType"], "octets");
    assert_eq!(octets["used"], 420 * KIB);
    assert_eq!(octets["hardLimit"], KIB * KIB * KIB);
    assert_eq!(count["resourceType"], "count");
    assert_eq!(count["used"], 17);
    assert_eq!(count["hardLimit"], 10_000);
    for quota in list {
        assert_eq!(quota["scope"], "account");
        assert_eq!(quota["name"], "User quota");
        assert_eq!(quota["types"], json!(["Email"]));
        assert_eq!(quota["warnLimit"], Value::Null);
        // RFC 8620 section 1.2: an id in the URL-safe alphabet.
        let id = quota["id"].as_str().unwrap();
        assert!(
            id.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        );
    }
    assert_ne!(octets["id"], count["id"]);
    let ids = json!([count["id"], "q-none", count["id"]]);
    let arguments = json!({ "accountId": ACCOUNT, "ids": ids, "properties": ["used"] });
    let named = quota_call(&rig, "Quota/get", arguments).await;
    assert_eq!(named[1]["list"], json!([{ "id": count["id"], "used": 17 }]));
    assert_eq!(named[1]["notFound"], json!(["q-none"]));
    let arguments = json!({ "accountId": ACCOUNT, "ids": null, "properties": ["size"] });
    let unknown = quota_call(&rig, "Quota/get", arguments).await;
    assert_eq!(unknown[1]["type"], "invalidArguments");
}

#[tokio::test]
async fn a_request_without_the_mail_capability_sees_no_quota_over_email_rfc9425_4_1() {
    let rig = Rig::start(quota_server(user_quota(1))).await;
    let id = get(&rig).await["list"][0]["id"].clone();
    let calls = json!([
        ["Quota/get", { "accountId": ACCOUNT, "ids": null }, "c1"],
        ["Quota/get", { "accountId": ACCOUNT, "ids": [id] }, "c2"],
    ]);
    let answer = request(&rig, &[CORE_CAPABILITY, QUOTA_CAPABILITY], calls).await;
    assert_eq!(answer["methodResponses"][0][1]["list"], json!([]));
    assert_eq!(answer["methodResponses"][1][1]["notFound"], json!([id]));
}

#[tokio::test]
async fn the_state_moves_with_a_reading_and_changes_know_the_current_one_alone_rfc9425_4_3() {
    let model = quota_server(user_quota(420));
    let rig = Rig::start(model.clone()).await;
    let first = get(&rig).await;
    let state = first["state"].as_str().unwrap().to_owned();
    let since = |state: &str| json!({ "accountId": ACCOUNT, "sinceState": state });
    let same = quota_call(&rig, "Quota/changes", since(&state)).await;
    assert_eq!(same[0], "Quota/changes", "{same}");
    assert_eq!(
        same[1],
        json!({
            "accountId": ACCOUNT,
            "oldState": state,
            "newState": state,
            "hasMoreChanges": false,
            "created": [],
            "updated": [],
            "destroyed": [],
            "updatedProperties": null,
        })
    );
    // The usage grows; the refresh a `/changes` call runs reads it.
    model.set_quota(QuotaAnswer::Lines(user_quota(421)));
    let moved = quota_call(&rig, "Quota/changes", since(&state)).await;
    assert_eq!(moved[1]["type"], "cannotCalculateChanges", "{moved}");
    let second = get(&rig).await;
    assert_ne!(second["state"], first["state"]);
    assert_eq!(second["list"][0]["used"], 421 * KIB);
    assert_eq!(second["list"][0]["id"], first["list"][0]["id"]);
    // A state another store issued, as before a restart, is unknown.
    let other = Store::in_memory().unwrap();
    let foreign = other.quota(&AccountKey::new(ACCOUNT)).state;
    for stale in [foreign.as_str(), "7", ""] {
        let answer = quota_call(&rig, "Quota/changes", since(stale)).await;
        assert_eq!(answer[1]["type"], "cannotCalculateChanges", "{stale}");
    }
    let capped = json!({ "accountId": ACCOUNT, "sinceState": state, "maxChanges": 0 });
    let answer = quota_call(&rig, "Quota/changes", capped).await;
    assert_eq!(answer[1]["type"], "invalidArguments");
}

#[tokio::test]
async fn a_changes_call_under_core_and_quota_alone_refreshes_the_account_first() {
    let model = quota_server(user_quota(420));
    let rig = Rig::start(model.clone()).await;
    let first = get(&rig).await;
    assert_eq!(first["list"][0]["used"], 420 * KIB);
    let state = first["state"].as_str().unwrap().to_owned();
    model.set_quota(QuotaAnswer::Lines(user_quota(421)));
    let calls = json!([["Quota/changes", { "accountId": ACCOUNT, "sinceState": state }, "c1"]]);
    let answer = request(&rig, &[CORE_CAPABILITY, QUOTA_CAPABILITY], calls).await;
    let changes = &answer["methodResponses"][0];
    assert_eq!(changes[1]["type"], "cannotCalculateChanges", "{changes}");
    // The refresh ahead of the call read the usage; a `/get` reads nothing anew.
    let second = get(&rig).await;
    assert_ne!(second["state"], first["state"]);
    assert_eq!(second["list"][0]["used"], 421 * KIB);
}

#[tokio::test]
async fn the_session_lists_quota_on_the_account_once_a_pass_found_it_and_the_state_follows() {
    let rig = Rig::over(quota_server(user_quota(1))).await;
    let before = session(&rig);
    assert_eq!(before["state"], HOST_STATE);
    let account = &before["accounts"][ACCOUNT]["accountCapabilities"];
    assert_eq!(account.get(QUOTA_CAPABILITY), None);
    rig.pass().await.unwrap();
    let after = session(&rig);
    assert_ne!(after["state"], HOST_STATE);
    let account = &after["accounts"][ACCOUNT]["accountCapabilities"];
    assert_eq!(account[QUOTA_CAPABILITY], json!({}));
    let answer = request(&rig, &[CORE_CAPABILITY], json!([["Core/echo", {}, "c1"]])).await;
    assert_eq!(answer["sessionState"], after["state"]);
}
