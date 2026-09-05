// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The private-network rule: which resolved addresses an upstream
//! connect may use.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use ipnet::{IpNet, Ipv4Net, Ipv6Net};

/// The four-byte blocks of the IANA special-purpose registry plus
/// multicast and the reserved range; no public mail server lives here.
const DENIED_V4: &[Ipv4Net] = &[
    Ipv4Net::new_assert(Ipv4Addr::UNSPECIFIED, 8),
    Ipv4Net::new_assert(Ipv4Addr::new(10, 0, 0, 0), 8),
    Ipv4Net::new_assert(Ipv4Addr::new(100, 64, 0, 0), 10),
    Ipv4Net::new_assert(Ipv4Addr::new(127, 0, 0, 0), 8),
    Ipv4Net::new_assert(Ipv4Addr::new(169, 254, 0, 0), 16),
    Ipv4Net::new_assert(Ipv4Addr::new(172, 16, 0, 0), 12),
    Ipv4Net::new_assert(Ipv4Addr::new(192, 0, 0, 0), 24),
    Ipv4Net::new_assert(Ipv4Addr::new(192, 0, 2, 0), 24),
    Ipv4Net::new_assert(Ipv4Addr::new(192, 88, 99, 0), 24),
    Ipv4Net::new_assert(Ipv4Addr::new(192, 168, 0, 0), 16),
    Ipv4Net::new_assert(Ipv4Addr::new(198, 18, 0, 0), 15),
    Ipv4Net::new_assert(Ipv4Addr::new(198, 51, 100, 0), 24),
    Ipv4Net::new_assert(Ipv4Addr::new(203, 0, 113, 0), 24),
    Ipv4Net::new_assert(Ipv4Addr::new(224, 0, 0, 0), 4),
    Ipv4Net::new_assert(Ipv4Addr::new(240, 0, 0, 0), 4),
];

/// The sixteen-byte counterpart: unspecified, loopback, mapped,
/// translation, discard, IETF assignments, 6to4, documentation, segment
/// routing, unique local, link local and multicast.
const DENIED_V6: &[Ipv6Net] = &[
    Ipv6Net::new_assert(Ipv6Addr::UNSPECIFIED, 128),
    Ipv6Net::new_assert(Ipv6Addr::LOCALHOST, 128),
    Ipv6Net::new_assert(Ipv6Addr::new(0, 0, 0, 0, 0, 0xffff, 0, 0), 96),
    Ipv6Net::new_assert(Ipv6Addr::new(0x64, 0xff9b, 1, 0, 0, 0, 0, 0), 48),
    Ipv6Net::new_assert(Ipv6Addr::new(0x100, 0, 0, 0, 0, 0, 0, 0), 64),
    Ipv6Net::new_assert(Ipv6Addr::new(0x2001, 0, 0, 0, 0, 0, 0, 0), 23),
    Ipv6Net::new_assert(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 0), 32),
    Ipv6Net::new_assert(Ipv6Addr::new(0x2002, 0, 0, 0, 0, 0, 0, 0), 16),
    Ipv6Net::new_assert(Ipv6Addr::new(0x3fff, 0, 0, 0, 0, 0, 0, 0), 20),
    Ipv6Net::new_assert(Ipv6Addr::new(0x5f00, 0, 0, 0, 0, 0, 0, 0), 16),
    Ipv6Net::new_assert(Ipv6Addr::new(0xfc00, 0, 0, 0, 0, 0, 0, 0), 7),
    Ipv6Net::new_assert(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 0), 10),
    Ipv6Net::new_assert(Ipv6Addr::new(0xff00, 0, 0, 0, 0, 0, 0, 0), 8),
];

/// Refuses every address inside a denied block unless the instance lists
/// its network.
#[derive(Debug, Clone)]
pub struct NetworkRule {
    allowed: Vec<IpNet>,
}

impl NetworkRule {
    #[must_use]
    pub fn new(allowed: &[IpNet]) -> Self {
        Self {
            allowed: allowed.to_vec(),
        }
    }

    /// Whether a connect may go to `address`.
    #[must_use]
    pub fn permits(&self, address: IpAddr) -> bool {
        !denied(address)
            || self
                .allowed
                .iter()
                .any(|network| network.contains(&address))
    }
}

fn denied(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => DENIED_V4.iter().any(|network| network.contains(&address)),
        IpAddr::V6(address) => DENIED_V6.iter().any(|network| network.contains(&address)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn address(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    fn rule(allowed: &[&str]) -> NetworkRule {
        let networks: Vec<IpNet> = allowed.iter().map(|net| net.parse().unwrap()).collect();
        NetworkRule::new(&networks)
    }

    #[test]
    fn special_purpose_addresses_are_denied_by_default() {
        let rule = rule(&[]);
        for text in [
            "0.0.0.1",
            "10.0.0.1",
            "100.64.0.1",
            "127.0.0.1",
            "169.254.1.1",
            "172.16.0.1",
            "192.0.0.1",
            "192.0.2.1",
            "192.88.99.1",
            "192.168.1.1",
            "198.18.0.1",
            "198.51.100.1",
            "203.0.113.1",
            "224.0.0.1",
            "255.255.255.255",
            "::",
            "::1",
            "::ffff:8.8.8.8",
            "64:ff9b:1::1",
            "100::1",
            "2001:db8::1",
            "2001::1",
            "2002::1",
            "3fff::1",
            "5f00::1",
            "fc00::1",
            "fd12::1",
            "fe80::1",
            "ff02::1",
        ] {
            assert!(!rule.permits(address(text)), "{text}");
        }
    }

    #[test]
    fn public_addresses_are_permitted() {
        let rule = rule(&[]);
        for text in [
            "8.8.8.8",
            "103.168.172.38",
            "2606:4700::1111",
            "2a00:1450:4001:80b::2005",
        ] {
            assert!(rule.permits(address(text)), "{text}");
        }
    }

    #[test]
    fn a_listed_network_is_permitted_and_no_other() {
        let rule = rule(&["127.0.0.0/8", "::1/128"]);
        assert!(rule.permits(address("127.0.0.1")));
        assert!(rule.permits(address("127.5.6.7")));
        assert!(rule.permits(address("::1")));
        assert!(!rule.permits(address("10.0.0.1")));
        assert!(!rule.permits(address("::ffff:127.0.0.1")));
    }
}
