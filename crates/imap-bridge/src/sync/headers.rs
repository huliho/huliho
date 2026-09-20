// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The header fields of one message, decoded into the personal fields
//! of the Email object (RFC 8621 section 4.1.2.3). Whoever sent the
//! mail wrote these bytes.

use mail_parser::{Address as Parsed, HeaderName, HeaderValue, Message, MessageParser};

use crate::dates::utc_date;
use crate::store::{Address, Personal};

/// The addresses kept per field; a header of `MAX_HEADER_BYTES` holds
/// more than any list a person reads.
const MAX_ADDRESSES: usize = 1024;

/// The ids kept per id field. Clients trim References far below it, so
/// the bound is about the blob and never about a real thread.
const MAX_MESSAGE_IDS: usize = 256;

/// What the header bytes say.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Headers {
    pub personal: Personal,
    /// The Date field in seconds since the epoch, where it holds a date
    /// a `UTCDate` can write.
    pub sent_at: Option<i64>,
}

/// Reads the fields; bytes that hold no header give empty fields. RFC
/// 2047 words are decoded and a group gives its members.
#[must_use]
pub fn parse(bytes: &[u8]) -> Headers {
    let Some(message) = MessageParser::default().parse_headers(bytes) else {
        return Headers::default();
    };
    Headers {
        personal: Personal {
            from: addresses(message.from()),
            to: addresses(message.to()),
            cc: addresses(message.cc()),
            bcc: addresses(message.bcc()),
            reply_to: addresses(message.reply_to()),
            sender: addresses(message.sender()),
            subject: message.subject().map(str::to_owned),
            message_id: ids(&message, HeaderName::MessageId),
            in_reply_to: ids(&message, HeaderName::InReplyTo),
            references: ids(&message, HeaderName::References),
        },
        sent_at: message
            .date()
            .filter(|date| date.is_valid())
            .map(mail_parser::DateTime::to_timestamp)
            .filter(|timestamp| utc_date(*timestamp).is_some()),
    }
}

/// The addresses of one field; an entry without an address is left out
/// and a field without any reads as absent.
fn addresses(parsed: Option<&Parsed<'_>>) -> Option<Vec<Address>> {
    let found: Vec<Address> = parsed?
        .iter()
        .filter_map(|entry| {
            Some(Address {
                name: entry
                    .name
                    .as_deref()
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned),
                email: entry.address.as_deref()?.to_owned(),
            })
        })
        .take(MAX_ADDRESSES)
        .collect();
    (!found.is_empty()).then_some(found)
}

/// The ids of one id field without their angle brackets.
fn ids(message: &Message<'_>, name: HeaderName<'static>) -> Option<Vec<String>> {
    let found: Vec<String> = match message.header(name)? {
        HeaderValue::Text(id) => vec![id.to_string()],
        HeaderValue::TextList(list) => list
            .iter()
            .take(MAX_MESSAGE_IDS)
            .map(ToString::to_string)
            .collect(),
        _ => Vec::new(),
    };
    (!found.is_empty()).then_some(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn address(name: Option<&str>, email: &str) -> Address {
        Address {
            name: name.map(str::to_owned),
            email: email.to_owned(),
        }
    }

    #[test]
    fn the_eleven_fields_read_into_the_personal_fields_rfc8621_4_1_2_3() {
        let headers = parse(
            b"From: Sanne <sanne@example.test>\r\n\
              Sender: desk@example.test\r\n\
              Reply-To: \"Desk, Front\" <desk@example.test>\r\n\
              To: team: mo@example.test, Li <li@example.test>;, kim@example.test\r\n\
              Subject: =?UTF-8?Q?Caf=C3=A9_om_drie_uur?=\r\n\
              Date: Sun, 7 Jul 1996 02:44:25 -0700\r\n\
              Message-ID: <m1@example.test>\r\n\
              In-Reply-To: <m0@example.test>\r\n\
              References: <a@example.test> <m0@example.test>\r\n\r\n",
        );
        let personal = &headers.personal;
        assert_eq!(
            personal.from,
            Some(vec![address(Some("Sanne"), "sanne@example.test")])
        );
        assert_eq!(
            personal.sender,
            Some(vec![address(None, "desk@example.test")])
        );
        assert_eq!(
            personal.reply_to,
            Some(vec![address(Some("Desk, Front"), "desk@example.test")])
        );
        assert_eq!(
            personal.to,
            Some(vec![
                address(None, "mo@example.test"),
                address(Some("Li"), "li@example.test"),
                address(None, "kim@example.test"),
            ])
        );
        assert_eq!(personal.cc, None);
        assert_eq!(personal.subject.as_deref(), Some("Café om drie uur"));
        assert_eq!(
            personal.message_id,
            Some(vec!["m1@example.test".to_owned()])
        );
        assert_eq!(
            personal.in_reply_to,
            Some(vec!["m0@example.test".to_owned()])
        );
        assert_eq!(
            personal.references,
            Some(vec![
                "a@example.test".to_owned(),
                "m0@example.test".to_owned()
            ])
        );
        assert_eq!(headers.sent_at, Some(836_732_665));
    }

    #[test]
    fn a_subject_in_a_multi_byte_charset_decodes_rfc2047() {
        let headers = parse(b"Subject: =?ISO-2022-JP?B?GyRCJDMkcyRLJEEkTxsoQg==?=\r\n\r\n");
        assert_eq!(
            headers.personal.subject.as_deref(),
            Some("\u{3053}\u{3093}\u{306b}\u{3061}\u{306f}")
        );
    }

    #[test]
    fn bytes_without_a_header_and_a_date_no_calendar_holds_give_nothing() {
        assert_eq!(parse(b""), Headers::default());
        assert_eq!(parse(b"\xff\xfe\x00garbage"), Headers::default());
        let headers = parse(b"Date: Mon, 99 Foo 0000 99:99:99 +9999\r\n\r\n");
        assert_eq!(headers.sent_at, None);
    }

    #[test]
    fn a_field_past_its_count_keeps_the_first_entries() {
        let many: Vec<String> = (0..MAX_ADDRESSES + 5)
            .map(|n| format!("u{n}@example.test"))
            .collect();
        let ids: Vec<String> = (0..MAX_MESSAGE_IDS + 5)
            .map(|n| format!("<r{n}@example.test>"))
            .collect();
        let raw = format!(
            "To: {}\r\nReferences: {}\r\n\r\n",
            many.join(", "),
            ids.join(" ")
        );
        let headers = parse(raw.as_bytes());
        assert_eq!(headers.personal.to.unwrap().len(), MAX_ADDRESSES);
        assert_eq!(headers.personal.references.unwrap().len(), MAX_MESSAGE_IDS);
    }
}
