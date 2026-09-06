// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The provider presets: which domains and hosts mark a provider, its
//! fixed servers and the credential it takes.

use serde::Serialize;
use url::Url;

use crate::accounts::{AccountSettings, Endpoint, Provider, TlsMode};
use crate::discovery::Address;
use crate::providers::OauthProvider;

/// The IANA ports for IMAP over TLS, SMTP over TLS and submission
/// (RFC 8314 sections 3.2 and 3.3, RFC 6409 section 3.1).
const IMAPS_PORT: u16 = 993;
const SMTPS_PORT: u16 = 465;
const SUBMISSION_PORT: u16 = 587;

/// What the user hands over for a preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CredentialKind {
    Password,
    AppPassword,
    ApiToken,
    Oauth,
}

/// A server a preset fixes: host, port and TLS mode.
type Fixed = (&'static str, u16, TlsMode);

/// Where a provider's accounts connect, when the provider fixes it.
enum FixedTarget {
    Jmap(&'static str),
    Imap { imap: Fixed, smtp: Fixed },
    Unknown,
}

pub struct Preset {
    pub provider: Provider,
    /// The name a new account gets when the user leaves it blank; a
    /// generic server has none.
    pub display_name: Option<&'static str>,
    /// Mail domains that name the provider before any lookup.
    pub domains: &'static [&'static str],
    /// Host suffixes that mark the provider: its MX names and its servers.
    pub host_suffixes: &'static [&'static str],
    pub credential_kind: CredentialKind,
    /// The sign-in provider behind the preset, when it has one.
    pub oauth: Option<OauthProvider>,
    target: FixedTarget,
}

const GMAIL: Preset = Preset {
    provider: Provider::Gmail,
    display_name: Some("Gmail"),
    domains: &["gmail.com", "googlemail.com"],
    host_suffixes: &["gmail.com", "google.com", "googlemail.com"],
    credential_kind: CredentialKind::AppPassword,
    oauth: Some(OauthProvider::Google),
    target: FixedTarget::Imap {
        imap: ("imap.gmail.com", IMAPS_PORT, TlsMode::Implicit),
        smtp: ("smtp.gmail.com", SMTPS_PORT, TlsMode::Implicit),
    },
};

const MICROSOFT: Preset = Preset {
    provider: Provider::Microsoft,
    display_name: Some("Microsoft"),
    domains: &["outlook.com", "hotmail.com", "live.com", "msn.com"],
    host_suffixes: &["outlook.com", "office365.com"],
    credential_kind: CredentialKind::Oauth,
    oauth: Some(OauthProvider::Microsoft),
    target: FixedTarget::Imap {
        imap: ("outlook.office365.com", IMAPS_PORT, TlsMode::Implicit),
        smtp: ("smtp.office365.com", SUBMISSION_PORT, TlsMode::Starttls),
    },
};

const FASTMAIL: Preset = Preset {
    provider: Provider::Fastmail,
    display_name: Some("Fastmail"),
    domains: &["fastmail.com"],
    host_suffixes: &["fastmail.com", "messagingengine.com"],
    credential_kind: CredentialKind::ApiToken,
    oauth: None,
    target: FixedTarget::Jmap("https://api.fastmail.com/jmap/session"),
};

const ICLOUD: Preset = Preset {
    provider: Provider::Icloud,
    display_name: Some("iCloud"),
    domains: &["icloud.com", "me.com", "mac.com"],
    host_suffixes: &["icloud.com", "me.com"],
    credential_kind: CredentialKind::AppPassword,
    oauth: None,
    target: FixedTarget::Imap {
        imap: ("imap.mail.me.com", IMAPS_PORT, TlsMode::Implicit),
        smtp: ("smtp.mail.me.com", SUBMISSION_PORT, TlsMode::Starttls),
    },
};

const YAHOO: Preset = Preset {
    provider: Provider::Yahoo,
    display_name: Some("Yahoo"),
    domains: &["yahoo.com", "ymail.com", "rocketmail.com"],
    host_suffixes: &["yahoo.com", "yahoodns.net"],
    credential_kind: CredentialKind::AppPassword,
    oauth: None,
    target: FixedTarget::Imap {
        imap: ("imap.mail.yahoo.com", IMAPS_PORT, TlsMode::Implicit),
        smtp: ("smtp.mail.yahoo.com", SMTPS_PORT, TlsMode::Implicit),
    },
};

