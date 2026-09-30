// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The `header:` properties of an Email (RFC 8621 section 4.1.3): which
//! field, in which form, one instance or all. The Raw and the Text form
//! are served; the fields come from a partial fetch of the message.

use mail_parser::MessageParser;
use mail_parser::parsers::MessageStream;
use serde_json::Value;
use unicode_normalization::UnicodeNormalization as _;

use super::MethodError;
use crate::session::sendable_field;

/// The header fields one `Email/get` may name; each goes on the FETCH
/// command line.
pub const MAX_HEADER_FIELDS: usize = 32;

/// The longest field name; RFC 5322 section 2.1.1 bounds a whole line
/// at 998 bytes.
const MAX_HEADER_NAME_BYTES: usize = 255;

const PREFIX: &str = "header:";
const RAW_FORM: &str = "asRaw";
const TEXT_FORM: &str = "asText";
const ALL_SUFFIX: &str = "all";

/// The fields of RFC 5322 and RFC 2369 whose value is not text, so the
/// Text form is refused for them (RFC 8621 section 4.1.2.2).
const STRUCTURED_FIELDS: [&str; 25] = [
    "Date",
    "From",
    "Sender",
    "Reply-To",
    "To",
    "Cc",
    "Bcc",
    "Message-ID",
    "In-Reply-To",
    "References",
    "Resent-Date",
    "Resent-From",
    "Resent-Sender",
    "Resent-To",
    "Resent-Cc",
    "Resent-Bcc",
    "Resent-Message-ID",
    "Return-Path",
    "Received",
    "List-Help",
    "List-Unsubscribe",
    "List-Subscribe",
    "List-Post",
    "List-Owner",
    "List-Archive",
];

/// How a field's value is answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Form {
    /// The octets after the colon up to the line's end (section 4.1.2.1).
    Raw,
    /// Unfolded, RFC 2047 words decoded, trimmed (section 4.1.2.2).
    Text,
}

/// One `header:` property as asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct HeaderAsk {
    /// The property as written, which the answer echoes.
    pub key: String,
    pub name: String,
    pub form: Form,
    /// Every instance as a list, else the last one.
    pub all: bool,
}

/// The ask behind a property; `None` for a property that is no header.
pub(super) fn ask(property: &str) -> Result<Option<HeaderAsk>, MethodError> {
    let Some(rest) = property.strip_prefix(PREFIX) else {
        return Ok(None);
    };
    let mut pieces = rest.split(':');
    let name = pieces.next().unwrap_or_default();
    if name.is_empty() || name.len() > MAX_HEADER_NAME_BYTES || !sendable_field(name) {
        return Err(MethodError::InvalidArguments(
            "a header property names a field that cannot be fetched",
        ));
    }
    let mut form = Form::Raw;
    let mut all = false;
    let mut next = pieces.next();
    if let Some(word) = next.filter(|word| word.starts_with("as")) {
        form = match word {
            RAW_FORM => Form::Raw,
            TEXT_FORM if !is_structured(name) => Form::Text,
            _ => {
                return Err(MethodError::InvalidArguments(
                    "a header form is not served for that field",
                ));
            }
        };
        next = pieces.next();
    }
    if next == Some(ALL_SUFFIX) {
        all = true;
        next = pieces.next();
    }
    if next.is_some() {
        return Err(MethodError::InvalidArguments(
            "a header property carries an unknown suffix",
        ));
    }
    Ok(Some(HeaderAsk {
        key: property.to_owned(),
        name: name.to_owned(),
        form,
        all,
    }))
}

fn is_structured(name: &str) -> bool {
    STRUCTURED_FIELDS
        .iter()
        .any(|field| field.eq_ignore_ascii_case(name))
}

/// The distinct field names of the asks, as the FETCH command names
/// them; more than `MAX_HEADER_FIELDS` is `invalidArguments`.
pub(super) fn names(asks: &[HeaderAsk]) -> Result<Vec<String>, MethodError> {
    let mut names: Vec<String> = Vec::new();
    for ask in asks {
        if !names
            .iter()
            .any(|name| name.eq_ignore_ascii_case(&ask.name))
        {
            names.push(ask.name.clone());
        }
    }
    if names.len() > MAX_HEADER_FIELDS {
        return Err(MethodError::InvalidArguments(
            "a call names more header fields than one fetch carries",
        ));
    }
    Ok(names)
}

/// The value of one ask from the fetched header fields: every instance
/// in the order written for `:all`, else the last one or null.
pub(super) fn value(ask: &HeaderAsk, header: &[u8]) -> Value {
    let instances: Vec<Value> = MessageParser::default()
        .parse_headers(header)
        .into_iter()
        .flat_map(|message| {
            message
                .headers()
                .iter()
                .filter(|field| field.name().eq_ignore_ascii_case(&ask.name))
                .map(|field| {
                    let start = usize::try_from(field.offset_start()).unwrap_or(usize::MAX);
                    let end = usize::try_from(field.offset_end()).unwrap_or(usize::MAX);
                    Value::String(render(
                        ask.form,
                        &header[start.min(header.len())..end.min(header.len())],
                    ))
                })
                .collect::<Vec<_>>()
        })
        .collect();
    if ask.all {
        Value::Array(instances)
    } else {
        instances.into_iter().last().unwrap_or(Value::Null)
    }
}

