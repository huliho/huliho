// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Where and how an account connects: the row's `settings` column,
//! which never holds a secret.

use serde::{Deserialize, Serialize};
use url::Url;

use super::AccountKind;

/// How a connection is encrypted; plain is not an option.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TlsMode {
    Implicit,
    Starttls,
}

/// One IMAP or SMTP server to reach.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
    pub tls: TlsMode,
}

/// The target of an account, one shape per protocol family.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
pub enum AccountSettings {
    Jmap {
        session_url: Url,
    },
    Imap {
        username: String,
        imap: Endpoint,
        smtp: Endpoint,
    },
}

impl AccountSettings {
    #[must_use]
    pub fn kind(&self) -> AccountKind {
        match self {
            Self::Jmap { .. } => AccountKind::Jmap,
            Self::Imap { .. } => AccountKind::Imap,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SESSION_URL: &str = "https://api.fastmail.com/jmap/session";

    fn imap() -> AccountSettings {
        AccountSettings::Imap {
            username: "sanne".to_owned(),
            imap: Endpoint {
                host: "imap.example.net".to_owned(),
                port: 993,
                tls: TlsMode::Implicit,
            },
            smtp: Endpoint {
                host: "smtp.example.net".to_owned(),
                port: 587,
                tls: TlsMode::Starttls,
            },
        }
    }

    #[test]
    fn a_plain_connection_is_not_a_setting() {
        let json = r#"{"kind":"imap","username":"sanne","imap":{"host":"h","port":143,"tls":"plain"},"smtp":{"host":"h","port":25,"tls":"plain"}}"#;
        assert!(serde_json::from_str::<AccountSettings>(json).is_err());
    }

    #[test]
    fn a_jmap_target_serializes_to_its_session_url() {
        let settings = AccountSettings::Jmap {
            session_url: Url::parse(SESSION_URL).unwrap(),
        };
        let json = serde_json::to_string(&settings).unwrap();
        assert_eq!(
            json,
            format!(r#"{{"kind":"jmap","sessionUrl":"{SESSION_URL}"}}"#)
        );
        assert_eq!(
            serde_json::from_str::<AccountSettings>(&json).unwrap(),
            settings
        );
        assert_eq!(settings.kind(), AccountKind::Jmap);
    }

    #[test]
    fn an_imap_target_serializes_both_endpoints() {
        let settings = imap();
        let json = serde_json::to_string(&settings).unwrap();
        assert_eq!(
            json,
            r#"{"kind":"imap","username":"sanne","imap":{"host":"imap.example.net","port":993,"tls":"implicit"},"smtp":{"host":"smtp.example.net","port":587,"tls":"starttls"}}"#
        );
        assert_eq!(
            serde_json::from_str::<AccountSettings>(&json).unwrap(),
            settings
        );
        assert_eq!(settings.kind(), AccountKind::Imap);
    }
}
