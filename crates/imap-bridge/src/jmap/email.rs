// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! `Email/get` (RFC 8621 section 4.2): the metadata, the header
//! properties and the preview from the rows the header sync wrote; the
//! body properties and the `header:` forms from the message on the
//! server, fetched between two passes over the request.

use std::collections::HashSet;

use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::bodies::{Body, BodyAsk, Fetched};
use super::body::{self, MAX_BODIES_IN_GET};
use super::headers::{self, HeaderAsk};
use super::values::{self, ValueAsk};
use super::{Context, MethodError, arguments, get};
use crate::dates::utc_date;
use crate::store::{EmailRow, Personal};

/// The properties served from the store: the metadata of RFC 8621
/// section 4.1.1, the header properties of section 4.1.2.3,
/// `hasAttachment` and `preview` (section 4.1.4).
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

/// The properties served from the message on the server.
const BODY: [&str; 5] = [
    "bodyStructure",
    "bodyValues",
    "textBody",
    "htmlBody",
    "attachments",
];

/// The body properties served when no properties are named (RFC 8621
/// section 4.2), beside every stored one.
const DEFAULT_BODY: [&str; 4] = ["bodyValues", "textBody", "htmlBody", "attachments"];

const PREVIEW: &str = "preview";
const BODY_VALUES: &str = "bodyValues";

/// The arguments of `Email/get` (RFC 8620 section 5.1, RFC 8621
/// section 4.2).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EmailGetArguments {
    account_id: String,
    ids: Option<Vec<String>>,
    properties: Option<Vec<String>>,
    body_properties: Option<Vec<String>>,
    fetch_text_body_values: Option<bool>,
    #[serde(rename = "fetchHTMLBodyValues")]
    fetch_html_body_values: Option<bool>,
    fetch_all_body_values: Option<bool>,
    max_body_value_bytes: Option<u64>,
}

/// What a call asks for, by where it comes from.
struct Wanted {
    stored: Vec<&'static str>,
    body: Vec<&'static str>,
    headers: Vec<HeaderAsk>,
}

impl Wanted {
    /// Whether the message has to be read from the server.
    fn fetches(&self) -> bool {
        !self.body.is_empty() || !self.headers.is_empty()
    }
}

/// Each property named once; the default set when none are named. An
/// unknown property is `invalidArguments`.
fn wanted(named: Option<&[String]>) -> Result<Wanted, MethodError> {
    let Some(named) = named else {
        return Ok(Wanted {
            stored: PROPERTIES.to_vec(),
            body: DEFAULT_BODY.to_vec(),
            headers: Vec::new(),
        });
    };
    let mut wanted = Wanted {
        stored: vec!["id"],
        body: Vec::new(),
        headers: Vec::new(),
    };
    for name in named {
        if let Some(known) = PROPERTIES.iter().copied().find(|known| known == name) {
            if !wanted.stored.contains(&known) {
                wanted.stored.push(known);
            }
        } else if let Some(known) = BODY.iter().copied().find(|known| known == name) {
            if !wanted.body.contains(&known) {
                wanted.body.push(known);
            }
        } else if let Some(ask) = headers::ask(name)? {
            if !wanted.headers.iter().any(|found| found.key == ask.key) {
                wanted.headers.push(ask);
            }
        } else {
            return Err(MethodError::InvalidArguments(
                "an unknown property was asked for",
            ));
        }
    }
    Ok(wanted)
}

/// `Email/get`: the named rows, each cut to the wanted properties, with
/// the ids that name nothing. A preview the row lacks is served empty
/// and noted for the request to fetch; a body or a header form is noted
/// the same way on the first pass and served on the second.
pub(super) fn get(
    context: &Context<'_>,
    raw: &Map<String, Value>,
) -> Result<Map<String, Value>, MethodError> {
    let arguments: EmailGetArguments = arguments(raw)?;
    context.account(&arguments.account_id)?;
    let ids = get::ids(arguments.ids.as_deref())?;
    let wanted = wanted(arguments.properties.as_deref())?;
    let body_properties = body::body_properties(arguments.body_properties.as_deref())?;
    let fields = headers::names(&wanted.headers)?;
    let values = value_ask(&arguments, &wanted);
    let snapshot = context.store.emails(context.key, &ids)?;
    let bodies = context.bodies.borrow();
    if wanted.fetches() && bodies.is_none() && ids.len() > MAX_BODIES_IN_GET {
        return Err(MethodError::RequestTooLarge);
    }
    // A call with no row to read answers on the first pass: nothing to
    // fetch, so no second pass over the request.
    if wanted.fetches() && bodies.is_none() && !snapshot.rows.is_empty() {
        // The previews travel with the bodies, so the second pass has both.
        if wanted.stored.contains(&PREVIEW) {
            for row in &snapshot.rows {
                if lacks_preview(context, row) {
                    context.missing_previews.borrow_mut().push(row.id.clone());
                }
            }
        }
        *context.body_ask.borrow_mut() = Some(BodyAsk {
            ids: snapshot.rows.iter().map(|row| row.id.clone()).collect(),
            fields,
            values,
            structure: !wanted.body.is_empty(),
        });
        return Err(MethodError::ServerUnavailable);
    }
    let mut found: HashSet<&str> = snapshot.rows.iter().map(|row| row.id.as_str()).collect();
    let mut list = Vec::with_capacity(snapshot.rows.len());
    for row in &snapshot.rows {
        let body = if wanted.fetches() {
            match bodies.as_ref().and_then(|bodies| bodies.get(&row.id)) {
                Some(Fetched::Body(body)) => Some(body.as_ref()),
                Some(Fetched::Gone) => {
                    found.remove(row.id.as_str());
                    continue;
                }
                None => return Err(MethodError::ServerUnavailable),
            }
        } else {
            None
        };
        let (rendered, lacks_preview) = object(context, row, &wanted, (body, &body_properties))?;
        if lacks_preview && wanted.stored.contains(&PREVIEW) {
            context.missing_previews.borrow_mut().push(row.id.clone());
        }
        list.push(rendered);
    }
    Ok(get::answer(context, (snapshot.state, list), &ids, &found))
}

