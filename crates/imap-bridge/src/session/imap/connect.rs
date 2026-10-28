// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The way to a greeting: TCP to the first address that answers, the
//! TLS handshake and the greeting itself.

use std::io;
use std::net::SocketAddr;
use std::time::Duration;

use async_imap::Client;
use async_imap::imap_proto::{Response, Status};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;
use tokio_rustls::rustls::pki_types::ServerName;

use super::Wire;
use crate::session::{SessionError, io_error};

/// The addresses in order; the last failure is the answer when none
/// accepts.
pub(super) async fn connect_tcp(
    addresses: &[SocketAddr],
    step: Duration,
) -> Result<TcpStream, SessionError> {
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

pub(super) async fn handshake(
    connector: &TlsConnector,
    server_name: ServerName<'static>,
    tcp: TcpStream,
    step: Duration,
) -> Result<TlsStream<TcpStream>, SessionError> {
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
pub(super) async fn greeting<T: Wire>(
    client: &mut Client<T>,
    step: Duration,
) -> Result<(), SessionError> {
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
