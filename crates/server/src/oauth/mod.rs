// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Signing a mail account in through a provider: the consent a user
//! starts, the outcome the card polls and the tokens that come out of it.

mod client;
mod consents;
mod refresh;

pub use client::{Grant, PKCE_MIN_LENGTH, Started, TokenError, Tokens, authorization, exchange};
pub use consents::{Claimant, Claimed, Consents, DeniedCause, NewConsent, Outcome};
pub use refresh::{RefreshError, access_token};
