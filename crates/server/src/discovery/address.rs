// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A mail address as discovery reads it: the local part as typed, the
//! domain as a hostname.

use std::fmt;

use thiserror::Error;
use url::Host;

/// Longest address: the 256-octet path of RFC 5321 section 4.5.3.1.3
/// minus its angle brackets.
pub const MAX_ADDRESS_BYTES: usize = 254;

/// Longest DNS label (RFC 1035 section 2.3.4).
const MAX_LABEL_BYTES: usize = 63;

/// A mail domain is a name under a top-level domain.
const MIN_LABELS: usize = 2;

#[derive(Debug, Error, PartialEq, Eq)]
#[error("not a mail address")]
pub struct InvalidAddress;

/// `local@domain` with the domain in ASCII lowercase.
#[derive(Clone, PartialEq, Eq)]
pub struct Address {
    local: String,
    domain: String,
}

impl Address {
    /// Reads `local@domain`.
    ///
    /// # Errors
    ///
    /// Refuses a text over the RFC 5321 length, without exactly one `@`
    /// between two non-empty parts, with whitespace or control
    /// characters or whose domain is not a hostname of at least two
    /// labels.
    pub fn parse(text: &str) -> Result<Self, InvalidAddress> {
        if text.len() > MAX_ADDRESS_BYTES
            || text
                .chars()
                .any(|character| character.is_whitespace() || character.is_control())
        {
            return Err(InvalidAddress);
        }
        let (local, domain) = text.split_once('@').ok_or(InvalidAddress)?;
        if local.is_empty() || domain.contains('@') {
            return Err(InvalidAddress);
        }
        let domain = hostname(domain).ok_or(InvalidAddress)?;
        Ok(Self {
            local: local.to_owned(),
            domain,
        })
    }

    /// The domain in ASCII lowercase.
    #[must_use]
    pub fn domain(&self) -> &str {
        &self.domain
    }

    pub(super) fn local(&self) -> &str {
        &self.local
    }
}

/// The whole address with the domain normalized.
impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.local, self.domain)
    }
}

/// The domain only, so an address never reaches a log line by way of
/// `{:?}`.
impl fmt::Debug for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Address(@{})", self.domain)
    }
}

/// A host the chain may look up and connect to: a name, never an
/// address literal, so the connect resolves through the pinned resolver.
pub(super) fn named_host(text: &str) -> Option<String> {
    match Host::parse(text).ok()? {
        Host::Domain(name) => Some(name),
        Host::Ipv4(_) | Host::Ipv6(_) => None,
    }
}

/// A mail domain: mapped to ASCII lowercase, at least two labels of
/// letters, digits and inner hyphens, none over 63 octets.
fn hostname(text: &str) -> Option<String> {
    if text.contains('%') {
        return None;
    }
    let name = named_host(text)?;
    let labels: Vec<&str> = name.split('.').collect();
    let valid = labels.len() >= MIN_LABELS
        && labels.iter().all(|label| {
            !label.is_empty()
                && label.len() <= MAX_LABEL_BYTES
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        });
    valid.then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_address_parses_with_its_domain_lowercased() {
        let address = Address::parse("Sanne@Example.TEST").unwrap();
        assert_eq!(address.domain(), "example.test");
        assert_eq!(address.local(), "Sanne");
        assert_eq!(address.to_string(), "Sanne@example.test");
    }

    #[test]
    fn an_international_domain_maps_to_punycode() {
        let address = Address::parse("sanne@bücher.example").unwrap();
        assert_eq!(address.domain(), "xn--bcher-kva.example");
    }

    #[test]
    fn debug_output_leaves_the_local_part_out() {
        let address = Address::parse("sanne@example.test").unwrap();
        assert_eq!(format!("{address:?}"), "Address(@example.test)");
    }

    #[test]
    fn malformed_addresses_are_refused() {
        for text in [
            "",
            "sanne",
            "sanne@",
            "@example.test",
            "sanne@localhost",
            "sanne@127.0.0.1",
            "sanne@[::1]",
            "sanne@2130706433",
            "sanne@exa mple.test",
            "sanne\n@example.test",
            "sanne@example.test.",
            "sanne@-example.test",
            "sanne@ex_ample.test",
            "sanne@a..b",
            "sa@nne@example.test",
            "sanne@%41.test",
        ] {
            assert_eq!(Address::parse(text), Err(InvalidAddress), "{text:?}");
        }
    }

    #[test]
    fn the_length_limit_holds() {
        let domain = "@example.test";
        let fits = format!("{}{domain}", "a".repeat(MAX_ADDRESS_BYTES - domain.len()));
        assert!(Address::parse(&fits).is_ok());
        let long = format!(
            "{}{domain}",
            "a".repeat(MAX_ADDRESS_BYTES - domain.len() + 1)
        );
        assert_eq!(Address::parse(&long), Err(InvalidAddress));
    }

    #[test]
    fn a_host_is_a_name_never_an_address_literal() {
        assert_eq!(
            named_host("api.fastmail.com"),
            Some("api.fastmail.com".to_owned())
        );
        assert_eq!(
            named_host("API.Fastmail.com"),
            Some("api.fastmail.com".to_owned())
        );
        for text in ["127.0.0.1", "[::1]", "2130706433", ""] {
            assert_eq!(named_host(text), None, "{text:?}");
        }
    }
}
