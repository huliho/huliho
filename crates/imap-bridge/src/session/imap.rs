// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The session on async-imap over tokio-rustls.

mod auth;
mod connect;

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use async_imap::error::Error as ImapError;
use async_imap::{Client, Connection};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::time::{error::Elapsed, timeout};
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;
use tokio_rustls::rustls::ClientConfig;
use tokio_rustls::rustls::pki_types::ServerName;

use self::auth::{Xoauth2, auth_error};
use self::connect::{connect_tcp, greeting, handshake};
use super::capability::{Tags, capabilities_of};
use super::guard::Guarded;
use super::{
    Capabilities, FetchItems, FetchedMessage, FlagFetch, Flagged, ListReturn, Listing, MailboxName,
    MoveAsk, Moved, PartAsk, PreviewAsk, PreviewBytes, QuotaRoot, Rights, SearchAsk, Selected,
    Session, SessionError, StatusEntry, StatusItems, StoreAsk, Structure, Target, TlsMode,
    UidRange, Writable, io_error, mailbox, quota, read, write,
};

pub(super) type Stream = Guarded<TlsStream<TcpStream>>;
type Attempt = Result<async_imap::Session<Stream>, (ImapError, Client<Stream>)>;

/// The stream types the client library accepts.
pub(super) trait Wire: AsyncRead + AsyncWrite + Unpin + Send + fmt::Debug {}

impl<T: AsyncRead + AsyncWrite + Unpin + Send + fmt::Debug> Wire for T {}

enum State {
    Fresh(Client<Stream>),
    SignedIn(async_imap::Session<Stream>),
    Gone,
}

/// The one implementation of [`Session`].
pub struct ImapSession {
    state: State,
    step: Duration,
    /// What the server advertised over TLS before any sign-in.
    offered: Capabilities,
    tags: Tags,
    /// The message count of the selected mailbox; while one is selected
    /// other clients' lines ride along.
    selected: Option<read::Count>,
    /// A SELECT opened the selected mailbox without `[READ-ONLY]`.
    writable: bool,
}

impl Session for ImapSession {
    async fn connect(
        tls: Arc<ClientConfig>,
        target: &Target,
        step_timeout: Duration,
    ) -> Result<Self, SessionError> {
        let server_name =
            ServerName::try_from(target.host.clone()).map_err(SessionError::ServerName)?;
        let connector = TlsConnector::from(tls);
        let tcp = connect_tcp(&target.addresses, step_timeout).await?;
        let mut tags = Tags::default();
        let client = match target.tls {
            TlsMode::Implicit => {
                let stream = handshake(&connector, server_name, tcp, step_timeout).await?;
                let mut client = Client::new(Guarded::new(stream));
                greeting(&mut client, step_timeout).await?;
                client
            }
            TlsMode::Starttls => {
                let mut plain = Client::new(Guarded::new(tcp));
                greeting(&mut plain, step_timeout).await?;
                let offered = capabilities_of(&mut plain, &mut tags, step_timeout, None).await?;
                if !offered.has("STARTTLS") {
                    return Err(SessionError::StarttlsAbsent);
                }
                match run(&mut plain, "STARTTLS", step_timeout).await {
                    Ok(()) => {}
                    Err(SessionError::Refused | SessionError::Protocol(_)) => {
                        return Err(SessionError::StarttlsRefused);
                    }
                    Err(other) => return Err(other),
                }
                let tcp = plain.into_inner().into_inner();
                let stream = handshake(&connector, server_name, tcp, step_timeout).await?;
                Client::new(Guarded::new(stream))
            }
        };
        let mut client = client;
        let offered = capabilities_of(&mut client, &mut tags, step_timeout, None).await?;
        Ok(Self {
            state: State::Fresh(client),
            step: step_timeout,
            offered,
            tags,
            selected: None,
            writable: false,
        })
    }

    async fn login(&mut self, username: &str, password: &str) -> Result<(), SessionError> {
        // RFC 9051 section 7.2.2: no LOGIN while LOGINDISABLED is advertised.
        if self.offered.has("LOGINDISABLED") {
            return Err(SessionError::Protocol("LOGIN is disabled"));
        }
        let client = self.take_fresh()?;
        let attempt = timeout(self.step, client.login(username, password)).await;
        self.finish_sign_in(attempt)
    }

    async fn authenticate_xoauth2(
        &mut self,
        username: &str,
        token: &str,
    ) -> Result<(), SessionError> {
        if !self.offered.has("AUTH=XOAUTH2") {
            return Err(SessionError::Protocol("XOAUTH2 is not offered"));
        }
        let client = self.take_fresh()?;
        let authenticator = Xoauth2 {
            initial: Some(format!("user={username}\x01auth=Bearer {token}\x01\x01")),
        };
        let attempt = timeout(self.step, client.authenticate("XOAUTH2", authenticator)).await;
        self.finish_sign_in(attempt)
    }

