// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! `Email/get` (RFC 8621 section 4.2) over the rows the header sync
//! wrote: the metadata, the header properties and the preview. Bodies
//! are not served yet, so a body property is an unknown one.

use std::collections::HashSet;

use serde_json::{Map, Value, json};

use super::get::{self, GetArguments};
use super::{Context, MethodError, arguments};
use crate::dates::utc_date;
use crate::store::{EmailRow, Personal};

/// The properties served: the metadata of RFC 8621 section 4.1.1, the
/// header properties of section 4.1.2.3, `hasAttachment` and
/// `preview` (section 4.1.4).
const PROPERTIES: [&str; 20] = [
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
    "preview",
];

const PREVIEW: &str = "preview";

/// `Email/get`: the named rows, each cut to the wanted properties, with
/// the ids that name nothing. A preview the row lacks is served empty
/// and noted for the request to fetch.
pub(super) fn get(
    context: &Context<'_>,
    raw: &Map<String, Value>,
) -> Result<Map<String, Value>, MethodError> {
    let arguments: GetArguments = arguments(raw)?;
    context.account(&arguments.account_id)?;
    let ids = get::ids(arguments.ids.as_deref())?;
    let wanted = get::properties(&PROPERTIES, arguments.properties.as_deref())?;
    let snapshot = context.store.emails(context.key, &ids)?;
    let found: HashSet<&str> = snapshot.rows.iter().map(|row| row.id.as_str()).collect();
    let mut list = Vec::with_capacity(snapshot.rows.len());
    for row in &snapshot.rows {
        let (rendered, lacks_preview) = object(context, row, &wanted)?;
        if lacks_preview && wanted.contains(&PREVIEW) {
            context.missing_previews.borrow_mut().push(row.id.clone());
        }
        list.push(rendered);
    }
    Ok(get::answer(context, (snapshot.state, list), &ids, &found))
}

/// One Email object cut to the wanted properties, and whether its
/// preview is still to be fetched: the row names a text part and holds
/// no preview yet. A blob the host does not open or a date outside the
/// calendar is `serverFail`.
fn object(
    context: &Context<'_>,
    row: &EmailRow,
    wanted: &[&str],
) -> Result<(Value, bool), MethodError> {
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
    let lacks_preview = personal.preview.is_none() && personal.preview_part.is_some();
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
        (PREVIEW, json!(personal.preview.unwrap_or_default())),
    ];
    let rendered = pairs
        .into_iter()
        .filter(|(name, _)| wanted.contains(name))
        .map(|(name, value)| (name.to_owned(), value))
        .collect::<Map<_, _>>()
        .into();
    Ok((rendered, lacks_preview))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn properties(named: Option<&[String]>) -> Result<Vec<&'static str>, MethodError> {
        get::properties(&PROPERTIES, named)
    }

    #[test]
    fn properties_default_to_the_served_ones_and_a_body_property_is_unknown() {
        assert_eq!(properties(None).unwrap().len(), PROPERTIES.len());
        let named = [
            "subject".to_owned(),
            "preview".to_owned(),
            "subject".to_owned(),
        ];
        assert_eq!(
            properties(Some(&named)).unwrap(),
            ["id", "subject", "preview"]
        );
        for body in ["bodyValues", "textBody", "htmlBody", "header:Subject"] {
            assert!(matches!(
                properties(Some(&[body.to_owned()])),
                Err(MethodError::InvalidArguments(_))
            ));
        }
    }
}
