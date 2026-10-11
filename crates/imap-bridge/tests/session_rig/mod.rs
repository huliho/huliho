// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A scripted server over a mailbox model and a session signed in on
//! it, for the tests that drive the session itself.

use std::time::Duration;

use huliho_imap_bridge::session::{ImapSession, Session, TlsMode};
use huliho_imap_bridge::testing::imap::{FakeImap, HOST, Script};
use huliho_imap_bridge::testing::{Mailboxes, PASSWORD, USER};

/// Room for a loopback exchange.
const STEP: Duration = Duration::from_secs(2);

/// The server over the model.
pub async fn start(mailboxes: Mailboxes) -> FakeImap {
    FakeImap::start(Script {
        mailboxes,
        ..Script::tls()
    })
    .await
}

/// A session past LOGIN.
pub async fn session(fake: &FakeImap) -> ImapSession {
    let target = fake.target(HOST, TlsMode::Implicit);
    let mut session = ImapSession::connect(fake.trusting(), &target, STEP)
        .await
        .unwrap();
    session.login(USER, PASSWORD).await.unwrap();
    session
}
