// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The values of a create: what a header carries, an address, an id and
//! a Content-Type. A value that holds what cannot stand there reads as
//! nothing, which the caller names back.

use serde_json::Value;

use crate::message::fits_address;
use crate::store::Address;

/// The Content-Type parameters a part may declare.
const PARAMETERS: [&str; 3] = ["charset", "format", "delsp"];

/// The two dispositions a part may give (RFC 2183 section 2).
const DISPOSITIONS: [&str; 2] = ["attachment", "inline"];

/// The addresses one field may hold, as the sync keeps them.
const MAX_ADDRESSES: usize = 1024;

/// The ids one id field may hold, as the sync keeps them.
pub(super) const MAX_MESSAGE_IDS: usize = 256;

/// The bytes of one id: one line of a header holds it with room to spare
/// (RFC 5322 section 2.1.1).
pub(super) const MAX_ID_BYTES: usize = 512;

/// The bytes of a type or a subtype and of a parameter value (RFC 6838
/// section 4.2).
const MAX_TOKEN_BYTES: usize = 127;

/// The text of one `EmailBodyValue`; its two flags are false or absent
/// (RFC 8621 section 4.6). A NUL fits no message.
pub(super) fn body_value(entry: &Value) -> Option<&str> {
    let entry = entry.as_object()?;
    let known = ["value", "isEncodingProblem", "isTruncated"];
    if !entry.keys().all(|name| known.contains(&name.as_str())) {
        return None;
    }
    for flag in ["isEncodingProblem", "isTruncated"] {
        if entry
            .get(flag)
            .is_some_and(|value| *value != Value::Bool(false))
        {
            return None;
        }
    }
    entry
        .get("value")?
        .as_str()
        .filter(|text| !text.contains('\0'))
}

/// An address list; every address fits one header entry.
pub(super) fn addresses(value: &Value) -> Option<Vec<Address>> {
    let list = value
        .as_array()
        .filter(|list| list.len() <= MAX_ADDRESSES)?;
    list.iter().map(address).collect()
}

fn address(value: &Value) -> Option<Address> {
    let entry = value.as_object()?;
    if !entry.keys().all(|name| name == "name" || name == "email") {
        return None;
    }
    let email = entry
        .get("email")?
        .as_str()
        .filter(|email| fits_address(email))?;
    let name = match entry.get("name") {
        None | Some(Value::Null) => None,
        Some(name) => Some(header_text(name)?),
    };
    Some(Address {
        name,
        email: email.to_owned(),
    })
}

/// A string a header carries: no CR, LF or NUL, so it cannot end the
/// field or open another.
pub(super) fn header_text(value: &Value) -> Option<String> {
    let text = value.as_str()?;
    (!text.contains(['\r', '\n', '\0'])).then(|| text.to_owned())
}

/// An id field: the ids without their angle brackets (RFC 8621 section
/// 4.1.2.3).
pub(super) fn ids(value: &Value) -> Option<Vec<String>> {
    let list = value
        .as_array()
        .filter(|list| list.len() <= MAX_MESSAGE_IDS)?;
    list.iter().map(|id_value| id(id_value.as_str()?)).collect()
}

/// One id: printable ASCII without a space or an angle bracket (RFC 5322
/// section 3.6.4).
pub(super) fn id(text: &str) -> Option<String> {
    let fits = !text.is_empty()
        && text.len() <= MAX_ID_BYTES
        && text
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && byte != b'<' && byte != b'>');
    fits.then(|| text.to_owned())
}

pub(super) fn disposition(value: &Value) -> Option<String> {
    let kind = value.as_str()?;
    DISPOSITIONS.contains(&kind).then(|| kind.to_owned())
}

/// A token of a media type or a parameter value in lower case (RFC 2045
/// section 5.1), quotes and tspecials left out.
pub(super) fn token(text: &str) -> Option<String> {
    let fits = !text.is_empty()
        && text.len() <= MAX_TOKEN_BYTES
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte));
    fits.then(|| text.to_ascii_lowercase())
}

/// `type/subtype` in lower case.
pub(super) fn media_type(text: &str) -> Option<String> {
    let (kind, subtype) = text.split_once('/')?;
    Some(format!("{}/{}", token(kind)?, token(subtype)?))
}

/// The raw form of `header:Content-Type`: the type, then parameters of the
/// fixed set as tokens (RFC 8621 section 4.1.2.1).
pub(super) fn raw_content_type(raw: &str) -> Option<(String, Vec<(String, String)>)> {
    let mut pieces = raw.split(';').map(str::trim);
    let media_type = media_type(pieces.next()?)?;
    let parameters = pieces
        .map(|piece| {
            let (name, value) = piece.split_once('=')?;
            let name = token(name)?;
            PARAMETERS.contains(&name.as_str()).then_some(())?;
            Some((name, token(value)?))
        })
        .collect::<Option<Vec<_>>>()?;
    Some((media_type, parameters))
}