const GENERIC: Preset = Preset {
    provider: Provider::Generic,
    display_name: None,
    domains: &[],
    host_suffixes: &[],
    credential_kind: CredentialKind::Password,
    oauth: None,
    target: FixedTarget::Unknown,
};

const PRESETS: [&Preset; 6] = [&GMAIL, &MICROSOFT, &FASTMAIL, &ICLOUD, &YAHOO, &GENERIC];

/// The preset of a provider.
#[must_use]
pub fn for_provider(provider: Provider) -> &'static Preset {
    match provider {
        Provider::Gmail => &GMAIL,
        Provider::Microsoft => &MICROSOFT,
        Provider::Fastmail => &FASTMAIL,
        Provider::Icloud => &ICLOUD,
        Provider::Yahoo => &YAHOO,
        Provider::Generic => &GENERIC,
    }
}

/// The provider a mail domain names outright, before any lookup.
#[must_use]
pub fn provider_for_domain(domain: &str) -> Option<Provider> {
    PRESETS
        .iter()
        .find(|preset| preset.domains.contains(&domain))
        .map(|preset| preset.provider)
}

/// The provider a server or MX name marks by its suffix; generic when
/// none does.
#[must_use]
pub fn provider_for_host(host: &str) -> Provider {
    PRESETS
        .iter()
        .find(|preset| {
            preset
                .host_suffixes
                .iter()
                .any(|suffix| ends_with_labels(host, suffix))
        })
        .map_or(Provider::Generic, |preset| preset.provider)
}

/// The name a new account gets when the user leaves it blank: the
/// provider's name, the mail domain for a generic server.
#[must_use]
pub fn default_name(provider: Provider, address: &Address) -> String {
    for_provider(provider)
        .display_name
        .map_or_else(|| address.domain().to_owned(), str::to_owned)
}

/// The fixed servers of a provider as an account target, `None` for a
/// provider without them. The address is the username.
///
/// # Panics
///
/// Only if a fixed session URL fails to parse, which a test rules out.
#[must_use]
pub fn fixed_target(provider: Provider, address: &Address) -> Option<AccountSettings> {
    match for_provider(provider).target {
        FixedTarget::Jmap(session_url) => Some(AccountSettings::Jmap {
            session_url: Url::parse(session_url).expect("a fixed session URL parses"),
        }),
        FixedTarget::Imap { imap, smtp } => Some(AccountSettings::Imap {
            username: address.to_string(),
            imap: endpoint(imap),
            smtp: endpoint(smtp),
        }),
        FixedTarget::Unknown => None,
    }
}

fn endpoint((host, port, tls): Fixed) -> Endpoint {
    Endpoint {
        host: host.to_owned(),
        port,
        tls,
    }
}

