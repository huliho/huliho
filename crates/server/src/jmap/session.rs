// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The upstream session object as the browser may see it: the URLs
//! point at the proxy, the capabilities shrink to what the proxy carries
//! and the limits to what it enforces.

use serde_json::{Map, Value};

use super::{JMAP_REQUEST_LIMIT, MAX_CONCURRENT_REQUESTS};
use crate::ids::AccountId;

/// The core capability every JMAP server carries (RFC 8620 section 2).
pub(crate) const CORE_CAPABILITY: &str = "urn:ietf:params:jmap:core";

/// The capability every mail account needs (RFC 8621 section 1.1).
pub(crate) const MAIL_CAPABILITY: &str = "urn:ietf:params:jmap:mail";

/// The capabilities the proxy carries; every other one leaves the
/// session object, so no upstream host name reaches the browser.
const CARRIED: [&str; 2] = [CORE_CAPABILITY, MAIL_CAPABILITY];

/// The core limits the proxy enforces itself; the browser learns the
/// proxy's values where the upstream's are higher.
const ENFORCED: [(&str, usize); 2] = [
    ("maxSizeRequest", JMAP_REQUEST_LIMIT),
    ("maxConcurrentRequests", MAX_CONCURRENT_REQUESTS),
];

/// Rewrites `session` for the browser and answers the API endpoint the
/// upstream named; `None` when the object names none.
pub(super) fn rewrite(session: &mut Map<String, Value>, account_id: &AccountId) -> Option<String> {
    let api_url = session.get("apiUrl")?.as_str()?.to_owned();
    let id = account_id.as_str();
    let urls = [
        ("apiUrl", format!("/api/jmap/{id}")),
        (
            "downloadUrl",
            format!("/api/jmap/{id}/download/{{accountId}}/{{blobId}}/{{name}}?type={{type}}"),
        ),
        ("uploadUrl", format!("/api/jmap/{id}/upload/{{accountId}}")),
        (
            "eventSourceUrl",
            format!(
                "/api/jmap/{id}/events?types={{types}}&closeafter={{closeafter}}&ping={{ping}}"
            ),
        ),
    ];
    for (name, url) in urls {
        session.insert(name.to_owned(), Value::String(url));
    }
    carried_only(session.get_mut("capabilities"));
    let core = session
        .get_mut("capabilities")
        .and_then(|capabilities| capabilities.get_mut(CORE_CAPABILITY));
    clamp_limits(core);
    carried_only(session.get_mut("primaryAccounts"));
    if let Some(Value::Object(accounts)) = session.get_mut("accounts") {
        for account in accounts.values_mut() {
            carried_only(account.get_mut("accountCapabilities"));
        }
    }
    Some(api_url)
}

/// Keeps the entries keyed by a carried capability; a value that is not
/// an object stays as it is.
fn carried_only(value: Option<&mut Value>) {
    if let Some(map) = value.and_then(Value::as_object_mut) {
        map.retain(|key, _| CARRIED.contains(&key.as_str()));
    }
}

