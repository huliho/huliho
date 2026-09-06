// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Scripted IMAP and SMTP servers for tests: a fresh certificate per
//! server, one answer per command and a record of every line received.

pub mod imap;
pub mod smtp;

use std::future::Future;
use std::marker::PhantomData;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, PoisonError};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use rcgen::{BasicConstraints, CertificateParams, CertifiedIssuer, IsCa, KeyPair};
use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader, ReadHalf,
    WriteHalf,
};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio_rustls::rustls::{ClientConfig, RootCertStore, ServerConfig};

use crate::session::{Target, TlsMode};
use crate::verify::Credential;

pub use imap::FakeImap;
pub use smtp::FakeSmtp;

/// The user every script signs in.
pub const USER: &str = "sanne";
/// The password every script accepts.
pub const PASSWORD: &str = "correct horse";
/// The OAuth token every script accepts.
pub const TOKEN: &str = "ya29.token";

/// The challenge Google sends on a wrong token, before its refusal.
const GOOGLE_ERROR: &str =
    r#"{"status":"401","schemes":"bearer","scope":"https://mail.google.com/"}"#;

/// The fixture user with the given password, as the checks take it.
#[must_use]
pub fn password(password: &str) -> Credential {
    Credential::Password {
        username: USER.to_owned(),
        password: password.to_owned(),
    }
}

/// The fixture user with the given token.
#[must_use]
pub fn token(token: &str) -> Credential {
    Credential::Xoauth2 {
        username: USER.to_owned(),
        token: token.to_owned(),
    }
}

/// Whether a plaintext listener offers STARTTLS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Starttls {
    Offered,
    Absent,
    Refused,
}

/// What a script listens with: TLS from the first byte or plaintext.
#[derive(Debug, Clone, Copy)]
pub enum Listen {
    Tls,
    Plain(Starttls),
}

/// What a script says first.
#[derive(Debug, Clone, Copy)]
pub enum Greeting {
    Ok,
    Bye,
    Garbage,
    Silence,
}

/// Where a conversation stands; after STARTTLS no greeting is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Plain(Starttls),
    Tls,
    Upgraded,
}

impl Phase {
    /// The prefix of every line recorded in this phase.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Plain(_) => "plain",
            Self::Tls | Self::Upgraded => "tls",
        }
    }

    /// Whether the connection is encrypted.
    #[must_use]
    pub fn is_tls(self) -> bool {
        !matches!(self, Self::Plain(_))
    }
}

/// Every line a server received, prefixed with `plain` or `tls`; SASL
/// data the client sent appears decoded after `SASL`.
pub type Lines = Arc<Mutex<Vec<String>>>;

/// Records one received line under the phase it arrived in.
pub fn record(lines: &Lines, phase: Phase, line: &str) {
    lines
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(format!("{} {line}", phase.label()));
}

/// Decodes the SASL data the client sent and records it.
#[must_use]
pub fn decoded(lines: &Lines, phase: Phase, encoded: &str) -> String {
    let text = BASE64.decode(encoded.trim()).map_or_else(
        |_| format!("{encoded:?}"),
        |bytes| String::from_utf8_lossy(&bytes).into_owned(),
    );
    record(lines, phase, &format!("SASL {text}"));
    text
}

/// One line without its line ending; `None` when the client left.
pub async fn read_line<R: AsyncBufRead + Unpin>(reader: &mut R) -> Option<String> {
    let mut line = String::new();
    if reader.read_line(&mut line).await.ok()? == 0 {
        return None;
    }
    Some(line.trim_end_matches(['\r', '\n']).to_owned())
}

/// What a scripted server says: the listener it runs and the
/// conversation it holds.
pub trait Protocol: Copy + Send + Sync + 'static {
    /// The name the certificate carries.
    const HOST: &'static str;

    /// TLS from the first byte or plaintext with or without STARTTLS.
    fn listen(self) -> Listen;

    /// What the server says first.
    fn greeting(self) -> Greeting;

    /// Whether commands after the greeting get an answer.
    fn answers(self) -> bool;

    /// The greeting line for a kind that says something.
    fn words(greeting: Greeting) -> &'static str;

    /// Answers commands until the client leaves; hands the stream back
    /// only when STARTTLS was accepted, so the caller can upgrade it.
    fn converse<S: AsyncRead + AsyncWrite + Unpin + Send>(
        self,
        phase: Phase,
        lines: &Lines,
        stream: S,
    ) -> impl Future<Output = Option<S>> + Send;
}

/// Splits the stream and opens the conversation: the greeting unless
/// the connection was just upgraded, then a hold when the script never
/// answers. `None` when the greeting ends the conversation.
pub(crate) async fn open<P: Protocol, S: AsyncRead + AsyncWrite>(
    script: P,
    phase: Phase,
    stream: S,
) -> Option<(BufReader<ReadHalf<S>>, WriteHalf<S>)> {
    let (reader, mut writer) = tokio::io::split(stream);
    if phase != Phase::Upgraded {
        let greeting = script.greeting();
        if matches!(greeting, Greeting::Silence) {
            std::future::pending::<()>().await;
        }
        writer.write_all(P::words(greeting).as_bytes()).await.ok()?;
        if !matches!(greeting, Greeting::Ok) {
            return None;
        }
    }
    if !script.answers() {
        std::future::pending::<()>().await;
    }
    Some((BufReader::new(reader), writer))
}