    async fn capabilities(&mut self) -> Result<Capabilities, SessionError> {
        let step = self.step;
        match &mut self.state {
            State::Fresh(client) => capabilities_of(client, &mut self.tags, step, None).await,
            State::SignedIn(session) => {
                let count = self.selected.as_mut();
                capabilities_of(session, &mut self.tags, step, count).await
            }
            State::Gone => Err(SessionError::Closed),
        }
    }

    async fn list(&mut self, options: ListReturn) -> Result<Listing, SessionError> {
        let room = self.room();
        read::list(&mut self.selection()?, options, room).await
    }

    async fn lsub(&mut self) -> Result<Vec<String>, SessionError> {
        let room = self.room();
        read::lsub(&mut self.selection()?, room).await
    }

    async fn status(
        &mut self,
        mailbox: &str,
        items: StatusItems,
    ) -> Result<StatusEntry, SessionError> {
        let room = self.room();
        read::status(&mut self.selection()?, mailbox, items, room).await
    }

    async fn create(&mut self, name: &MailboxName) -> Result<(), SessionError> {
        let room = self.room();
        mailbox::create(&mut self.selection()?, name, room).await
    }

    async fn rename(&mut self, from: &str, to: &MailboxName) -> Result<(), SessionError> {
        let room = self.room();
        mailbox::rename(&mut self.selection()?, (from, to), room).await
    }

    async fn delete(&mut self, mailbox: &str) -> Result<(), SessionError> {
        let room = self.room();
        mailbox::named(&mut self.selection()?, "DELETE", mailbox, room).await
    }

    async fn subscribe(&mut self, mailbox: &str) -> Result<(), SessionError> {
        let room = self.room();
        mailbox::named(&mut self.selection()?, "SUBSCRIBE", mailbox, room).await
    }

    async fn unsubscribe(&mut self, mailbox: &str) -> Result<(), SessionError> {
        let room = self.room();
        mailbox::named(&mut self.selection()?, "UNSUBSCRIBE", mailbox, room).await
    }

    async fn my_rights(&mut self, mailbox: &str) -> Result<Rights, SessionError> {
        let room = self.room();
        mailbox::my_rights(&mut self.selection()?, mailbox, room).await
    }

    async fn examine(&mut self, mailbox: &str) -> Result<Selected, SessionError> {
        let opened = self.open(read::Access::ReadOnly, mailbox).await;
        self.opened(opened.and_then(|opened| opened.selected()))
    }

    async fn select(&mut self, mailbox: &str) -> Result<Writable, SessionError> {
        let opened = self.open(read::Access::ReadWrite, mailbox).await;
        let writable = self.opened(opened.and_then(read::Opened::writable))?;
        self.writable = !writable.read_only;
        Ok(writable)
    }

    async fn uid_store(&mut self, ask: &StoreAsk<'_>) -> Result<(), SessionError> {
        let room = self.room();
        write::uid_store(&mut self.selection()?, ask, room).await
    }

    async fn uid_move(&mut self, ask: &MoveAsk<'_>) -> Result<Option<Moved>, SessionError> {
        let room = self.room();
        write::uid_move(&mut self.selection()?, ask, room).await
    }

    async fn uid_copy(&mut self, ask: &MoveAsk<'_>) -> Result<Option<Moved>, SessionError> {
        let room = self.room();
        write::uid_copy(&mut self.selection()?, ask, room).await
    }

    async fn uid_expunge(&mut self, uids: &[u32]) -> Result<(), SessionError> {
        let room = self.room();
        write::uid_expunge(&mut self.selection()?, uids, room).await
    }

    async fn quota_root(&mut self) -> Result<Vec<QuotaRoot>, SessionError> {
        let room = self.room();
        quota::quota_root(&mut self.selection()?, room).await
    }

    async fn uid_list(&mut self) -> Result<Vec<u32>, SessionError> {
        let room = self.room();
        read::uid_list(&mut self.selection()?, room).await
    }

    async fn uid_search(&mut self, ask: &SearchAsk<'_>) -> Result<Vec<u32>, SessionError> {
        let room = self.room();
        read::uid_search(&mut self.selection()?, ask, room).await
    }

    async fn uid_fetch(
        &mut self,
        range: UidRange,
        items: FetchItems,
    ) -> Result<Vec<FetchedMessage>, SessionError> {
        let room = self.room();
        read::uid_fetch(&mut self.selection()?, range, items, room).await
    }

    async fn uid_flags(
        &mut self,
        fetch: FlagFetch,
        labels: bool,
    ) -> Result<Vec<Flagged>, SessionError> {
        let room = self.room();
        read::uid_flags(&mut self.selection()?, (fetch, labels), room).await
    }

