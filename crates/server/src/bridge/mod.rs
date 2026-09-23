// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The in-process bridge as this server runs it: its connector signs in
//! with the sealed credential through the gate, its sealer keeps the
//! header cache under the instance secret and the accounts it serves
//! are the IMAP rows.

mod connector;
mod sealer;

use std::sync::Arc;

use huliho_imap_bridge::jmap::Urls;
use huliho_imap_bridge::runtime::{Bridge, Registration, Timing};
use huliho_imap_bridge::store::AccountKey;
use sha2::{Digest, Sha256};

pub use connector::HostConnector;
pub use sealer::HostSealer;

use crate::accounts::{Account, Provider};
use crate::api::ApiState;
use crate::gate::Reconnect;
use crate::ids::AccountId;
use crate::jmap;

/// The bridge on this server's connector.
pub type ServerBridge = Bridge<HostConnector>;

/// The bytes of the digest a session state shows; sixteen hex digits
/// tell any two apart.
const SESSION_STATE_BYTES: usize = 8;

/// The bridge wired from the state: the same gate, keys and resolver
/// the routes use, over the bridge's own connection to the database,
/// on the runtime's clocks.
#[must_use]
pub fn open(state: &ApiState) -> ServerBridge {
    open_with_timing(state, Timing::default())
}

/// The bridge wired from the state on the given clocks.
#[must_use]
pub fn open_with_timing(state: &ApiState, timing: Timing) -> ServerBridge {
    let connector = HostConnector::new(Reconnect::from(state));
    let sealer = HostSealer::new(Arc::clone(&state.keys));
    Bridge::with_timing(
        Arc::clone(&state.bridge_store),
        connector,
        Arc::new(sealer),
        timing,
    )
}

/// What the bridge is told about an account: its key, whether the row
/// says Gmail and the state of its session object.
#[must_use]
pub fn registration(account: &Account) -> Registration {
    Registration {
        key: key_of(&account.id),
        gmail: account.provider == Provider::Gmail,
        session_state: session_state(account),
    }
}

/// The account's key in the bridge's tables.
#[must_use]
pub fn key_of(account_id: &AccountId) -> AccountKey {
    AccountKey::new(account_id.as_str())
}

/// The URLs of the account's endpoint as the session object shows them.
#[must_use]
pub fn urls(account_id: &AccountId) -> Urls {
    let [(_, api), (_, download), (_, upload), (_, event_source)] = jmap::urls(account_id);
    Urls {
        api,
        download,
        upload,
        event_source,
    }
}

/// The state of a bridge account's session object: a digest over what
/// the object is built from, so it moves with a release or the account
/// and never with the cache.
fn session_state(account: &Account) -> String {
    let mut hasher = Sha256::new();
    for part in [
        env!("CARGO_PKG_VERSION"),
        account.id.as_str(),
        &account.address,
        account.provider.as_str(),
    ] {
        hasher.update(part.as_bytes());
        hasher.update([0]);
    }
    let digest = hasher.finalize();
    base16ct::lower::encode_string(&digest[..SESSION_STATE_BYTES])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::{AccountKind, AuthMethod};
    use crate::ids::{OrganizationId, UserId};

    fn account(id: &str, address: &str, provider: Provider) -> Account {
        Account {
            id: AccountId::from(id.to_owned()),
            organization_id: OrganizationId::from("o".to_owned()),
            user_id: UserId::from("u".to_owned()),
            address: address.to_owned(),
            name: "Mail".to_owned(),
            provider,
            kind: AccountKind::Imap,
            auth_method: AuthMethod::Password,
            stopped_cause: None,
            stopped_at: None,
            created_at: 0,
        }
    }

    #[test]
    fn the_registration_carries_the_key_the_gmail_word_and_a_stable_state() {
        let gmail = account("a1", "sanne@gmail.com", Provider::Gmail);
        let registered = registration(&gmail);
        assert_eq!(registered.key.as_str(), "a1");
        assert!(registered.gmail);
        assert_eq!(registered.session_state.len(), 2 * SESSION_STATE_BYTES);
        assert_eq!(registered.session_state, registration(&gmail).session_state);
        let generic = account("a1", "sanne@gmail.com", Provider::Generic);
        assert!(!registration(&generic).gmail);
        assert_ne!(
            registration(&generic).session_state,
            registered.session_state
        );
        let other = account("a2", "sanne@gmail.com", Provider::Gmail);
        assert_ne!(registration(&other).session_state, registered.session_state);
    }

    #[test]
    fn the_urls_are_the_ones_the_proxy_rewrites_to() {
        let urls = urls(&AccountId::from("a1".to_owned()));
        assert_eq!(urls.api, "/api/jmap/a1");
        assert_eq!(
            urls.download,
            "/api/jmap/a1/download/{accountId}/{blobId}/{name}?type={type}"
        );
        assert_eq!(urls.upload, "/api/jmap/a1/upload/{accountId}");
        assert_eq!(
            urls.event_source,
            "/api/jmap/a1/events?types={types}&closeafter={closeafter}&ping={ping}"
        );
    }
}