/// Whether the row names a text part and holds no preview yet; a blob
/// the host does not open counts as holding one, since the render
/// fails on it anyway.
fn lacks_preview(context: &Context<'_>, row: &EmailRow) -> bool {
    context
        .sealer
        .open(context.key, &row.id, &row.sealed)
        .and_then(|plain| serde_json::from_slice::<Personal>(&plain).ok())
        .is_some_and(|personal| personal.preview.is_none() && personal.preview_part.is_some())
}

/// Which values the call fetches: the flags where `bodyValues` is asked
/// and nothing otherwise, under the cap.
fn value_ask(arguments: &EmailGetArguments, wanted: &Wanted) -> ValueAsk {
    let asked = wanted.body.contains(&BODY_VALUES);
    ValueAsk {
        text: asked && arguments.fetch_text_body_values.unwrap_or(false),
        html: asked && arguments.fetch_html_body_values.unwrap_or(false),
        all: asked && arguments.fetch_all_body_values.unwrap_or(false),
        cap: values::cap(arguments.max_body_value_bytes.unwrap_or(0)),
    }
}

/// One Email object cut to the wanted properties plus whether its
/// preview is still to be fetched: the row names a text part and holds
/// no preview yet. A blob the host does not open or a date outside the
/// calendar is `serverFail`.
fn object(
    context: &Context<'_>,
    row: &EmailRow,
    wanted: &Wanted,
    (body, body_properties): (Option<&Body>, &[&str]),
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
    let mut rendered: Map<String, Value> = pairs
        .into_iter()
        .filter(|(name, _)| wanted.stored.contains(name))
        .map(|(name, value)| (name.to_owned(), value))
        .collect();
    if let Some(body) = body {
        rendered.extend(body_pairs(row, body, wanted, body_properties));
    }
    Ok((rendered.into(), lacks_preview))
}

/// The body properties and the header forms of one row from what the
/// server holds of its message.
fn body_pairs(
    row: &EmailRow,
    body: &Body,
    wanted: &Wanted,
    body_properties: &[&str],
) -> Vec<(String, Value)> {
    let email = row.id.as_str();
    // The one part of a message the server could not describe is the
    // message itself, so its blob is the message's.
    let whole = body.tree.is_none();
    let tree = body
        .tree
        .clone()
        .unwrap_or_else(|| body::too_complex(row.size));
    let lists = body::lists(&tree);
    let render = |node: &body::Node<'_>| {
        let mut part = body::render_node(node, email, body_properties);
        if whole && let Some(blob) = part.get_mut("blobId") {
            *blob = json!(email);
        }
        part
    };
    let nodes = |list: &[body::Node<'_>]| Value::Array(list.iter().map(render).collect());
    let mut pairs = Vec::new();
    for name in &wanted.body {
        let value = match *name {
            "bodyStructure" if whole => nodes(&lists.attachments)
                .as_array_mut()
                .and_then(Vec::pop)
                .unwrap_or(Value::Null),
            "bodyStructure" => body::render_tree(&tree, email, body_properties),
            "textBody" => nodes(&lists.text),
            "htmlBody" => nodes(&lists.html),
            "attachments" => nodes(&lists.attachments),
            _ => body
                .values
                .iter()
                .map(|(part_id, value)| (part_id.clone(), value.render()))
                .collect::<Map<_, _>>()
                .into(),
        };
        pairs.push(((*name).to_owned(), value));
    }
    for ask in &wanted.headers {
        pairs.push((ask.key.clone(), headers::value(ask, &body.header)));
    }
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn properties_default_to_the_rfc_set_and_an_unknown_one_is_refused_rfc8621_4_2() {
        let default = wanted(None).unwrap();
        assert_eq!(default.stored.len(), PROPERTIES.len());
        assert_eq!(default.body, DEFAULT_BODY);
        assert!(default.headers.is_empty());
        let picked = wanted(Some(&named(&[
            "subject",
            "preview",
            "subject",
            "htmlBody",
            "header:X-A:all",
            "header:X-A:all",
        ])))
        .unwrap();
        assert_eq!(picked.stored, ["id", "subject", "preview"]);
        assert_eq!(picked.body, ["htmlBody"]);
        assert_eq!(picked.headers.len(), 1);
        assert!(picked.fetches());
        assert!(!wanted(Some(&named(&["subject"]))).unwrap().fetches());
        for unknown in ["value", "header:From:asText", "Subject"] {
            assert!(
                matches!(
                    wanted(Some(&named(&[unknown]))),
                    Err(MethodError::InvalidArguments(_))
                ),
                "{unknown}"
            );
        }
    }

    #[test]
    fn values_are_fetched_only_where_body_values_is_asked() {
        let arguments = EmailGetArguments {
            account_id: "a1".to_owned(),
            ids: None,
            properties: None,
            body_properties: None,
            fetch_text_body_values: Some(true),
            fetch_html_body_values: Some(true),
            fetch_all_body_values: None,
            max_body_value_bytes: Some(4096),
        };
        let with = value_ask(&arguments, &wanted(None).unwrap());
        assert!(with.text && with.html && !with.all);
        assert_eq!(with.cap, 4096);
        let without = value_ask(&arguments, &wanted(Some(&named(&["htmlBody"]))).unwrap());
        assert!(!without.wants_any());
    }
}