/// A server on a loopback port that follows one script.
pub struct Fake<P> {
    /// Where the server listens.
    pub address: SocketAddr,
    ca: CertificateDer<'static>,
    ca_pem: String,
    lines: Lines,
    script: PhantomData<P>,
}

impl<P: Protocol> Fake<P> {
    /// Listens on a loopback port and serves every connection by the
    /// script.
    ///
    /// # Panics
    ///
    /// When no loopback port can be bound.
    pub async fn start(script: P) -> Self {
        let certificates = certificates(P::HOST);
        let acceptor =
            TlsAcceptor::from(Arc::new(server_config(certificates.leaf, certificates.key)));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback port is free");
        let address = listener
            .local_addr()
            .expect("a bound listener has an address");
        let lines = Lines::default();
        let recorded = Arc::clone(&lines);
        tokio::spawn(async move {
            while let Ok((tcp, _)) = listener.accept().await {
                tokio::spawn(serve(script, acceptor.clone(), Arc::clone(&recorded), tcp));
            }
        });
        Self {
            address,
            ca: certificates.ca,
            ca_pem: certificates.ca_pem,
            lines,
            script: PhantomData,
        }
    }

    /// A client that trusts this server's CA and nothing else.
    ///
    /// # Panics
    ///
    /// When the CA cannot enter a root store; the ones made here always
    /// can.
    #[must_use]
    pub fn trusting(&self) -> Arc<ClientConfig> {
        client_config(self.ca.clone())
    }

    /// A client that trusts an unrelated CA only.
    ///
    /// # Panics
    ///
    /// When the CA cannot enter a root store; the ones made here always
    /// can.
    #[must_use]
    pub fn distrusting() -> Arc<ClientConfig> {
        client_config(certificates(P::HOST).ca)
    }

    /// The CA as PEM, for a trust store that reads files.
    #[must_use]
    pub fn ca_pem(&self) -> &str {
        &self.ca_pem
    }

    /// This server as a target under `host`.
    #[must_use]
    pub fn target(&self, host: &str, tls: TlsMode) -> Target {
        Target {
            host: host.to_owned(),
            addresses: vec![self.address],
            tls,
        }
    }

    /// Every line received so far.
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        self.lines
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

struct Certificates {
    ca: CertificateDer<'static>,
    ca_pem: String,
    leaf: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
}

/// A fresh CA and a certificate for `host` signed by it.
fn certificates(host: &str) -> Certificates {
    let ca_key = KeyPair::generate().expect("a key pair generates");
    let mut ca_params =
        CertificateParams::new(Vec::<String>::new()).expect("no names is a valid subject");
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let ca = CertifiedIssuer::self_signed(ca_params, ca_key).expect("a CA signs itself");
    let key = KeyPair::generate().expect("a key pair generates");
    let leaf = CertificateParams::new(vec![host.to_owned()])
        .expect("a host name is a valid subject")
        .signed_by(&key, &ca)
        .expect("the CA signs the leaf");
    Certificates {
        ca: ca.der().clone(),
        ca_pem: ca.pem(),
        leaf: leaf.der().clone(),
        key: PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
    }
}

fn provider() -> Arc<tokio_rustls::rustls::crypto::CryptoProvider> {
    Arc::new(tokio_rustls::rustls::crypto::ring::default_provider())
}

fn server_config(leaf: CertificateDer<'static>, key: PrivateKeyDer<'static>) -> ServerConfig {
    ServerConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .expect("the default versions are supported")
        .with_no_client_auth()
        .with_single_cert(vec![leaf], key)
        .expect("the certificate matches its key")
}

fn client_config(ca: CertificateDer<'static>) -> Arc<ClientConfig> {
    let mut roots = RootCertStore::empty();
    roots.add(ca).expect("a CA certificate is a root");
    Arc::new(
        ClientConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()
            .expect("the default versions are supported")
            .with_root_certificates(roots)
            .with_no_client_auth(),
    )
}

async fn serve<P: Protocol>(script: P, acceptor: TlsAcceptor, lines: Lines, tcp: TcpStream) {
    let (tcp, phase) = match script.listen() {
        Listen::Tls => (tcp, Phase::Tls),
        Listen::Plain(starttls) => {
            let Some(tcp) = script.converse(Phase::Plain(starttls), &lines, tcp).await else {
                return;
            };
            (tcp, Phase::Upgraded)
        }
    };
    let Ok(stream) = acceptor.accept(tcp).await else {
        return;
    };
    script.converse(phase, &lines, stream).await;
}
