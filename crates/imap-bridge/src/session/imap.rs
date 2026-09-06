// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The session on async-imap over tokio-rustls.

use std::fmt;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use async_imap::error::Error as ImapError;
use async_imap::imap_proto::{Capability, Response, Status};
use async_imap::types::UnsolicitedResponse;
use async_imap::{Authenticator, Client, Connection};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::time::{error::Elapsed, timeout};
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;
use tokio_rustls::rustls::ClientConfig;
use tokio_rustls::rustls::pki_types::ServerName;

use super::{Capabilities, Session, SessionError, Target, TlsMode};

type Stream = TlsStream<TcpStream>;
type Attempt = Result<async_imap::Session<Stream>, (ImapError, Client<Stream>)>;

/// The untagged lines one CAPABILITY answer may carry before its tagged
/// OK; one is the norm, the rest is room for alerts.
const CAPABILITY_LINES: usize = 8;

/// The stream types the client library accepts.
trait Wire: AsyncRead + AsyncWrite + Unpin + Send + fmt::Debug {}

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
        let client = match target.tls {
            TlsMode::Implicit => {
                let stream = handshake(&connector, server_name, tcp, step_timeout).await?;
                let mut client = Client::new(stream);
                greeting(&mut client, step_timeout).await?;
                client
            }
            TlsMode::Starttls => {
                let mut plain = Client::new(tcp);
                greeting(&mut plain, step_timeout).await?;
                if !capabilities_of(&mut plain, step_timeout)
                    .await?
                    .has("STARTTLS")
                {
                    return Err(SessionError::StarttlsAbsent);
                }
                match run(&mut plain, "STARTTLS", step_timeout).await {
                    Ok(()) => {}
                    Err(SessionError::Protocol(_)) => return Err(SessionError::StarttlsRefused),
                    Err(other) => return Err(other),
                }
                let stream =
                    handshake(&connector, server_name, plain.into_inner(), step_timeout).await?;
                Client::new(stream)
            }
        };
        let mut client = client;
        let offered = capabilities_of(&mut client, step_timeout).await?;
        Ok(Self {
            state: State::Fresh(client),
            step: step_timeout,
            offered,
        })
    }

    async fn login(&mut self, username: &str, password: &str) -> Result<(), SessionError> {
        // RFC 9051 section 7.2.2: no LOGIN while LOGINDISABLED is advertised.
        if self.offered.has("LOGINDISABLED") {
            return Err(SessionError::Protocol("LOGIN is disabled"));
        }
        let client = self.take_fresh()?;
        let attempt = timeout(self.step, client.login(username, password)).await;
        self.signed_in(attempt)
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
        self.signed_in(attempt)
    }

    async fn capabilities(&mut self) -> Result<Capabilities, SessionError> {
        let step = self.step;
        match &mut self.state {
            State::Fresh(client) => capabilities_of(client, step).await,
            State::SignedIn(session) => capabilities_of(session, step).await,
            State::Gone => Err(SessionError::Closed),
        }
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

    fn signed_in(&mut self, attempt: Result<Attempt, Elapsed>) -> Result<(), SessionError> {
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

/// The SASL XOAUTH2 exchange: the identity once, then an empty line so
/// the server's error challenge ends in its NO.
struct Xoauth2 {
    initial: Option<String>,
}

impl Authenticator for Xoauth2 {
    type Response = String;

    fn process(&mut self, _challenge: &[u8]) -> String {
        self.initial.take().unwrap_or_default()
    }
}

/// The addresses in order; the last failure is the answer when none
/// accepts.
async fn connect_tcp(addresses: &[SocketAddr], step: Duration) -> Result<TcpStream, SessionError> {
    let mut failure = SessionError::NoAddress;
    for address in addresses {
        match timeout(step, TcpStream::connect(address)).await {
            Ok(Ok(stream)) => return Ok(stream),
            Ok(Err(error)) => failure = SessionError::Connect(error),
            Err(_elapsed) => failure = SessionError::Timeout,
        }
    }
    Err(failure)
}

async fn handshake(
    connector: &TlsConnector,
    server_name: ServerName<'static>,
    tcp: TcpStream,
    step: Duration,
) -> Result<Stream, SessionError> {
    // tokio-rustls reports what rustls refused as invalid data; any other
    // kind is the transport giving up.
    timeout(step, connector.connect(server_name, tcp))
        .await
        .map_err(|_elapsed| SessionError::Timeout)?
        .map_err(|error| match error.kind() {
            io::ErrorKind::InvalidData => SessionError::Tls(error),
            io::ErrorKind::UnexpectedEof => SessionError::Closed,
            _ => SessionError::Io(error),
        })
}

/// Reads the greeting; only an OK leads anywhere (RFC 9051 section 7.1).
async fn greeting<T: Wire>(client: &mut Client<T>, step: Duration) -> Result<(), SessionError> {
    let response = timeout(step, client.read_response())
        .await
        .map_err(|_elapsed| SessionError::Timeout)?
        .map_err(io_error)?
        .ok_or(SessionError::Closed)?;
    match response.parsed() {
        Response::Data {
            status: Status::Ok, ..
        } => Ok(()),
        Response::Data {
            status: Status::Bye,
            ..
        } => Err(SessionError::Closed),
        Response::Data {
            status: Status::PreAuth,
            ..
        } => Err(SessionError::Protocol("the greeting is PREAUTH")),
        _ => Err(SessionError::Protocol("the greeting is not an OK")),
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

/// CAPABILITY in any state; the untagged data arrives on the channel the
/// client hands responses it did not ask for.
async fn capabilities_of<T: Wire>(
    connection: &mut Connection<T>,
    step: Duration,
) -> Result<Capabilities, SessionError> {
    let (sender, receiver) = async_channel::bounded(CAPABILITY_LINES);
    timeout(
        step,
        connection.run_command_and_check_ok("CAPABILITY", Some(sender)),
    )
    .await
    .map_err(|_elapsed| SessionError::Timeout)?
    .map_err(command_error)?;
    let mut names = Vec::new();
    while let Ok(message) = receiver.try_recv() {
        if let UnsolicitedResponse::Other(data) = message
            && let Response::Capabilities(found) = data.parsed()
        {
            names.extend(found.iter().map(capability_name));
        }
    }
    if names.is_empty() {
        return Err(SessionError::Protocol("CAPABILITY answered without data"));
    }
    Ok(names.into_iter().collect())
}

fn capability_name(capability: &Capability<'_>) -> String {
    match capability {
        Capability::Imap4rev1 => "IMAP4rev1".to_owned(),
        Capability::Auth(mechanism) => format!("AUTH={mechanism}"),
        Capability::Atom(atom) => atom.to_string(),
    }
}

/// A NO answers the credential; a BAD is about the command.
fn auth_error(error: ImapError) -> SessionError {
    match error {
        ImapError::No(_) | ImapError::Validate(_) => SessionError::CredentialRejected,
        ImapError::Bad(_) => SessionError::Protocol("the sign-in command was not accepted"),
        other => command_error(other),
    }
}

/// The server's own words never travel: a server that has just seen a
/// credential could echo it.
fn command_error(error: ImapError) -> SessionError {
    match error {
        ImapError::Io(error) => io_error(error),
        ImapError::ConnectionLost => SessionError::Closed,
        ImapError::No(_) => SessionError::Protocol("the server answered NO"),
        ImapError::Bad(_) => SessionError::Protocol("the server answered BAD"),
        ImapError::Parse(_) => SessionError::Protocol("the answer could not be parsed"),
        _ => SessionError::Protocol("the command was not accepted"),
    }
}

/// The client library reports bytes it cannot parse under the kind
/// `Other` and a connection that ends mid-response as an unexpected end.
fn io_error(error: io::Error) -> SessionError {
    match error.kind() {
        io::ErrorKind::Other => SessionError::Protocol("the answer could not be parsed"),
        io::ErrorKind::UnexpectedEof => SessionError::Closed,
        _ => SessionError::Io(error),
    }
}
