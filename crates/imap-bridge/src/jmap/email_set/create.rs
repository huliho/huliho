// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! One create of `Email/set` read into a draft (RFC 8621 section 4.6): a
//! fixed set of properties, the tree under `bodyStructure`, every other
//! form refused under `invalidProperties` with the paths it names. A
//! header value, an address or a part name that holds CR, LF or NUL and a
//! body value that holds a NUL are refused here, before anything is built.

mod values;

use std::collections::HashMap;

use serde_json::{Map, Value};

use self::values::{
    addresses, body_value, disposition, header_text, id, ids, media_type, raw_content_type, token,
};
use super::super::set::SetError;
use crate::message::{Content, Draft, Part};
use crate::store::Personal;
use crate::sync::mapping::canonical_keyword;

/// The properties a create may give. `messageId`, `sentAt`, `receivedAt`,
/// `size`, `blobId`, `threadId` and `headers` are the server's and the
/// body lists stand aside for `bodyStructure`.
const PROPERTIES: [&str; 13] = [
    "mailboxIds",
    "keywords",
    "from",
    "sender",
    "replyTo",
    "to",
    "cc",
    "bcc",
    "subject",
    "inReplyTo",
    "references",
    "bodyStructure",
    "bodyValues",
];

/// The properties a part may give. `header:Content-Type` in its raw form
/// is how a part declares `format` and `delsp`, as `type` has no room for
/// them (RFC 8621 section 4.1.4).
const PART_PROPERTIES: [&str; 10] = [
    "partId",
    "blobId",
    "type",
    "header:Content-Type",
    "charset",
    "size",
    "name",
    "disposition",
    "cid",
    "subParts",
];

/// The keywords a create may store: the four system flags, since
/// PERMANENTFLAGS is unknown before a SELECT.
pub(in crate::jmap) const STORABLE_KEYWORDS: [&str; 4] =
    ["$seen", "$flagged", "$answered", "$draft"];

const CHARSET: &str = "charset";

/// The parts of one tree and how deep it nests, far above what a
/// composer builds.
const MAX_PARTS: usize = 256;
const MAX_DEPTH: usize = 8;

/// The bytes of a blob id: an email id and a part number, bounded at the
/// 255 octets RFC 8620 section 1.2 allows an Id.
const MAX_BLOB_ID_BYTES: usize = 255;

const MULTIPART: &str = "multipart/";
const TEXT: &str = "text/";
/// What a part is without a type (RFC 8621 section 4.1.4).
const IMPLICIT_TYPE: &str = "text/plain";
const IMPLICIT_MULTIPART: &str = "multipart/mixed";

/// Reads one create; a refusal names every property it refuses.
pub(in crate::jmap) fn read(object: &Value) -> Result<Draft, SetError> {
    let Value::Object(object) = object else {
        return Err(SetError::InvalidProperties(Vec::new()));
    };
    let mut reading = Reading {
        object,
        values: HashMap::new(),
        invalid: object
            .keys()
            .filter(|name| !PROPERTIES.contains(&name.as_str()))
            .cloned()
            .collect(),
        parts: 0,
    };
    let mailbox = reading.mailbox();
    let keywords = reading.keywords();
    let personal = Personal {
        from: reading.field("from", addresses),
        sender: reading.field("sender", addresses),
        reply_to: reading.field("replyTo", addresses),
        to: reading.field("to", addresses),
        cc: reading.field("cc", addresses),
        bcc: reading.field("bcc", addresses),
        subject: reading.field("subject", header_text),
        in_reply_to: reading.field("inReplyTo", ids),
        references: reading.field("references", ids),
        ..Personal::default()
    };
    reading.body_values();
    let body = reading.body();
    match (mailbox, keywords, body, reading.invalid.is_empty()) {
        (Some(mailbox), Some(keywords), Some(body), true) => Ok(Draft {
            mailbox,
            keywords,
            personal,
            body,
        }),
        _ => Err(SetError::InvalidProperties(reading.invalid)),
    }
}

/// One object while it is read.
struct Reading<'a> {
    object: &'a Map<String, Value>,
    values: HashMap<&'a str, &'a str>,
    invalid: Vec<String>,
    parts: usize,
}

