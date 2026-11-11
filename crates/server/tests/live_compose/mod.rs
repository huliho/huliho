// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The compose steps of the suite: the draft cycle on Cyrus natively
//! and over IMAPS and on both Dovecots, and the round trip between Cyrus
//! and Dovecot through the compose Postfix.

use crate::live_rig::{Target, drafts, sending};
use crate::{CYRUS, DOVECOT, PLAIN};

#[tokio::test]
async fn cyrus_runs_the_draft_cycle_through_the_proxy_rfc8621_4_6() {
    let _cyrus = CYRUS.lock().await;
    drafts::cycle(Target::Cyrus).await;
}

#[tokio::test]
async fn cyrus_runs_the_draft_cycle_through_the_bridge_over_imaps_rfc8621_4_6() {
    let _cyrus = CYRUS.lock().await;
    drafts::cycle(Target::CyrusBridge).await;
}

#[tokio::test]
async fn dovecot_runs_the_draft_cycle_through_the_bridge_rfc8621_4_6() {
    let _dovecot = DOVECOT.lock().await;
    drafts::cycle(Target::Dovecot).await;
}

#[tokio::test]
async fn dovecot_without_an_index_runs_the_draft_cycle_through_the_bridge_rfc8621_4_6() {
    let _plain = PLAIN.lock().await;
    drafts::cycle(Target::DovecotPlain).await;
}

/// The trip takes Cyrus's turn; its Dovecot user is its own.
#[tokio::test]
async fn cyrus_and_dovecot_trade_a_message_and_its_reply_through_postfix_rfc8621_7() {
    let _cyrus = CYRUS.lock().await;
    sending::round_trip().await;
}

#[tokio::test]
async fn cyrus_over_imaps_sends_to_dovecot_through_its_submission_rfc8621_7() {
    let _cyrus = CYRUS.lock().await;
    sending::over_imaps().await;
}
