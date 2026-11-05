// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Where the sends of one account come from: a connection of their own
//! per message, opened through the host and closed with QUIT, one send
//! of the account at a time.

use tokio::sync::{Mutex, MutexGuard};

use super::{ConnectError, Connector};
use crate::smtp::Submission;
use crate::store::AccountKey;

/// The sends of one account.
pub struct Sends<C: Connector> {
    connector: C,
    /// Held for the whole of one send, so the sends of an account run in
    /// the order they came. It is taken before the account's conversation
    /// and never while that is held.
    one: Mutex<()>,
}

impl<C: Connector> Sends<C> {
    pub(crate) fn new(connector: C) -> Self {
        Self {
            connector,
            one: Mutex::new(()),
        }
    }

    /// The account's turn to send; the next send waits until it is
    /// dropped.
    pub(crate) async fn turn(&self) -> MutexGuard<'_, ()> {
        self.one.lock().await
    }

    /// A connection signed in for one send.
    ///
    /// # Errors
    ///
    /// As [`Connector::submission`].
    pub(crate) async fn connect(&self, key: &AccountKey) -> Result<Submission, ConnectError> {
        self.connector.submission(key).await
    }
}
