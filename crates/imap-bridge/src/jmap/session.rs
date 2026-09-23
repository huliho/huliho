// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The session object of one bridge account (RFC 8620 section 2): the
//! core and mail capabilities with the bridge's limits, the vendor
//! capability and the state the host derived.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{Map, Value, json};

use super::{MAX_CALLS_IN_REQUEST, MAX_CONCURRENT_REQUESTS, MAX_OBJECTS_IN_GET, MAX_SIZE_REQUEST};
use crate::runtime::Registration;
use crate::store::StoreError;

/// The core capability (RFC 8620 section 2).
pub const CORE_CAPABILITY: &str = "urn:ietf:params:jmap:core";

/// The mail capability (RFC 8621 section 1.3.1).
pub const MAIL_CAPABILITY: &str = "urn:ietf:params:jmap:mail";

/// The vendor capability (RFC 8620 section 1.8) on the product domain;
/// it carries the Mailbox property `syncedEmails`.
pub const HULIHO_CAPABILITY: &str = "https://huliho.com/jmap";

/// A mailbox name on the wire is 255 octets at most on every server
/// the bridge meets.
const MAX_MAILBOX_NAME_BYTES: u32 = 255;

/// No upload route exists, so uploads read as zero.
const MAX_SIZE_UPLOAD: u32 = 0;
const MAX_CONCURRENT_UPLOAD: u32 = 0;

/// No `/set` method exists, so nothing can be set.
const MAX_OBJECTS_IN_SET: u32 = 0;

/// The one collation RFC 8620 section 2 requires of every server.
const COLLATION: &str = "i;unicode-casemap";

/// One folder per email outside Gmail, where a copy is a second email;
/// on Gmail a message sits under any number of labels.
const MAX_MAILBOXES_PER_EMAIL: u32 = 1;

/// No submission exists, so attachments read as zero.
const MAX_SIZE_ATTACHMENTS_PER_EMAIL: u32 = 0;

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

/// The session object for one account as JSON: read-only until a write
/// method exists, the vendor capability on the account, the state as
/// the host derived it from what the object is built from. Nothing here
/// touches the store, so the object never moves with the cache.
///
/// # Errors
///
/// Returns the store's encoding failure when the object cannot be
/// encoded.
pub fn session_object(
    registration: &Registration,
    address: &str,
    urls: &Urls,
) -> Result<Vec<u8>, StoreError> {
    let key = &registration.key;
    let account_capabilities: BTreeMap<&'static str, Value> = [
        (MAIL_CAPABILITY, mail_capability(registration.gmail)),
        (HULIHO_CAPABILITY, Value::Object(Map::new())),
    ]
    .into_iter()
    .collect();
    // RFC 8621 section 1.3.1: at session level the mail capability is an empty object.
    let capabilities: BTreeMap<&'static str, Value> = [
        (CORE_CAPABILITY, core_capability()),
        (MAIL_CAPABILITY, Value::Object(Map::new())),
        (HULIHO_CAPABILITY, Value::Object(Map::new())),
    ]
    .into_iter()
    .collect();
    let account = AccountObject {
        name: address.to_owned(),
        is_personal: true,
        is_read_only: true,
        account_capabilities,
    };
    let object = SessionObject {
        capabilities,
        accounts: [(key.as_str(), account)].into_iter().collect(),
        primary_accounts: [
            (MAIL_CAPABILITY, key.as_str()),
            (HULIHO_CAPABILITY, key.as_str()),
        ]
        .into_iter()
        .collect(),
        username: address,
        api_url: &urls.api,
        download_url: &urls.download,
        upload_url: &urls.upload,
        event_source_url: &urls.event_source,
        state: registration.session_state.clone(),
    };
    Ok(serde_json::to_vec(&object)?)
}

fn core_capability() -> Value {
    json!({
        "maxSizeUpload": MAX_SIZE_UPLOAD,
        "maxConcurrentUpload": MAX_CONCURRENT_UPLOAD,
        "maxSizeRequest": MAX_SIZE_REQUEST,
        "maxConcurrentRequests": MAX_CONCURRENT_REQUESTS,
        "maxCallsInRequest": MAX_CALLS_IN_REQUEST,
        "maxObjectsInGet": MAX_OBJECTS_IN_GET,
        "maxObjectsInSet": MAX_OBJECTS_IN_SET,
        "collationAlgorithms": [COLLATION],
    })
}

fn mail_capability(gmail: bool) -> Value {
    // RFC 8621 section 1.3.1: null means no limit.
    let per_email = (!gmail).then_some(MAX_MAILBOXES_PER_EMAIL);
    json!({
        "maxMailboxesPerEmail": per_email,
        "maxMailboxDepth": null,
        "maxSizeMailboxName": MAX_MAILBOX_NAME_BYTES,
        "maxSizeAttachmentsPerEmail": MAX_SIZE_ATTACHMENTS_PER_EMAIL,
        "emailQuerySortOptions": EMAIL_QUERY_SORT_OPTIONS,
        "mayCreateTopLevelMailbox": false,
    })
}