/// Lowers the enforced limits to the proxy's own where the upstream's
/// are higher, absent or not a number.
fn clamp_limits(core: Option<&mut Value>) {
    let Some(core) = core.and_then(Value::as_object_mut) else {
        return;
    };
    for (name, limit) in ENFORCED {
        let upstream = core
            .get(name)
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok());
        if upstream.is_none_or(|value| value > limit) {
            core.insert(name.to_owned(), Value::from(limit));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UPSTREAM: &str = r#"{
        "capabilities": {
            "urn:ietf:params:jmap:core": {
                "maxSizeRequest": 10000000,
                "maxConcurrentRequests": 8,
                "maxObjectsInGet": 500,
                "collationAlgorithms": ["i;unicode-casemap"]
            },
            "urn:ietf:params:jmap:mail": {},
            "urn:ietf:params:jmap:websocket": {
                "url": "wss://api.example.test/jmap/ws",
                "supportsPush": true
            }
        },
        "accounts": {
            "u1": {
                "name": "sanne@example.test",
                "isPersonal": true,
                "isReadOnly": false,
                "accountCapabilities": {
                    "urn:ietf:params:jmap:core": {},
                    "urn:ietf:params:jmap:mail": {"maxMailboxDepth": null},
                    "urn:ietf:params:jmap:websocket": {}
                }
            }
        },
        "primaryAccounts": {
            "urn:ietf:params:jmap:mail": "u1",
            "urn:ietf:params:jmap:websocket": "u1"
        },
        "username": "sanne@example.test",
        "apiUrl": "https://api.example.test/jmap/api",
        "downloadUrl": "https://api.example.test/jmap/download/{accountId}/{blobId}/{name}?accept={type}",
        "uploadUrl": "https://api.example.test/jmap/upload/{accountId}/",
        "eventSourceUrl": "https://api.example.test/jmap/eventsource/?types={types}&closeafter={closeafter}&ping={ping}",
        "state": "75128aab4b1b"
    }"#;

    fn account() -> AccountId {
        AccountId::from("a1".to_owned())
    }

    fn rewritten() -> (Map<String, Value>, String) {
        let mut session: Map<String, Value> = serde_json::from_str(UPSTREAM).unwrap();
        let api_url = rewrite(&mut session, &account()).unwrap();
        (session, api_url)
    }

    fn keys(value: &Value) -> Vec<&str> {
        value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect()
    }

    #[test]
    fn the_four_urls_point_at_the_proxy_and_the_upstream_endpoint_comes_back() {
        let (session, api_url) = rewritten();
        assert_eq!(api_url, "https://api.example.test/jmap/api");
        assert_eq!(session["apiUrl"], "/api/jmap/a1");
        assert_eq!(
            session["downloadUrl"],
            "/api/jmap/a1/download/{accountId}/{blobId}/{name}?type={type}"
        );
        assert_eq!(session["uploadUrl"], "/api/jmap/a1/upload/{accountId}");
        assert_eq!(
            session["eventSourceUrl"],
            "/api/jmap/a1/events?types={types}&closeafter={closeafter}&ping={ping}"
        );
        assert_eq!(session["state"], "75128aab4b1b");
        assert_eq!(session["username"], "sanne@example.test");
        assert_eq!(session["accounts"]["u1"]["name"], "sanne@example.test");
    }

    #[test]
    fn only_the_carried_capabilities_survive_rfc8620_2() {
        let (session, _) = rewritten();
        assert_eq!(keys(&session["capabilities"]), CARRIED);
        assert_eq!(
            keys(&session["accounts"]["u1"]["accountCapabilities"]),
            CARRIED
        );
        assert_eq!(keys(&session["primaryAccounts"]), [MAIL_CAPABILITY]);
        assert_eq!(
            session["accounts"]["u1"]["accountCapabilities"][MAIL_CAPABILITY],
            serde_json::json!({"maxMailboxDepth": null})
        );
        let text = serde_json::to_string(&session).unwrap();
        assert!(!text.contains("websocket"), "{text}");
        assert!(!text.contains("api.example.test"), "{text}");
    }

    #[test]
    fn the_enforced_limits_come_back_as_the_proxys_own_and_the_rest_stays() {
        let (session, _) = rewritten();
        let core = &session["capabilities"][CORE_CAPABILITY];
        assert_eq!(core["maxSizeRequest"], JMAP_REQUEST_LIMIT);
        assert_eq!(core["maxConcurrentRequests"], MAX_CONCURRENT_REQUESTS);
        assert_eq!(core["maxObjectsInGet"], 500);
        assert_eq!(
            core["collationAlgorithms"],
            serde_json::json!(["i;unicode-casemap"])
        );
        let mut lower: Map<String, Value> = serde_json::from_str(UPSTREAM).unwrap();
        lower["capabilities"][CORE_CAPABILITY]["maxConcurrentRequests"] = Value::from(2);
        rewrite(&mut lower, &account()).unwrap();
        assert_eq!(
            lower["capabilities"][CORE_CAPABILITY]["maxConcurrentRequests"],
            2
        );
    }

    #[test]
    fn an_object_without_an_api_url_is_not_a_session_object() {
        let mut bare: Map<String, Value> = serde_json::from_str(r#"{"capabilities":{}}"#).unwrap();
        assert_eq!(rewrite(&mut bare, &account()), None);
        let mut odd: Map<String, Value> = serde_json::from_str(r#"{"apiUrl":7}"#).unwrap();
        assert_eq!(rewrite(&mut odd, &account()), None);
    }

    #[test]
    fn the_mail_capability_is_the_one_the_check_reads() {
        assert_eq!(MAIL_CAPABILITY, "urn:ietf:params:jmap:mail");
    }
}
