// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The providers a mail account can sign in through, with the endpoints
//! and the scopes each one's consent uses.

use crate::ids::text_enum;

text_enum!(
    /// The providers a mail account can sign in through.
    OauthProvider {
        Google => "google",
        Microsoft => "microsoft",
    }
);

/// A refresh token comes only when the consent asks for it: Google wants
/// these two parameters, Microsoft a scope.
const GOOGLE_EXTRA_PARAMS: &[(&str, &str)] = &[("access_type", "offline"), ("prompt", "consent")];

/// Full mailbox access, the one scope Gmail's IMAP and SMTP accept.
const GOOGLE_SCOPES: &[&str] = &["https://mail.google.com/"];

/// IMAP and SMTP access plus the refresh token.
const MICROSOFT_SCOPES: &[&str] = &[
    "https://outlook.office.com/IMAP.AccessAsUser.All",
    "https://outlook.office.com/SMTP.Send",
    "offline_access",
];

pub(super) const ALL: [OauthProvider; 2] = [OauthProvider::Google, OauthProvider::Microsoft];

impl OauthProvider {
    /// The provider behind a word from a URL path.
    #[must_use]
    pub fn from_word(word: &str) -> Option<Self> {
        ALL.into_iter().find(|provider| provider.as_str() == word)
    }

    /// The issuer, which keys the provider row.
    #[must_use]
    pub fn issuer(self) -> &'static str {
        match self {
            Self::Google => "https://accounts.google.com",
            Self::Microsoft => "https://login.microsoftonline.com/common/v2.0",
        }
    }

    /// The `OpenID` configuration document, for the sign-in that comes
    /// later.
    #[must_use]
    pub fn discovery_url(self) -> &'static str {
        match self {
            Self::Google => "https://accounts.google.com/.well-known/openid-configuration",
            Self::Microsoft => {
                "https://login.microsoftonline.com/common/v2.0/.well-known/openid-configuration"
            }
        }
    }

    /// The authorization endpoint the discovery document names.
    #[must_use]
    pub fn authorization_url(self) -> &'static str {
        match self {
            Self::Google => "https://accounts.google.com/o/oauth2/v2/auth",
            Self::Microsoft => "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
        }
    }

    /// The token endpoint the discovery document names.
    #[must_use]
    pub fn token_url(self) -> &'static str {
        match self {
            Self::Google => "https://oauth2.googleapis.com/token",
            Self::Microsoft => "https://login.microsoftonline.com/common/oauth2/v2.0/token",
        }
    }

    /// The least a mail client asks for.
    #[must_use]
    pub fn scopes(self) -> &'static [&'static str] {
        match self {
            Self::Google => GOOGLE_SCOPES,
            Self::Microsoft => MICROSOFT_SCOPES,
        }
    }

    /// Parameters beyond the standard ones the consent needs.
    #[must_use]
    pub fn extra_params(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Google => GOOGLE_EXTRA_PARAMS,
            Self::Microsoft => &[],
        }
    }

    pub(super) fn from_issuer(issuer: &str) -> Option<Self> {
        ALL.into_iter().find(|provider| provider.issuer() == issuer)
    }
}

#[cfg(test)]
mod tests {
    use url::Url;

    use super::*;

    #[test]
    fn the_endpoints_are_https_and_the_discovery_url_hangs_off_the_issuer() {
        for provider in ALL {
            assert_eq!(
                provider.discovery_url(),
                format!("{}/.well-known/openid-configuration", provider.issuer())
            );
            for text in [provider.authorization_url(), provider.token_url()] {
                let url = Url::parse(text).unwrap();
                assert_eq!(url.scheme(), "https", "{text}");
            }
            assert!(!provider.scopes().is_empty());
            assert_eq!(OauthProvider::from_word(provider.as_str()), Some(provider));
        }
        assert_eq!(OauthProvider::from_word("yahoo"), None);
        assert!(
            OauthProvider::Microsoft
                .scopes()
                .contains(&"offline_access")
        );
        assert!(OauthProvider::Microsoft.extra_params().is_empty());
        assert_eq!(OauthProvider::Google.extra_params().len(), 2);
    }

    #[test]
    fn the_provider_words_are_stable() {
        assert_eq!(OauthProvider::Google.as_str(), "google");
        assert_eq!(OauthProvider::Microsoft.as_str(), "microsoft");
        assert_eq!(
            serde_json::to_string(&OauthProvider::Microsoft).unwrap(),
            "\"microsoft\""
        );
    }
}
