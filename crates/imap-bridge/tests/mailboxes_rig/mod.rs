// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A scripted server with a mailbox model, a signed-in session against
//! it and one pass into a store.

use std::sync::Arc;
use std::time::Duration;

use huliho_imap_bridge::mailboxes::{SyncError, sync};
use huliho_imap_bridge::session::{ImapSession, Session, TlsMode};
use huliho_imap_bridge::store::{AccountKey, Store};
use huliho_imap_bridge::sync::Cache;
use huliho_imap_bridge::testing::imap::{FakeImap, HOST, Script};
use huliho_imap_bridge::testing::seal::TestSealer;
use huliho_imap_bridge::testing::{Mailboxes, PASSWORD, USER};

/// Room for a loopback exchange.
const STEP: Duration = Duration::from_secs(1);

/// The one account of the rig.
pub fn key() -> AccountKey {
    AccountKey::new("a1")
}

/// A cache over the store for that account, as a folder account.
pub fn cache(store: &Arc<Store>) -> Cache {
    Cache {
        store: Arc::clone(store),
        sealer: Arc::new(TestSealer::default()),
        key: key(),
        gmail: false,
    }
}

/// The TLS script over the given mailbox model.
pub fn script(mailboxes: Mailboxes) -> Script {
    Script {
        mailboxes,
        ..Script::tls()
    }
}

/// A session against the scripted server, past LOGIN.
pub async fn signed_in(fake: &FakeImap) -> ImapSession {
    let target = fake.target(HOST, TlsMode::Implicit);
    let mut session = ImapSession::connect(fake.trusting(), &target, STEP)
        .await
        .unwrap();
    session.login(USER, PASSWORD).await.unwrap();
    session
}

/// One pass on a fresh session that logs out after it; the state it ends at.
pub async fn pass(fake: &FakeImap, store: &Arc<Store>) -> Result<u64, SyncError> {
    let mut session = signed_in(fake).await;
    let state = sync(&mut session, &cache(store)).await;
    session.logout().await.unwrap();
    state
}

/// The commands received, without the phase and the tag.
pub fn commands(fake: &FakeImap) -> Vec<String> {
    fake.lines()
        .iter()
        .map(|line| line.splitn(3, ' ').nth(2).unwrap_or_default().to_owned())
        .collect()
}