/// One instance's value in the asked form; `bytes` run from the colon
/// through the line's end. The Raw form drops a NUL octet and the Text
/// form every control character but a tab (RFC 8621 section 4.1.2).
fn render(form: Form, bytes: &[u8]) -> String {
    match form {
        Form::Raw => {
            let end = bytes
                .strip_suffix(b"\r\n")
                .or_else(|| bytes.strip_suffix(b"\n"))
                .unwrap_or(bytes);
            String::from_utf8_lossy(end).replace('\0', "")
        }
        Form::Text => {
            let mut line = bytes.to_vec();
            line.extend_from_slice(b"\r\n");
            MessageStream::new(&line)
                .parse_unstructured()
                .into_text()
                .map(|text| {
                    text.chars()
                        .filter(|c| *c == '\t' || !c.is_control())
                        .nfc()
                        .collect()
                })
                .unwrap_or_default()
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const HEADER: &[u8] = b"Authentication-Results: mx.example;\r\n dkim=pass\r\n\
        Subject: =?UTF-8?Q?Caf=C3=A9?= om drie\r\n\
        authentication-results: other.example; spf=fail\r\n\r\n";

    fn asked(property: &str) -> HeaderAsk {
        ask(property).unwrap().unwrap()
    }

    #[test]
    fn a_property_reads_its_name_form_and_suffix_rfc8621_4_1_3() {
        let plain = asked("header:Subject");
        assert_eq!((plain.form, plain.all), (Form::Raw, false));
        let full = asked("header:Authentication-Results:asRaw:all");
        assert_eq!((full.form, full.all), (Form::Raw, true));
        assert_eq!(full.name, "Authentication-Results");
        assert_eq!(full.key, "header:Authentication-Results:asRaw:all");
        let text = asked("header:Subject:asText");
        assert_eq!(text.form, Form::Text);
        assert!(asked("header:X-Spam:all").all);
        assert_eq!(ask("subject").unwrap(), None);
    }

    #[test]
    fn a_form_a_field_does_not_take_and_an_unknown_suffix_are_refused_rfc8621_4_2() {
        for property in [
            "header:From:asText",
            "header:list-post:asText",
            "header:Subject:asAddresses",
            "header:Subject:all:asRaw",
            "header:Subject:asRaw:all:more",
            "header:",
            "header:a b",
            "header:a(b",
            "header:caf\u{e9}",
        ] {
            assert!(
                matches!(ask(property), Err(MethodError::InvalidArguments(_))),
                "{property}"
            );
        }
        let long = format!("header:{}", "x".repeat(MAX_HEADER_NAME_BYTES + 1));
        assert!(ask(&long).is_err());
        assert!(ask("header:List-Id:asText").is_ok());
    }

    #[test]
    fn the_names_are_distinct_without_regard_to_case_and_bounded() {
        let asks: Vec<HeaderAsk> = ["header:X-A", "header:x-a:all", "header:X-B"]
            .iter()
            .map(|property| asked(property))
            .collect();
        assert_eq!(names(&asks).unwrap(), ["X-A", "X-B"]);
        let many: Vec<HeaderAsk> = (0..=MAX_HEADER_FIELDS)
            .map(|n| asked(&format!("header:X-{n}")))
            .collect();
        assert!(names(&many).is_err());
    }

    #[test]
    fn every_instance_comes_in_order_the_last_one_alone_and_null_for_none() {
        let all = value(&asked("header:Authentication-Results:all"), HEADER);
        assert_eq!(
            all,
            json!([" mx.example;\r\n dkim=pass", " other.example; spf=fail"])
        );
        let last = value(&asked("header:AUTHENTICATION-RESULTS"), HEADER);
        assert_eq!(last, json!(" other.example; spf=fail"));
        assert_eq!(value(&asked("header:X-Missing"), HEADER), Value::Null);
        assert_eq!(value(&asked("header:X-Missing:all"), HEADER), json!([]));
    }

    #[test]
    fn the_text_form_unfolds_decodes_trims_normalizes_and_drops_controls_rfc8621_4_1_2() {
        assert_eq!(
            value(&asked("header:Subject:asText"), HEADER),
            json!("Caf\u{e9} om drie")
        );
        let decomposed = "Subject: Cafe\u{301}\r\n\r\n";
        assert_eq!(
            value(&asked("header:Subject:asText"), decomposed.as_bytes()),
            json!("Caf\u{e9}")
        );
        assert_eq!(
            value(&asked("header:Subject"), decomposed.as_bytes()),
            json!(" Cafe\u{301}")
        );
        let controls = b"Subject: =?UTF-8?Q?a=00b=07c=09d?=\r\n\r\n";
        assert_eq!(
            value(&asked("header:Subject:asText"), controls),
            json!("abc\td")
        );
        // A control octet between a letter and its mark goes before the
        // pair composes.
        let split = b"Subject: =?UTF-8?Q?Cafe=07=CC=81?=\r\n\r\n";
        assert_eq!(
            value(&asked("header:Subject:asText"), split),
            json!("Caf\u{e9}")
        );
        assert_eq!(
            value(&asked("header:Subject"), b"Subject: a\0b\r\n\r\n"),
            json!(" ab")
        );
        assert_eq!(
            value(&asked("header:Authentication-Results:asText:all"), HEADER),
            json!(["mx.example; dkim=pass", "other.example; spf=fail"])
        );
        assert_eq!(
            value(&asked("header:Subject:asText"), b"Subject:\r\n\r\n"),
            json!("")
        );
        assert_eq!(
            value(&asked("header:Subject"), b"Subject: a\xff\r\n\r\n"),
            json!(" a\u{fffd}")
        );
    }
}
