// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A connector for the tests: it signs a user in on a server with
//! LOGIN, refuses every connection as a server that is down does or
//! holds the account back as a host does for a stopped account. One that
//! names no submission server holds every send back; `Submitting` names
//! one.

use std::future::Future;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use tokio_rustls::rustls::ClientConfig;

use super::{PASSWORD, USER};
use crate::runtime::{ConnectError, Connector};
use crate::session::{ImapSession, Session, SessionError, Target};
use crate::smtp::{self, Submission};
use crate::store::AccountKey;
use crate::verify::Credential;

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
    /// The host holds the account back; nothing is tried.
    Holding,
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

    async fn connect(&self, _key: &AccountKey) -> Result<ImapSession, ConnectError> {
        match self {
            Self::Refusing => {
                Err(SessionError::Connect(io::Error::from(io::ErrorKind::ConnectionRefused)).into())
            }
            Self::Holding => Err(ConnectError::Held),
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

    fn submission(
        &self,
        _key: &AccountKey,
    ) -> impl Future<Output = Result<Submission, ConnectError>> + Send {
        std::future::ready(Err(ConnectError::Held))
    }
}

/// A connector that signs in on a submission server beside the sessions
/// of the one it wraps, as the user that one signs in or as the fixture
/// user where it names none.
#[derive(Clone)]
pub struct Submitting {
    pub inner: TestConnector,
    pub tls: Arc<ClientConfig>,
    pub smtp: Target,
    pub step: Duration,
}

impl Connector for Submitting {
    type Session = ImapSession;

    async fn connect(&self, key: &AccountKey) -> Result<ImapSession, ConnectError> {
        self.inner.connect(key).await
    }

    async fn submission(&self, _key: &AccountKey) -> Result<Submission, ConnectError> {
        let (username, password) = match &self.inner {
            TestConnector::Server { user, password, .. } => (user.clone(), password.clone()),
            TestConnector::Refusing | TestConnector::Holding => {
                (USER.to_owned(), PASSWORD.to_owned())
            }
        };
        let credential = Credential::Password { username, password };
        let tls = Arc::clone(&self.tls);
        Ok(smtp::submission(tls, &self.smtp, &credential, self.step).await?)
    }
}

/// A connector that counts how often it was asked for a session, so a
/// test can tell how many connections an account opened.
pub struct Counting {
    inner: TestConnector,
    connects: Arc<AtomicUsize>,
}

impl Counting {
    /// Counts the connects of `inner`; the count is shared with the
    /// caller.
    #[must_use]
    pub fn new(inner: TestConnector) -> (Self, Arc<AtomicUsize>) {
        let connects = Arc::new(AtomicUsize::new(0));
        (
            Self {
                inner,
                connects: Arc::clone(&connects),
            },
            connects,
        )
    }
}

impl Connector for Counting {
    type Session = ImapSession;

    async fn connect(&self, key: &AccountKey) -> Result<ImapSession, ConnectError> {
        self.connects.fetch_add(1, Ordering::SeqCst);
        self.inner.connect(key).await
    }

    async fn submission(&self, key: &AccountKey) -> Result<Submission, ConnectError> {
        self.inner.submission(key).await
    }
}
