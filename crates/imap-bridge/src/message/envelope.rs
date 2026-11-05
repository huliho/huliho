// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What a send reads of the message the user saved (RFC 8621 section
//! 7): one sender that is the identity's address, then the recipients
//! of its To, Cc and Bcc fields, each once. A list the call gives names
//! some or all of those and nothing beyond them.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};

use mail_parser::{HeaderName, MessageParser};

use super::fits_address;
use crate::sync::headers;

/// Why a message cannot go out as it stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unsendable {
    /// The header lacks one From field with one sender, holds a second
    /// Sender field or reads otherwise on the wire than in the draft: the
    /// property at fault (RFC 8621 section 7.5).
    InvalidEmail(&'static str),
    /// The sender is not the identity's address.
    ForbiddenFrom,
    NoRecipients,
    /// Addresses the message does not name or no command line takes.
    InvalidRecipients(Vec<String>),
}

/// The recipients of the message, which must come from `identity`: every
/// address of To, Cc and Bcc in that order, each once as the server
/// compares them, without regard to case, spelled as the message first
/// spells it. A list the call gives picks which of those get the message
/// and in which order; an address outside them is named back as the call
/// spelled it, beside any no command line takes.
///
/// # Errors
///
/// Returns what the message lacks.
pub fn recipients(
    message: &[u8],
    identity: &str,
    given: Option<&[String]>,
) -> Result<Vec<String>, Unsendable> {
    let fields = |name: HeaderName<'static>| {
        MessageParser::default()
            .parse_headers(message)
            .map_or(0, |parsed| {
                parsed
                    .headers()
                    .iter()
                    .filter(|header| header.name == name)
                    .count()
            })
    };
    if fields(HeaderName::From) != 1 {
        return Err(Unsendable::InvalidEmail("from"));
    }
    if fields(HeaderName::Sender) > 1 {
        return Err(Unsendable::InvalidEmail("sender"));
    }
    let personal = headers::parse(message).personal;
    let [sender] = personal.from.as_deref().unwrap_or_default() else {
        return Err(Unsendable::InvalidEmail("from"));
    };
    if !sender.email.eq_ignore_ascii_case(identity) {
        return Err(Unsendable::ForbiddenFrom);
    }
    let listed = [&personal.to, &personal.cc, &personal.bcc]
        .into_iter()
        .flatten()
        .flatten()
        .map(|address| address.email.clone());
    let mut saved: HashMap<String, String> = HashMap::new();
    let mut own: Vec<String> = Vec::new();
    for address in listed {
        if let Entry::Vacant(slot) = saved.entry(address.to_ascii_lowercase()) {
            slot.insert(address.clone());
            own.push(address);
        }
    }
    let named = given.map_or(own, <[String]>::to_vec);
    let mut seen: HashSet<String> = HashSet::with_capacity(named.len());
    let mut sending: Vec<String> = Vec::with_capacity(named.len());
    let mut unfit: Vec<String> = Vec::new();
    for address in named {
        let key = address.to_ascii_lowercase();
        if !seen.insert(key.clone()) {
            continue;
        }
        match saved.get(&key) {
            Some(spelled) if fits_address(spelled) => sending.push(spelled.clone()),
            _ => unfit.push(address),
        }
    }
    if sending.is_empty() && unfit.is_empty() {
        return Err(Unsendable::NoRecipients);
    }
    if unfit.is_empty() {
        Ok(sending)
    } else {
        Err(Unsendable::InvalidRecipients(unfit))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ME: &str = "sanne@example.test";

    fn read(header: &str) -> Result<Vec<String>, Unsendable> {
        recipients(format!("{header}\r\n\r\nbody").as_bytes(), ME, None)
    }

    #[test]
    fn every_address_of_to_cc_and_bcc_goes_once_in_that_order_rfc8621_7() {
        let found = read(
            "From: Sanne <sanne@example.test>\r\nTo: mo@example.test, Li <li@example.test>\r\n\
Cc: MO@example.test\r\nBcc: team: kim@example.test;",
        );
        assert_eq!(
            found.unwrap(),
            ["mo@example.test", "li@example.test", "kim@example.test"]
        );
    }

    #[test]
    fn a_given_list_sends_to_those_of_the_message_it_names_and_no_other_rfc8621_7() {
        let message = "From: sanne@example.test\r\nTo: mo@example.test, li@example.test\r\n\
Cc: kim@example.test\r\n\r\n";
        let some = ["Li@example.test".to_owned(), "LI@example.test".to_owned()];
        assert_eq!(
            recipients(message.as_bytes(), ME, Some(&some)).unwrap(),
            ["li@example.test"]
        );
        let beyond = [
            "kim@example.test".to_owned(),
            "pat@example.test".to_owned(),
            "mo@example.test".to_owned(),
        ];
        assert_eq!(
            recipients(message.as_bytes(), ME, Some(&beyond)),
            Err(Unsendable::InvalidRecipients(vec![
                "pat@example.test".to_owned()
            ]))
        );
        assert_eq!(
            recipients(message.as_bytes(), ME, Some(&[])),
            Err(Unsendable::NoRecipients)
        );
    }

    #[test]
    fn a_given_list_as_long_as_a_request_holds_sends_to_each_once_rfc8621_7() {
        // The most Address entries one request body holds: the bound over
        // the bytes of the smallest entry with its comma.
        const SMALLEST_ENTRY: &str = r#"{"email":""},"#;
        const ENTRIES: usize = crate::jmap::MAX_SIZE_REQUEST / SMALLEST_ENTRY.len();
        let message = "From: sanne@example.test\r\nTo: mo@example.test, li@example.test\r\n\
Cc: kim@example.test\r\n\r\n";
        let spellings = [
            "Mo@Example.Test",
            "LI@example.test",
            "kim@EXAMPLE.test",
            "mo@example.test",
            "li@EXAMPLE.TEST",
            "KIM@example.test",
        ];
        let given: Vec<String> = spellings
            .iter()
            .cycle()
            .take(ENTRIES)
            .map(|&spelling| spelling.to_owned())
            .collect();
        assert_eq!(
            recipients(message.as_bytes(), ME, Some(&given)).unwrap(),
            ["mo@example.test", "li@example.test", "kim@example.test"]
        );
    }

    #[test]
    fn a_given_list_picks_the_order_and_each_address_goes_as_the_message_spells_it_rfc8621_7() {
        let message = "From: sanne@example.test\r\nTo: Mo@Example.Test, li@example.test\r\n\
Cc: mo@example.test\r\n\r\n";
        let given = ["LI@example.test".to_owned(), "MO@example.test".to_owned()];
        assert_eq!(
            recipients(message.as_bytes(), ME, Some(&given)).unwrap(),
            ["li@example.test", "Mo@Example.Test"]
        );
        assert_eq!(
            recipients(message.as_bytes(), ME, None).unwrap(),
            ["Mo@Example.Test", "li@example.test"]
        );
    }

    #[test]
    fn one_sender_that_is_the_identity_or_nothing_goes_rfc8621_7_5() {
        let none = read("To: mo@example.test");
        assert_eq!(none, Err(Unsendable::InvalidEmail("from")));
        let two = read("From: sanne@example.test, mo@example.test\r\nTo: li@example.test");
        assert_eq!(two, Err(Unsendable::InvalidEmail("from")));
        let twice =
            read("From: sanne@example.test\r\nFrom: sanne@example.test\r\nTo: li@example.test");
        assert_eq!(twice, Err(Unsendable::InvalidEmail("from")));
        let senders = read(
            "From: sanne@example.test\r\nSender: a@example.test\r\nSender: b@example.test\r\n\
To: li@example.test",
        );
        assert_eq!(senders, Err(Unsendable::InvalidEmail("sender")));
        let other = read("From: mo@example.test\r\nTo: li@example.test");
        assert_eq!(other, Err(Unsendable::ForbiddenFrom));
        let cased = read("From: Sanne@Example.Test\r\nTo: li@example.test");
        assert_eq!(cased.unwrap(), ["li@example.test"]);
    }

    #[test]
    fn no_recipient_or_one_no_command_line_takes_is_named() {
        assert_eq!(
            read("From: sanne@example.test"),
            Err(Unsendable::NoRecipients)
        );
        let odd = read("From: sanne@example.test\r\nTo: mo@example.test, pat@b_c.test");
        assert_eq!(
            odd,
            Err(Unsendable::InvalidRecipients(vec![
                "pat@b_c.test".to_owned()
            ]))
        );
        let message = b"From: sanne@example.test\r\nTo: pat@b_c.test\r\n\r\n";
        let both = ["pat@b_c.test".to_owned(), "a>b@example.test".to_owned()];
        assert_eq!(
            recipients(message, ME, Some(&both)),
            Err(Unsendable::InvalidRecipients(both.to_vec()))
        );
    }
}
