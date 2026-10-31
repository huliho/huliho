// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What a folder command that failed comes to: how its object ends,
//! whether the session stands and what the work does afterwards.

use super::Refusal;
use crate::session::SessionError;

/// Why a step ended early: how its object ends and whether the session
/// still stands.
pub(super) struct Stop {
    pub refusal: Refusal,
    pub stands: bool,
}

/// What a failed command comes to. A NO is the server's refusal and
/// keeps the session, an answer that cannot be read is its failure and
/// costs it, anything else leaves the server unreached.
pub(super) fn failure(error: &SessionError) -> Stop {
    let (refusal, stands) = match error {
        SessionError::Refused => (Refusal::ServerFail, true),
        SessionError::Protocol(_) => (Refusal::ServerFail, false),
        _ => (Refusal::ServerUnavailable, false),
    };
    Stop { refusal, stands }
}

/// A command that tidies up after another: a NO leaves things as they
/// are, any other failure ends the session.
pub(super) fn kept(sent: Result<(), SessionError>) -> Result<(), Stop> {
    match sent {
        Ok(()) | Err(SessionError::Refused) => Ok(()),
        Err(error) => Err(failure(&error)),
    }
}

/// The folder commands the server refused or could not read, by what
/// they ask afterwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Refused {
    Nothing,
    /// A command: the next use signs in afresh.
    Command,
    /// A RENAME or a DELETE among them: a pass shows what stands.
    Move,
}
