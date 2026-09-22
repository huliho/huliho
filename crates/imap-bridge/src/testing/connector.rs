// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A connector for the tests: it signs a user in on a server with
//! LOGIN, or refuses every connection as a server that is down does.

use std::io;
use std::sync::Arc;
use std::time::Duration;

use tokio_rustls::rustls::ClientConfig;

use super::{PASSWORD, USER};
use crate::runtime::Connector;
use crate::session::{ImapSession, Session, SessionError, Target};
use crate::store::AccountKey;

/// Where the connector connects and how it signs in.
#[derive(Clone)]
pub enum TestConnector {
    /// A server over TLS from the first byte, a user signed in with
    /// LOGIN.
    Server {
        tls: Arc<ClientConfig>,
        target: Target,
        step: Duration,
        user: String,
        password: String,
    },
    /// Every connect is refused.
    Refusing,
}

impl TestConnector {
    /// The scripted server with the fixture user.
    #[must_use]
    pub fn scripted(tls: Arc<ClientConfig>, target: Target, step: Duration) -> Self {
        Self::Server {
            tls,
            target,
            step,
            user: USER.to_owned(),
            password: PASSWORD.to_owned(),
        }
    }
}

impl Connector for TestConnector {
    type Session = ImapSession;

    async fn connect(&self, _key: &AccountKey) -> Result<ImapSession, SessionError> {
        match self {
            Self::Refusing => Err(SessionError::Connect(io::Error::from(
                io::ErrorKind::ConnectionRefused,
            ))),
            Self::Server {
                tls,
                target,
                step,
                user,
                password,
            } => {
                let mut session = ImapSession::connect(Arc::clone(tls), target, *step).await?;
                session.login(user, password).await?;
                Ok(session)
            }
        }
    }
}
