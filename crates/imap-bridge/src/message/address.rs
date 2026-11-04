// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! One address as a header carries it: ASCII on both sides of the `@` (RFC
//! 5322 section 2.2), since the bridge never enables UTF8=ACCEPT, so a
//! domain comes as its A-label. The characters of an address list stay out
//! of the local part: a `>` or a comma there would make one address read
//! as two.

/// The bytes of one address (RFC 5321 section 4.5.3.1.3 less its angle
/// brackets).
const MAX_ADDRESS_BYTES: usize = 254;

/// The bytes of one label of a domain (RFC 1035 section 2.3.4).
const MAX_LABEL_BYTES: usize = 63;

/// The labels a domain holds at least.
const MIN_DOMAIN_LABELS: usize = 2;

/// What no local part holds beside the dot, so it is a dot-atom (RFC 5322
/// section 3.4.1) and a quoted one is left out.
const LOCAL_SPECIALS: &str = "<>()[],;:\\\"";

/// Whether the text is one address the bridge writes into a header. An
/// address literal is no name, as the client reads it.
#[must_use]
pub fn fits_address(address: &str) -> bool {
    if address.len() > MAX_ADDRESS_BYTES {
        return false;
    }
    let Some((local, domain)) = address.split_once('@') else {
        return false;
    };
    let plain_local = local.split('.').all(|atom| {
        !atom.is_empty()
            && atom
                .chars()
                .all(|c| c.is_ascii_graphic() && !LOCAL_SPECIALS.contains(c))
    });
    let named = !domain
        .bytes()
        .all(|byte| byte.is_ascii_digit() || byte == b'.');
    let labels = domain.split('.');
    plain_local
        && named
        && labels.clone().count() >= MIN_DOMAIN_LABELS
        && labels.into_iter().all(label)
}

/// One label of a domain: letters, digits and inner hyphens in ASCII.
fn label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= MAX_LABEL_BYTES
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_is_one_local_part_and_a_domain_of_two_labels_rfc5322_3_4_1() {
        let longest_label = format!("a@{}.test", "b".repeat(MAX_LABEL_BYTES));
        let long_label = format!("a@{}.test", "b".repeat(MAX_LABEL_BYTES + 1));
        for address in [
            "a@b.test",
            "a.b+c@sub.b-c.test",
            "a@xn--bcher-kva.example",
            "a@1.test",
            longest_label.as_str(),
        ] {
            assert!(fits_address(address), "{address}");
        }
        let domain = "@b.test";
        let local = "a".repeat(MAX_ADDRESS_BYTES - domain.len());
        assert!(fits_address(&format!("{local}{domain}")));
        let long = format!("a{local}{domain}");
        for address in [
            "",
            "a",
            "@b.test",
            "a@",
            "a@b",
            "a@b..test",
            "a@-b.test",
            "a@b-.test",
            "a@b_c.test",
            "a@jos\u{e9}.test",
            "a@b@c.test",
            "a b@c.test",
            "a\r\n@c.test",
            "a\0@c.test",
            "jos\u{e9}@b.test",
            ".a@b.test",
            "a.@b.test",
            "a..b@b.test",
            "a@127.0.0.1",
            long_label.as_str(),
            long.as_str(),
        ] {
            assert!(!fits_address(address), "{address:?}");
        }
    }

    #[test]
    fn a_local_part_never_reads_as_two_addresses_rfc5322_3_4() {
        for address in [
            "a>,<evil@b.test",
            "a,evil@b.test",
            "a;evil@b.test",
            "a:evil@b.test",
            "\"a b\"@b.test",
            "a(b)@b.test",
            "a[b]@b.test",
            "a\\b@b.test",
        ] {
            assert!(!fits_address(address), "{address}");
        }
    }
}
