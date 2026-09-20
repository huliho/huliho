// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! `Email/get` (RFC 8621 section 4.2) over the rows the header sync
//! wrote: the metadata and the header properties. Bodies are not served
//! yet, so a body property is an unknown one.

use std::collections::HashSet;

use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::{Context, MAX_OBJECTS_IN_GET, MethodError, arguments};
use crate::dates::utc_date;
use crate::store::{EmailRow, Personal};

/// The properties served: the metadata of RFC 8621 section 4.1.1, the
/// header properties of section 4.1.2.3 and `hasAttachment`.
const PROPERTIES: [&str; 19] = [
    "id",
    "blobId",
    "threadId",
    "mailboxIds",
    "keywords",
    "size",
    "receivedAt",
    "messageId",
    "inReplyTo",
    "references",
    "sender",
    "from",
    "to",
    "cc",
    "bcc",
    "replyTo",
    "subject",
    "sentAt",
    "hasAttachment",
];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetArguments {
    account_id: String,
    ids: Option<Vec<String>>,
    properties: Option<Vec<String>>,
}

/// `Email/get`: the named rows, each cut to the wanted properties, with
/// the ids that name nothing. Every email at once is more than one
/// answer holds, so null `ids` is `requestTooLarge` (RFC 8620 section
/// 5.1).
pub(super) fn get(
    context: &Context<'_>,
    raw: &Map<String, Value>,
) -> Result<Map<String, Value>, MethodError> {
    let arguments: GetArguments = arguments(raw)?;
    context.account(&arguments.account_id)?;
    let ids = arguments.ids.ok_or(MethodError::RequestTooLarge)?;
    if ids.len() > MAX_OBJECTS_IN_GET {
        return Err(MethodError::RequestTooLarge);
    }
    let wanted = properties(arguments.properties.as_deref())?;
    // RFC 8620 section 5.1: an id sent more than once is answered once.
    let mut seen = HashSet::new();
    let ids: Vec<&str> = ids
        .iter()
        .map(String::as_str)
        .filter(|id| seen.insert(*id))
        .collect();
    let snapshot = context.store.emails(context.key, &ids)?;
    let found: HashSet<&str> = snapshot.rows.iter().map(|row| row.id.as_str()).collect();
    let not_found: Vec<&str> = ids
        .iter()
        .copied()
        .filter(|id| !found.contains(id))
        .collect();
    let list = snapshot
        .rows
        .iter()
        .map(|row| object(context, row, &wanted))
        .collect::<Result<Vec<_>, _>>()?;
    let mut answer = Map::new();
    answer.insert("accountId".to_owned(), Value::from(context.key.as_str()));
    answer.insert("state".to_owned(), Value::from(snapshot.state.to_string()));
    answer.insert("list".to_owned(), Value::Array(list));
    answer.insert("notFound".to_owned(), json!(not_found));
    Ok(answer)
}

/// Every served property when none is named, the named ones plus `id`
/// otherwise.
fn properties(named: Option<&[String]>) -> Result<Vec<&'static str>, MethodError> {
    let Some(named) = named else {
        return Ok(PROPERTIES.to_vec());
    };
    let mut wanted = vec!["id"];
    for name in named {
        let property = PROPERTIES
            .iter()
            .copied()
            .find(|known| *known == name)
            .ok_or(MethodError::InvalidArguments(
                "an unknown property was asked for",
            ))?;
        if !wanted.contains(&property) {
            wanted.push(property);
        }
    }
    Ok(wanted)
}

/// One Email object cut to the wanted properties. A blob the host does
/// not open or a date outside the calendar is `serverFail`.
fn object(context: &Context<'_>, row: &EmailRow, wanted: &[&str]) -> Result<Value, MethodError> {
    let plain = context
        .sealer
        .open(context.key, &row.id, &row.sealed)
        .ok_or(MethodError::ServerFail)?;
    let personal: Personal = serde_json::from_slice(&plain).map_err(|_| MethodError::ServerFail)?;
    let received_at = utc_date(row.received_at).ok_or(MethodError::ServerFail)?;
    let mailbox_ids: Map<String, Value> = row
        .mailbox_ids
        .iter()
        .map(|id| (id.to_string(), Value::Bool(true)))
        .collect();
    let pairs = [
        ("id", json!(row.id)),
        ("blobId", json!(row.id)),
        ("threadId", json!(row.thread_id)),
        ("mailboxIds", Value::Object(mailbox_ids)),
        ("keywords", json!(row.keywords)),
        ("size", json!(row.size)),
        ("receivedAt", json!(received_at)),
        ("messageId", json!(personal.message_id)),
        ("inReplyTo", json!(personal.in_reply_to)),
        ("references", json!(personal.references)),
        ("sender", json!(personal.sender)),
        ("from", json!(personal.from)),
        ("to", json!(personal.to)),
        ("cc", json!(personal.cc)),
        ("bcc", json!(personal.bcc)),
        ("replyTo", json!(personal.reply_to)),
        ("subject", json!(personal.subject)),
        ("sentAt", json!(row.sent_at.and_then(utc_date))),
        ("hasAttachment", json!(row.has_attachment)),
    ];
    Ok(pairs
        .into_iter()
        .filter(|(name, _)| wanted.contains(name))
        .map(|(name, value)| (name.to_owned(), value))
        .collect::<Map<_, _>>()
        .into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn properties_default_to_the_served_ones_and_a_body_property_is_unknown() {
        assert_eq!(properties(None).unwrap().len(), PROPERTIES.len());
        let named = ["subject".to_owned(), "subject".to_owned()];
        assert_eq!(properties(Some(&named)).unwrap(), ["id", "subject"]);
        for body in ["bodyValues", "textBody", "preview", "header:Subject"] {
            assert!(matches!(
                properties(Some(&[body.to_owned()])),
                Err(MethodError::InvalidArguments(_))
            ));
        }
    }
}
