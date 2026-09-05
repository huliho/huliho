// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Fixtures for tests that put account rows into the store directly.

use huliho_server::accounts::{AccountSettings, Credential, NewAccount, Provider};
use huliho_server::secrets::{InstanceSecret, Keys};

/// The same instance secret the HTTP fixtures derive their keys from,
/// so a row sealed here opens inside a test router.
const SECRET: &[u8] = b"0123456789abcdef0123456789abcdef";

/// The token every fixture account carries.
pub const ACCOUNT_TOKEN: &str = "fmu1-example-api-token";

pub fn keys() -> Keys {
    Keys::derive(
        &InstanceSecret::from_bytes(SECRET.to_vec()).expect("the fixture secret is long enough"),
    )
}

/// A Fastmail account reached over JMAP with a bearer token.
pub fn new_account(address: &str) -> NewAccount {
    NewAccount {
        address: address.to_owned(),
        name: "Fastmail".to_owned(),
        provider: Provider::Fastmail,
        settings: AccountSettings::Jmap {
            session_url: "https://api.fastmail.com/jmap/session"
                .parse()
                .expect("the fixture URL parses"),
        },
        credential: Credential::Bearer {
            token: ACCOUNT_TOKEN.to_owned(),
        },
    }
}