/// Whether `host` is `suffix` or ends in `.suffix`.
fn ends_with_labels(host: &str, suffix: &str) -> bool {
    host == suffix
        || host
            .strip_suffix(suffix)
            .is_some_and(|rest| rest.ends_with('.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Provider; 6] = [
        Provider::Gmail,
        Provider::Microsoft,
        Provider::Fastmail,
        Provider::Icloud,
        Provider::Yahoo,
        Provider::Generic,
    ];

    fn address(text: &str) -> Address {
        Address::parse(text).unwrap()
    }

    #[test]
    fn a_suffix_matches_whole_labels_only() {
        assert_eq!(provider_for_host("aspmx.l.google.com"), Provider::Gmail);
        assert_eq!(provider_for_host("google.com"), Provider::Gmail);
        assert_eq!(provider_for_host("notgoogle.com"), Provider::Generic);
        assert_eq!(provider_for_host("google.com.example"), Provider::Generic);
    }

    #[test]
    fn the_mx_names_of_the_known_providers_mark_them() {
        for (host, provider) in [
            ("gmail-smtp-in.l.google.com", Provider::Gmail),
            ("eur.olc.protection.outlook.com", Provider::Microsoft),
            ("in1-smtp.messagingengine.com", Provider::Fastmail),
            ("mx01.mail.icloud.com", Provider::Icloud),
            ("mta5.am0.yahoodns.net", Provider::Yahoo),
            ("mail.example.org", Provider::Generic),
        ] {
            assert_eq!(provider_for_host(host), provider, "{host}");
        }
    }

    #[test]
    fn the_servers_of_the_known_providers_mark_them() {
        for (host, provider) in [
            ("api.fastmail.com", Provider::Fastmail),
            ("imap.gmail.com", Provider::Gmail),
            ("outlook.office365.com", Provider::Microsoft),
            ("imap.mail.me.com", Provider::Icloud),
            ("imap.mail.yahoo.com", Provider::Yahoo),
        ] {
            assert_eq!(provider_for_host(host), provider, "{host}");
        }
    }

    #[test]
    fn the_well_known_domains_name_their_provider() {
        for (domain, provider) in [
            ("gmail.com", Provider::Gmail),
            ("googlemail.com", Provider::Gmail),
            ("outlook.com", Provider::Microsoft),
            ("hotmail.com", Provider::Microsoft),
            ("live.com", Provider::Microsoft),
            ("msn.com", Provider::Microsoft),
            ("fastmail.com", Provider::Fastmail),
            ("icloud.com", Provider::Icloud),
            ("me.com", Provider::Icloud),
            ("mac.com", Provider::Icloud),
            ("yahoo.com", Provider::Yahoo),
            ("ymail.com", Provider::Yahoo),
            ("rocketmail.com", Provider::Yahoo),
        ] {
            assert_eq!(provider_for_domain(domain), Some(provider), "{domain}");
        }
        assert_eq!(provider_for_domain("example.com"), None);
        assert_eq!(provider_for_domain("fastmail.fm"), None);
    }

    #[test]
    fn every_preset_with_domains_fixes_a_target_and_generic_fixes_none() {
        for provider in ALL {
            let preset = for_provider(provider);
            assert_eq!(preset.provider, provider);
            let target = fixed_target(provider, &address("sanne@example.test"));
            assert_eq!(target.is_some(), !preset.domains.is_empty(), "{provider:?}");
        }
    }

    #[test]
    fn a_fixed_imap_target_takes_the_address_as_username() {
        match fixed_target(Provider::Gmail, &address("sanne@gmail.com")) {
            Some(AccountSettings::Imap {
                username,
                imap,
                smtp,
            }) => {
                assert_eq!(username, "sanne@gmail.com");
                assert_eq!(
                    (imap.host.as_str(), imap.port, imap.tls),
                    ("imap.gmail.com", 993, TlsMode::Implicit)
                );
                assert_eq!(
                    (smtp.host.as_str(), smtp.port, smtp.tls),
                    ("smtp.gmail.com", 465, TlsMode::Implicit)
                );
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn fastmail_connects_over_jmap_with_a_token() {
        let target = fixed_target(Provider::Fastmail, &address("mira@fastmail.com"));
        assert_eq!(
            target,
            Some(AccountSettings::Jmap {
                session_url: Url::parse("https://api.fastmail.com/jmap/session").unwrap(),
            })
        );
        assert_eq!(
            for_provider(Provider::Fastmail).credential_kind,
            CredentialKind::ApiToken
        );
    }

    #[test]
    fn microsoft_has_no_password_route() {
        let preset = for_provider(Provider::Microsoft);
        assert_eq!(preset.credential_kind, CredentialKind::Oauth);
        assert_eq!(preset.oauth, Some(OauthProvider::Microsoft));
        assert_eq!(
            for_provider(Provider::Gmail).oauth,
            Some(OauthProvider::Google)
        );
        assert_eq!(for_provider(Provider::Generic).oauth, None);
    }

    #[test]
    fn the_credential_kinds_serialize_in_camel_case() {
        for (kind, word) in [
            (CredentialKind::Password, "\"password\""),
            (CredentialKind::AppPassword, "\"appPassword\""),
            (CredentialKind::ApiToken, "\"apiToken\""),
            (CredentialKind::Oauth, "\"oauth\""),
        ] {
            assert_eq!(serde_json::to_string(&kind).unwrap(), word);
        }
    }

    #[test]
    fn the_default_name_is_the_provider_or_the_domain() {
        for (provider, name) in [
            (Provider::Gmail, "Gmail"),
            (Provider::Microsoft, "Microsoft"),
            (Provider::Fastmail, "Fastmail"),
            (Provider::Icloud, "iCloud"),
            (Provider::Yahoo, "Yahoo"),
            (Provider::Generic, "example.test"),
        ] {
            assert_eq!(default_name(provider, &address("sanne@example.test")), name);
        }
    }
}
