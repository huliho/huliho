// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! An IMAP server for tests: a fresh certificate, one scripted answer
//! per command and a record of every line it received.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use huliho_imap_bridge::session::{Target, TlsMode};
use rcgen::{BasicConstraints, CertificateParams, CertifiedIssuer, IsCa, KeyPair};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio_rustls::rustls::{ClientConfig, RootCertStore, ServerConfig};

/// The name the certificate carries.
pub const HOST: &str = "imap.example.test";
pub const USER: &str = "sanne";
pub const PASSWORD: &str = "correct horse";
pub const TOKEN: &str = "ya29.token";
/// Advertised once TLS is on, unless a script says otherwise.
pub const CAPABILITIES: &str = "IMAP4rev2 IMAP4rev1 AUTH=PLAIN AUTH=XOAUTH2 IDLE";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Starttls {
    Offered,
    Absent,
    Refused,
}

#[derive(Clone, Copy)]
pub enum Listen {
    Tls,
    Plain(Starttls),
}

#[derive(Clone, Copy)]
pub enum Greeting {
    Ok,
    Bye,
    Garbage,
    Silence,
}

#[derive(Clone, Copy)]
pub struct Script {
    pub listen: Listen,
    pub greeting: Greeting,
    /// Whether commands after the greeting get an answer.
    pub answers: bool,
    /// Advertised once TLS is on.
    pub capabilities: &'static str,
}

impl Script {
    pub fn tls() -> Self {
        Self {
            listen: Listen::Tls,
            greeting: Greeting::Ok,
            answers: true,
            capabilities: CAPABILITIES,
        }
    }

    pub fn plain(starttls: Starttls) -> Self {
        Self {
            listen: Listen::Plain(starttls),
            ..Self::tls()
        }
    }
}

/// Where a conversation stands; after STARTTLS no greeting is sent.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Plain(Starttls),
    Tls,
    Upgraded,
}

impl Phase {
    fn label(self) -> &'static str {
        match self {
            Self::Plain(_) => "plain",
            Self::Tls | Self::Upgraded => "tls",
        }
    }

    fn capabilities(self, script: Script) -> &'static str {
        match self {
            Self::Plain(Starttls::Absent) => "IMAP4rev1 LOGINDISABLED",
            Self::Plain(_) => "IMAP4rev1 STARTTLS LOGINDISABLED",
            Self::Tls | Self::Upgraded => script.capabilities,
        }
    }

    fn is_tls(self) -> bool {
        !matches!(self, Self::Plain(_))
    }
}

type Lines = Arc<Mutex<Vec<String>>>;

pub struct FakeImap {
    pub address: SocketAddr,
    ca: CertificateDer<'static>,
    lines: Lines,
}

impl FakeImap {
    /// Listens on a loopback port and serves every connection by the
    /// script.
    pub async fn start(script: Script) -> Self {
        let (ca, leaf, key) = certificates();
        let acceptor = TlsAcceptor::from(Arc::new(server_config(leaf, key)));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let lines = Lines::default();
        let recorded = Arc::clone(&lines);
        tokio::spawn(async move {
            while let Ok((tcp, _)) = listener.accept().await {
                let acceptor = acceptor.clone();
                let lines = Arc::clone(&recorded);
                tokio::spawn(serve(script, acceptor, lines, tcp));
            }
        });
        Self { address, ca, lines }
    }

    /// A client that trusts this server's CA and nothing else.
    pub fn trusting(&self) -> Arc<ClientConfig> {
        let mut roots = RootCertStore::empty();
        roots.add(self.ca.clone()).unwrap();
        client_config(roots)
    }

    /// A client that trusts an unrelated CA only.
    pub fn distrusting() -> Arc<ClientConfig> {
        let (ca, _leaf, _key) = certificates();
        let mut roots = RootCertStore::empty();
        roots.add(ca).unwrap();
        client_config(roots)
    }

    pub fn target(&self, host: &str, tls: TlsMode) -> Target {
        Target {
            host: host.to_owned(),
            addresses: vec![self.address],
            tls,
        }
    }

    /// Every line received so far, prefixed with `plain` or `tls`; the
    /// SASL data the client sent appears decoded after `SASL`.
    pub fn lines(&self) -> Vec<String> {
        self.lines.lock().unwrap().clone()
    }
}

fn certificates() -> (
    CertificateDer<'static>,
    CertificateDer<'static>,
    PrivateKeyDer<'static>,
) {
    let ca_key = KeyPair::generate().unwrap();
    let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let ca = CertifiedIssuer::self_signed(ca_params, ca_key).unwrap();
    let key = KeyPair::generate().unwrap();
    let leaf = CertificateParams::new(vec![HOST.to_owned()])
        .unwrap()
        .signed_by(&key, &ca)
        .unwrap();
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der()));
    (ca.der().clone(), leaf.der().clone(), key)
}

fn provider() -> Arc<tokio_rustls::rustls::crypto::CryptoProvider> {
    Arc::new(tokio_rustls::rustls::crypto::ring::default_provider())
}

