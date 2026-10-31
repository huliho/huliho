// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The clocks of the runtime.

use std::time::Duration;

/// A `/changes` call refreshes its account at most this often; until
/// the server pushes, a client polls about twice as slowly.
pub const REFRESH_INTERVAL: Duration = Duration::from_secs(30);

/// A kept session is logged out once nothing used it for this long.
pub const SESSION_IDLE_CLOSE: Duration = Duration::from_mins(5);

/// What one turn on the conversation may take: a refresh, a preview
/// fetch, a body fetch, the writes of an `Email/set` or a `Mailbox/set`,
/// one window of a blob or one batch of the header sync. It also bounds
/// one UID FETCH answer, which has per-line timeouts alone.
pub const CONVERSATION_DEADLINE: Duration = Duration::from_secs(60);

/// What the searches of one round of a request may take together, from
/// the moment the first is asked, their wait for an earlier search of
/// the account included; past it a call answers `serverUnavailable`. It
/// stays inside one turn on the conversation, and nothing between the
/// bridge and the client bounds a search otherwise.
pub const SEARCH_DEADLINE: Duration = Duration::from_secs(45);

/// Gmail fixes the folder list of a connection when it signs in, so a
/// label made elsewhere reaches the tree only through a fresh one: a
/// mailbox pass of a Gmail account signs in afresh once its connection
/// is this old. The same as the idle close, so a busy account signs in
/// as often as one that rests between uses.
pub const GMAIL_LIST_AGE: Duration = SESSION_IDLE_CLOSE;

/// The pause before a fresh connection after a failure.
pub const RECONNECT_PAUSE: Duration = Duration::from_secs(2);

/// How long an account waits for its next round after a round ended in
/// failures or found the host holding the account back.
pub const RETRY_INTERVAL: Duration = Duration::from_mins(5);

/// The clocks of the runtime as one value, so a test can shorten them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    /// Between two refreshes of one account.
    pub interval: Duration,
    /// What one turn on the conversation may take.
    pub deadline: Duration,
    /// What the searches of one round of a request may take.
    pub search: Duration,
    /// How long a session stays open unused.
    pub idle_close: Duration,
    /// How old a Gmail conversation may be when a mailbox pass runs on
    /// it.
    pub list_age: Duration,
    /// How long a task waits after a round that did not finish.
    pub retry_interval: Duration,
    /// The pause before connecting again after a failure.
    pub reconnect_pause: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            interval: REFRESH_INTERVAL,
            deadline: CONVERSATION_DEADLINE,
            search: SEARCH_DEADLINE,
            idle_close: SESSION_IDLE_CLOSE,
            list_age: GMAIL_LIST_AGE,
            retry_interval: RETRY_INTERVAL,
            reconnect_pause: RECONNECT_PAUSE,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clocks_default_to_the_documented_values() {
        let timing = Timing::default();
        assert_eq!(timing.interval, Duration::from_secs(30));
        assert_eq!(timing.deadline, Duration::from_secs(60));
        assert_eq!(timing.search, Duration::from_secs(45));
        assert_eq!(timing.idle_close, Duration::from_secs(300));
        assert_eq!(timing.list_age, Duration::from_secs(300));
        assert_eq!(timing.retry_interval, Duration::from_secs(300));
        assert_eq!(timing.reconnect_pause, Duration::from_secs(2));
    }
}
