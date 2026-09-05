// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A TLS server for tests: a fresh CA and certificate, every request
//! recorded.

use std::io::Write;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::{Request, State};
use axum::http::header;
use axum::middleware::{self, Next};
use axum::response::Response;
use axum_server::tls_rustls::RustlsConfig;
use huliho_server::config::UpstreamConfig;
use rcgen::{BasicConstraints, CertificateParams, CertifiedIssuer, IsCa, KeyPair};
use tempfile::NamedTempFile;

/// The names the certificate carries; every fake host is one of them.
const NAMES: &[&str] = &[
    "example.test",
    "*.example.test",
    "other.test",
    "*.other.test",
    "autoconfig.thunderbird.net",
];

type Requests = Arc<Mutex<Vec<String>>>;

pub struct TlsServer {
    pub address: SocketAddr,
    ca_file: NamedTempFile,
    requests: Requests,
}

impl TlsServer {
    /// Serves `app` over TLS on a loopback port.
    pub async fn start(app: Router) -> Self {
        let ca_key = KeyPair::generate().unwrap();
        let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let ca = CertifiedIssuer::self_signed(ca_params, ca_key).unwrap();
        let key = KeyPair::generate().unwrap();
        let names: Vec<String> = NAMES.iter().map(|name| (*name).to_owned()).collect();
        let certificate = CertificateParams::new(names)
            .unwrap()
            .signed_by(&key, &ca)
            .unwrap();
        let mut ca_file = NamedTempFile::new().unwrap();
        ca_file.write_all(ca.pem().as_bytes()).unwrap();
        let requests: Requests = Arc::new(Mutex::new(Vec::new()));
        let app = app.layer(middleware::from_fn_with_state(
            Arc::clone(&requests),
            record,
        ));
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let config = RustlsConfig::from_pem(
            certificate.pem().into_bytes(),
            key.serialize_pem().into_bytes(),
        )
        .await
        .unwrap();
        tokio::spawn(
            axum_server::from_tcp_rustls(listener, config)
                .unwrap()
                .serve(app.into_make_service()),
        );
        Self {
            address,
            ca_file,
            requests,
        }
    }

    /// The upstream rules a test runs with: this CA trusted and, when
    /// asked, the loopback network allowed.
    pub fn config(&self, allow_loopback: bool) -> UpstreamConfig {
        let allow_private_networks = if allow_loopback {
            vec!["127.0.0.0/8".parse().unwrap()]
        } else {
            Vec::new()
        };
        UpstreamConfig {
            allow_private_networks,
            additional_ca_file: Some(self.ca_file.path().to_owned()),
            ..UpstreamConfig::default()
        }
    }

    /// Every request so far as `METHOD host path`.
    pub fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

async fn record(State(requests): State<Requests>, request: Request, next: Next) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let line = format!("{} {host} {}", request.method(), request.uri());
    requests.lock().unwrap().push(line);
    next.run(request).await
}