impl<'a> Reading<'a> {
    /// One optional property; a null reads as absent (RFC 8621 section
    /// 4.1.2).
    fn field<T>(&mut self, name: &str, read: impl FnOnce(&'a Value) -> Option<T>) -> Option<T> {
        let value = self.object.get(name).filter(|value| !value.is_null())?;
        let found = read(value);
        if found.is_none() {
            self.invalid.push(name.to_owned());
        }
        found
    }

    /// The one mailbox `mailboxIds` names.
    fn mailbox(&mut self) -> Option<String> {
        let one = self.object.get("mailboxIds").and_then(Value::as_object);
        let found = one.filter(|ids| ids.len() == 1).and_then(|ids| {
            let (id, value) = ids.iter().next()?;
            (*value == Value::Bool(true) && !id.is_empty()).then(|| id.clone())
        });
        if found.is_none() {
            self.invalid.push("mailboxIds".to_owned());
        }
        found
    }

    /// The keywords, each a storable system flag's; none without the
    /// property.
    fn keywords(&mut self) -> Option<Vec<String>> {
        let Some(given) = self.object.get("keywords").filter(|value| !value.is_null()) else {
            return Some(Vec::new());
        };
        let read = given.as_object().and_then(|keywords| {
            keywords
                .iter()
                .map(|(keyword, value)| {
                    let keyword = canonical_keyword(keyword)?;
                    let storable = STORABLE_KEYWORDS.contains(&keyword.as_str());
                    (storable && *value == Value::Bool(true)).then_some(keyword)
                })
                .collect::<Option<Vec<String>>>()
        });
        if read.is_none() {
            self.invalid.push("keywords".to_owned());
        }
        read
    }

    /// The text of every entry of `bodyValues`.
    fn body_values(&mut self) {
        let Some(given) = self
            .object
            .get("bodyValues")
            .filter(|value| !value.is_null())
        else {
            return;
        };
        let Some(entries) = given.as_object() else {
            self.invalid.push("bodyValues".to_owned());
            return;
        };
        for (id, entry) in entries {
            let text = body_value(entry);
            if text.is_none() {
                self.invalid.push(format!("bodyValues/{id}"));
            }
            // A part that names a refused value is not named back as well.
            self.values.insert(id, text.unwrap_or_default());
        }
    }

    fn body(&mut self) -> Option<Part> {
        let given = self.object.get("bodyStructure");
        let Some(given) = given.filter(|value| !value.is_null()) else {
            self.invalid.push("bodyStructure".to_owned());
            return None;
        };
        self.part(given, "bodyStructure", 0)
    }

    /// One part with what it holds; every refusal goes under its path.
    fn part(&mut self, value: &'a Value, path: &str, depth: usize) -> Option<Part> {
        self.parts += 1;
        let Some(part) = value
            .as_object()
            .filter(|_| depth < MAX_DEPTH && self.parts <= MAX_PARTS)
        else {
            self.invalid.push(path.to_owned());
            return None;
        };
        let before = self.invalid.len();
        for name in part
            .keys()
            .filter(|name| !PART_PROPERTIES.contains(&name.as_str()))
        {
            self.invalid.push(format!("{path}/{name}"));
        }
        let (media_type, parameters) = self.content_type(part, path);
        let content = self.content(part, path, depth);
        let name = self.part_field((part, path), "name", header_text);
        let disposition = self.part_field((part, path), "disposition", disposition);
        let cid = self.part_field((part, path), "cid", |value| id(value.as_str()?));
        let content = content?;
        let media_type = match (media_type, &content) {
            (Some(media_type), _) => media_type,
            (None, Content::Parts(_)) => IMPLICIT_MULTIPART.to_owned(),
            (None, _) => IMPLICIT_TYPE.to_owned(),
        };
        self.fits((part, path), (&media_type, &parameters), &content);
        (self.invalid.len() == before).then_some(Part {
            media_type,
            parameters,
            name,
            disposition,
            cid,
            content,
        })
    }

    /// One optional property of a part.
    fn part_field<T>(
        &mut self,
        (part, path): (&'a Map<String, Value>, &str),
        name: &str,
        read: impl FnOnce(&'a Value) -> Option<T>,
    ) -> Option<T> {
        let value = part.get(name).filter(|value| !value.is_null())?;
        let found = read(value);
        if found.is_none() {
            self.invalid.push(format!("{path}/{name}"));
        }
        found
    }

    /// The type and the parameters of a part: `type`, or the raw
    /// `header:Content-Type`, never both (RFC 8621 section 4.6); `charset`
    /// beside them where the part goes by a blob.
    fn content_type(
        &mut self,
        part: &'a Map<String, Value>,
        path: &str,
    ) -> (Option<String>, Vec<(String, String)>) {
        let given = part.get("type").filter(|value| !value.is_null());
        let raw = part
            .get("header:Content-Type")
            .filter(|value| !value.is_null());
        if given.is_some() && raw.is_some() {
            self.invalid.push(format!("{path}/type"));
            return (None, Vec::new());
        }
        if let Some(raw) = raw {
            let read = raw.as_str().and_then(raw_content_type);
            if read.is_none() {
                self.invalid.push(format!("{path}/header:Content-Type"));
            }
            let (media_type, parameters) = read.unzip();
            return (media_type, parameters.unwrap_or_default());
        }
        let media_type = self.part_field((part, path), "type", |value| media_type(value.as_str()?));
        let charset = self.part_field((part, path), CHARSET, |value| token(value.as_str()?));
        let parameters = charset.map(|charset| (CHARSET.to_owned(), charset));
        (media_type, parameters.into_iter().collect())
    }

    /// What the part holds: one of `partId`, `blobId` and `subParts`.
    fn content(
        &mut self,
        part: &'a Map<String, Value>,
        path: &str,
        depth: usize,
    ) -> Option<Content> {
        let given = |name: &str| part.get(name).filter(|value| !value.is_null());
        match (given("partId"), given("blobId"), given("subParts")) {
            (Some(id), None, None) => {
                // Taken once: one value under every leaf would copy it per leaf.
                let text = id.as_str().and_then(|id| self.values.remove(id));
                if text.is_none() {
                    self.invalid.push(format!("{path}/partId"));
                }
                Some(Content::Value(text?.to_owned()))
            }
            (None, Some(id), None) => {
                let blob = id
                    .as_str()
                    .filter(|id| !id.is_empty() && id.len() <= MAX_BLOB_ID_BYTES);
                if blob.is_none() {
                    self.invalid.push(format!("{path}/blobId"));
                }
                Some(Content::Blob(blob?.to_owned()))
            }
            (None, None, Some(parts)) => {
                let Some(parts) = parts.as_array().filter(|parts| !parts.is_empty()) else {
                    self.invalid.push(format!("{path}/subParts"));
                    return None;
                };
                let read: Vec<Option<Part>> = parts
                    .iter()
                    .enumerate()
                    .map(|(index, part)| {
                        self.part(part, &format!("{path}/subParts/{index}"), depth + 1)
                    })
                    .collect();
                read.into_iter()
                    .collect::<Option<Vec<Part>>>()
                    .map(Content::Parts)
            }
            (Some(_), Some(_), _) => {
                self.invalid.push(format!("{path}/blobId"));
                None
            }
            _ => {
                self.invalid.push(format!("{path}/partId"));
                None
            }
        }
    }

    /// What a part may give for what it holds: text by a `partId` under a
    /// text type with no `charset` or `size`, a tree under a multipart
    /// type with nothing a leaf gives, a blob under any other type (RFC
    /// 8621 section 4.6).
    fn fits(
        &mut self,
        (part, path): (&'a Map<String, Value>, &str),
        (media_type, parameters): (&str, &[(String, String)]),
        content: &Content,
    ) {
        let multipart = media_type.starts_with(MULTIPART);
        let charset = parameters.iter().any(|(name, _)| name == CHARSET);
        let named = |name: &str| part.get(name).is_some_and(|value| !value.is_null());
        let mut refuse = |refused: bool, name: &str| {
            if refused {
                self.invalid.push(format!("{path}/{name}"));
            }
        };
        match content {
            Content::Value(_) => {
                refuse(!media_type.starts_with(TEXT), "type");
                refuse(charset, CHARSET);
                refuse(named("size"), "size");
            }
            Content::Blob(_) => refuse(multipart, "type"),
            Content::Parts(_) => {
                refuse(!multipart, "type");
                refuse(charset, CHARSET);
                for name in ["size", "name", "disposition", "cid"] {
                    refuse(named(name), name);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
