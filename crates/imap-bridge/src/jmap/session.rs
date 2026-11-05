// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The session object of one bridge account (RFC 8620 section 2): the
//! core, mail, quota and submission capabilities with the bridge's
//! limits, the vendor capability and the state the host derived.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{Map, Value, json};

use super::{
    MAX_CALLS_IN_REQUEST, MAX_CONCURRENT_REQUESTS, MAX_OBJECTS_IN_GET, MAX_OBJECTS_IN_SET,
    MAX_SIZE_REQUEST,
};
use crate::runtime::Registration;
use crate::session::MAX_MAILBOX_NAME_BYTES;
use crate::store::{Store, StoreError};
use crate::upload::{MAX_ATTACHMENTS_BYTES, MAX_CONCURRENT_UPLOADS};

/// The core capability (RFC 8620 section 2).
pub const CORE_CAPABILITY: &str = "urn:ietf:params:jmap:core";

/// The mail capability (RFC 8621 section 1.3.1).
pub const MAIL_CAPABILITY: &str = "urn:ietf:params:jmap:mail";

/// The quota capability (RFC 9425 section 2.1).
pub const QUOTA_CAPABILITY: &str = "urn:ietf:params:jmap:quota";

/// The submission capability (RFC 8621 section 1.3.2).
pub const SUBMISSION_CAPABILITY: &str = "urn:ietf:params:jmap:submission";

/// A send goes at once: the bridge holds nothing back (RFC 8621 section
/// 1.3.2).
const MAX_DELAYED_SEND: u64 = 0;

/// The vendor capability (RFC 8620 section 1.8) on the product domain;
/// it carries the Mailbox property `syncedEmails`.
pub const HULIHO_CAPABILITY: &str = "https://huliho.com/jmap";

/// What the state of the session object gains while the server behind
/// the account advertises QUOTA, since the object then lists the quota
/// capability on the account.
const QUOTA_STATE_MARK: &str = "-quota";

/// The one collation RFC 8620 section 2 requires of every server.
const COLLATION: &str = "i;unicode-casemap";

/// One folder per email outside Gmail, where a copy is a second email;
/// on Gmail a message sits under any number of labels.
const MAX_MAILBOXES_PER_EMAIL: u32 = 1;

/// The one sort the account advertises for `Email/query`.
const EMAIL_QUERY_SORT_OPTIONS: [&str; 1] = ["receivedAt"];

/// The URLs the host serves the account on; the bridge does not know
/// where it is mounted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Urls {
    pub api: String,
    pub download: String,
    pub upload: String,
    pub event_source: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionObject<'a> {
    capabilities: BTreeMap<&'static str, Value>,
    accounts: BTreeMap<&'a str, AccountObject>,
    primary_accounts: BTreeMap<&'static str, &'a str>,
    username: &'a str,
    api_url: &'a str,
    download_url: &'a str,
    upload_url: &'a str,
    event_source_url: &'a str,
    state: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountObject {
    name: String,
    is_personal: bool,
    is_read_only: bool,
    account_capabilities: BTreeMap<&'static str, Value>,
}

/// The session object for one account as JSON: writable, since
/// `Email/set` takes keywords, the vendor capability on the account and
/// the quota capability where the server advertises QUOTA, the state the
/// host derived with that word on it, the account's address as its name
/// and the username. No row is read, so the object never moves with the
/// cache.
///
/// # Errors
///
/// Returns the store's encoding failure when the object cannot be
/// encoded.
pub fn session_object(
    registration: &Registration,
    urls: &Urls,
    store: &Store,
) -> Result<Vec<u8>, StoreError> {
    let key = &registration.key;
    let address = registration.address.as_str();
    let quota = store.quota(key).advertised;
    let mut account_capabilities: BTreeMap<&'static str, Value> = [
        (MAIL_CAPABILITY, mail_capability(registration.gmail)),
        (SUBMISSION_CAPABILITY, submission_capability()),
        (HULIHO_CAPABILITY, Value::Object(Map::new())),
    ]
    .into_iter()
    .collect();
    if quota {
        account_capabilities.insert(QUOTA_CAPABILITY, Value::Object(Map::new()));
    }
    // RFC 8621 sections 1.3.1 and 1.3.2 and RFC 9425 section 2.1: at
    // session level the three are empty objects; the bridge answers the
    // quota methods for every account, each account says whether it has
    // a quota.
    let capabilities: BTreeMap<&'static str, Value> = [
        (CORE_CAPABILITY, core_capability()),
        (MAIL_CAPABILITY, Value::Object(Map::new())),
        (QUOTA_CAPABILITY, Value::Object(Map::new())),
        (SUBMISSION_CAPABILITY, Value::Object(Map::new())),
        (HULIHO_CAPABILITY, Value::Object(Map::new())),
    ]
    .into_iter()
    .collect();
    let account = AccountObject {
        name: address.to_owned(),
        is_personal: true,
        is_read_only: false,
        account_capabilities,
    };
    let object = SessionObject {
        capabilities,
        accounts: [(key.as_str(), account)].into_iter().collect(),
        primary_accounts: [
            (MAIL_CAPABILITY, key.as_str()),
            (SUBMISSION_CAPABILITY, key.as_str()),
            (HULIHO_CAPABILITY, key.as_str()),
        ]
        .into_iter()
        .collect(),
        username: address,
        api_url: &urls.api,
        download_url: &urls.download,
        upload_url: &urls.upload,
        event_source_url: &urls.event_source,
        state: session_state(&registration.session_state, quota),
    };
    Ok(serde_json::to_vec(&object)?)
}

/// The state of the session object and of every Response (RFC 8620
/// section 3.4): the host's, derived from what it builds the object
/// from, marked while the server advertises QUOTA.
pub(crate) fn session_state(host: &str, quota: bool) -> String {
    if quota {
        format!("{host}{QUOTA_STATE_MARK}")
    } else {
        host.to_owned()
    }
}

/// The upload limits are the store's own (RFC 8620 section 2): what one
/// upload may weigh and how many the host's lanes carry at once.
fn core_capability() -> Value {
    json!({
        "maxSizeUpload": MAX_ATTACHMENTS_BYTES,
        "maxConcurrentUpload": MAX_CONCURRENT_UPLOADS,
        "maxSizeRequest": MAX_SIZE_REQUEST,
        "maxConcurrentRequests": MAX_CONCURRENT_REQUESTS,
        "maxCallsInRequest": MAX_CALLS_IN_REQUEST,
        "maxObjectsInGet": MAX_OBJECTS_IN_GET,
        "maxObjectsInSet": MAX_OBJECTS_IN_SET,
        "collationAlgorithms": [COLLATION],
    })
}

/// What an account says of its sends (RFC 8621 section 1.3.2): none is
/// held back and no SMTP extension is offered.
fn submission_capability() -> Value {
    json!({
        "maxDelayedSend": MAX_DELAYED_SEND,
        "submissionExtensions": {},
    })
}

fn mail_capability(gmail: bool) -> Value {
    // RFC 8621 section 1.3.1: null means no limit.
    let per_email = (!gmail).then_some(MAX_MAILBOXES_PER_EMAIL);
    json!({
        "maxMailboxesPerEmail": per_email,
        "maxMailboxDepth": null,
        "maxSizeMailboxName": MAX_MAILBOX_NAME_BYTES,
        "maxSizeAttachmentsPerEmail": MAX_ATTACHMENTS_BYTES,
        "emailQuerySortOptions": EMAIL_QUERY_SORT_OPTIONS,
        "mayCreateTopLevelMailbox": true,
    })
}