fn server_config(leaf: CertificateDer<'static>, key: PrivateKeyDer<'static>) -> ServerConfig {
    ServerConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![leaf], key)
        .unwrap()
}

fn client_config(roots: RootCertStore) -> Arc<ClientConfig> {
    Arc::new(
        ClientConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    )
}

async fn serve(script: Script, acceptor: TlsAcceptor, lines: Lines, tcp: TcpStream) {
    let (tcp, phase) = match script.listen {
        Listen::Tls => (tcp, Phase::Tls),
        Listen::Plain(starttls) => {
            let Some(tcp) = converse(script, Phase::Plain(starttls), &lines, tcp).await else {
                return;
            };
            (tcp, Phase::Upgraded)
        }
    };
    let Ok(stream) = acceptor.accept(tcp).await else {
        return;
    };
    converse(script, phase, &lines, stream).await;
}

/// Answers commands until the client leaves; hands the stream back only
/// when STARTTLS was accepted, so the caller can upgrade it.
async fn converse<S: AsyncRead + AsyncWrite + Unpin>(
    script: Script,
    phase: Phase,
    lines: &Lines,
    stream: S,
) -> Option<S> {
    let (reader, mut writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader);
    if phase != Phase::Upgraded {
        let text = match script.greeting {
            Greeting::Ok => "* OK ready\r\n",
            Greeting::Bye => "* BYE not now\r\n",
            Greeting::Garbage => "220 mail.example.test ESMTP\r\n",
            Greeting::Silence => std::future::pending().await,
        };
        writer.write_all(text.as_bytes()).await.ok()?;
        if !matches!(script.greeting, Greeting::Ok) {
            return None;
        }
    }
    if !script.answers {
        std::future::pending::<()>().await;
    }
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).await.ok()? == 0 {
            return None;
        }
        let line = line.trim_end_matches(['\r', '\n']);
        lines
            .lock()
            .unwrap()
            .push(format!("{} {line}", phase.label()));
        let (tag, command) = line.split_once(' ').unwrap_or((line, ""));
        let verb = command
            .split(' ')
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase();
        let reply = match verb.as_str() {
            "CAPABILITY" => format!(
                "* CAPABILITY {}\r\n{tag} OK done\r\n",
                phase.capabilities(script)
            ),
            "STARTTLS" if phase == Phase::Plain(Starttls::Offered) => {
                writer
                    .write_all(format!("{tag} OK begin TLS\r\n").as_bytes())
                    .await
                    .ok()?;
                return Some(reader.into_inner().unsplit(writer));
            }
            "STARTTLS" if phase == Phase::Plain(Starttls::Refused) => {
                format!("{tag} NO not now\r\n")
            }
            "LOGIN" if phase.is_tls() && command == format!("LOGIN \"{USER}\" \"{PASSWORD}\"") => {
                format!("{tag} OK signed in\r\n")
            }
            "LOGIN" => format!("{tag} NO [AUTHENTICATIONFAILED] Authentication failed.\r\n"),
            "AUTHENTICATE" if phase.is_tls() => {
                xoauth2(&mut reader, &mut writer, lines, tag).await?
            }
            "LOGOUT" => {
                writer
                    .write_all(format!("* BYE bye\r\n{tag} OK done\r\n").as_bytes())
                    .await
                    .ok();
                return None;
            }
            _ => format!("{tag} BAD unknown command\r\n"),
        };
        writer.write_all(reply.as_bytes()).await.ok()?;
    }
}

/// The XOAUTH2 exchange as Google runs it: an empty challenge, the
/// identity; on a wrong token a challenge carrying the error, an empty
/// answer and then NO.
async fn xoauth2<R: AsyncBufRead + Unpin, W: AsyncWrite + Unpin>(
    reader: &mut R,
    writer: &mut W,
    lines: &Lines,
    tag: &str,
) -> Option<String> {
    writer.write_all(b"+ \r\n").await.ok()?;
    let mut response = String::new();
    reader.read_line(&mut response).await.ok()?;
    let decoded = BASE64.decode(response.trim_end()).ok()?;
    lines
        .lock()
        .unwrap()
        .push(format!("tls SASL {}", String::from_utf8_lossy(&decoded)));
    if decoded == format!("user={USER}\x01auth=Bearer {TOKEN}\x01\x01").as_bytes() {
        return Some(format!("{tag} OK signed in\r\n"));
    }
    let error =
        BASE64.encode(r#"{"status":"401","schemes":"bearer","scope":"https://mail.google.com/"}"#);
    writer
        .write_all(format!("+ {error}\r\n").as_bytes())
        .await
        .ok()?;
    let mut empty = String::new();
    reader.read_line(&mut empty).await.ok()?;
    lines.lock().unwrap().push(format!(
        "tls SASL {:?}",
        empty.trim_end_matches(['\r', '\n'])
    ));
    Some(format!(
        "{tag} NO [AUTHENTICATIONFAILED] Invalid credentials (Failure)\r\n"
    ))
}