    async fn uid_previews(
        &mut self,
        ask: &PreviewAsk<'_>,
    ) -> Result<Vec<PreviewBytes>, SessionError> {
        let room = self.room();
        read::uid_previews(&mut self.selection()?, ask, room).await
    }

    async fn uid_structure(
        &mut self,
        uid: u32,
        fields: &[String],
    ) -> Result<Option<Structure>, SessionError> {
        let room = self.room();
        read::uid_structure(&mut self.selection()?, uid, fields, room).await
    }

    async fn uid_header_fields(
        &mut self,
        uid: u32,
        fields: &[String],
    ) -> Result<Option<Vec<u8>>, SessionError> {
        let room = self.room();
        read::uid_header_fields(&mut self.selection()?, uid, fields, room).await
    }

    async fn uid_part(&mut self, ask: &PartAsk<'_>) -> Result<Option<Vec<u8>>, SessionError> {
        let room = self.room();
        read::uid_part(&mut self.selection()?, ask, room).await
    }

    async fn noop(&mut self) -> Result<(), SessionError> {
        let room = self.room();
        read::noop(&mut self.selection()?, room).await
    }

    async fn logout(self) -> Result<(), SessionError> {
        match self.state {
            State::Fresh(mut client) => run(&mut client, "LOGOUT", self.step).await,
            State::SignedIn(mut session) => run(&mut session, "LOGOUT", self.step).await,
            State::Gone => Ok(()),
        }
    }
}

impl ImapSession {
    /// Takes the client out for a sign-in; a timeout drops it with the
    /// future, so the state stays gone in that case.
    fn take_fresh(&mut self) -> Result<Client<Stream>, SessionError> {
        match std::mem::replace(&mut self.state, State::Gone) {
            State::Fresh(client) => Ok(client),
            State::SignedIn(session) => {
                self.state = State::SignedIn(session);
                Err(SessionError::Protocol("already signed in"))
            }
            State::Gone => Err(SessionError::Closed),
        }
    }

    fn room(&self) -> read::Room {
        read::Room::new(self.step, self.selected.is_some())
    }

    /// EXAMINE or SELECT. A server may select before it fails, so the
    /// room grows first; no write goes out until a SELECT succeeds.
    async fn open(
        &mut self,
        access: read::Access,
        mailbox: &str,
    ) -> Result<read::Opened, SessionError> {
        self.selected = Some(read::Count::default());
        self.writable = false;
        let room = self.room();
        read::open(&mut self.selection()?, access, mailbox, room).await
    }

    /// What an open came to; after a failure no mailbox is selected
    /// (RFC 3501 section 6.3.1).
    fn opened<T>(&mut self, outcome: Result<T, SessionError>) -> Result<T, SessionError> {
        if outcome.is_err() {
            self.selected = None;
        }
        outcome
    }

    /// The signed-in session the read commands run on, with the count
    /// of its selected mailbox.
    fn selection(&mut self) -> Result<read::Selection<'_>, SessionError> {
        match &mut self.state {
            State::SignedIn(session) => Ok(read::Selection {
                session,
                count: self.selected.as_mut(),
                writable: self.writable,
            }),
            State::Fresh(_) => Err(SessionError::Protocol("not signed in")),
            State::Gone => Err(SessionError::Closed),
        }
    }

    fn finish_sign_in(&mut self, attempt: Result<Attempt, Elapsed>) -> Result<(), SessionError> {
        match attempt {
            Ok(Ok(session)) => {
                self.state = State::SignedIn(session);
                Ok(())
            }
            Ok(Err((error, client))) => {
                self.state = State::Fresh(client);
                Err(auth_error(error))
            }
            Err(_elapsed) => Err(SessionError::Timeout),
        }
    }
}

/// One command, its tagged answer checked for OK.
async fn run<T: Wire>(
    connection: &mut Connection<T>,
    command: &str,
    step: Duration,
) -> Result<(), SessionError> {
    timeout(step, connection.run_command_and_check_ok(command, None))
        .await
        .map_err(|_elapsed| SessionError::Timeout)?
        .map_err(command_error)
}

/// The server's own words never travel: a server that has just seen a
/// credential could echo it.
pub(super) fn command_error(error: ImapError) -> SessionError {
    match error {
        ImapError::Io(error) => io_error(error),
        ImapError::ConnectionLost => SessionError::Closed,
        ImapError::No(_) => SessionError::Refused,
        ImapError::Bad(_) => SessionError::Protocol("the server answered BAD"),
        ImapError::Parse(_) => SessionError::Protocol("the answer could not be parsed"),
        _ => SessionError::Protocol("the command was not accepted"),
    }
}
