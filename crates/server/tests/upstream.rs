// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The connector: pinned addresses behind the network rule, TLS against
//! the roots plus the CA file, redirects over HTTPS to named hosts only.

mod fake_dns;
mod tls_server;

use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use axum::Router;
use axum::extract::Path;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Redirect};
use axum::routing::get;
use fake_dns::FakeDns;
use huliho_server::config::UpstreamConfig;
use huliho_server::upstream::{Dns, Upstream, UpstreamError};
use tls_server::TlsServer;

const HOST: &str = "example.test";

/// The redirect chain the connector still follows in full.
const REDIRECTS_FOLLOWED: u32 = 3;

fn app() -> Router {
    Router::new()
        .route("/", get(|| async { "ok" }))
        .route(
            "/to-http",
            get(|| async { Redirect::temporary("http://example.test/") }),
        )
        .route(
            "/to-other",
            get(|| async { Redirect::temporary("https://other.test/") }),
        )
        .route("/to-literal", get(to_literal))
        .route(
            "/hop/{n}",
            get(
                |Path(n): Path<u32>| async move { Redirect::temporary(&format!("/hop/{}", n + 1)) },
            ),
        )
        .route(
            "/three/{n}",
            get(|Path(n): Path<u32>| async move {
                if n >= REDIRECTS_FOLLOWED {
                    "ok".into_response()
                } else {
                    Redirect::temporary(&format!("/three/{}", n + 1)).into_response()
                }
            }),
        )
}

/// Redirects to the server's own address literal on the port the Host
/// header names.
async fn to_literal(headers: HeaderMap) -> Redirect {
    let host = headers[header::HOST].to_str().unwrap();
    let port = host.rsplit(':').next().unwrap();
    Redirect::temporary(&format!("https://127.0.0.1:{port}/"))
}

/// Every name at the server; the URL carries the port.
fn dns_at(server: &TlsServer, names: &[&str]) -> Arc<dyn Dns> {
    let mut dns = FakeDns::default();
    for name in names {
        dns.addresses
            .insert((*name).to_owned(), vec![server.address]);
    }
    Arc::new(dns)
}

fn url(server: &TlsServer, host: &str, path: &str) -> String {
    format!("https://{host}:{}{path}", server.address.port())
}

async fn status(upstream: &Upstream, url: &str) -> Result<StatusCode, reqwest::Error> {
    upstream
        .http()
        .get(url)
        .send()
        .await
        .map(|response| response.status())
}

#[tokio::test]
async fn a_private_address_from_dns_is_refused_before_the_connect() {
    let server = TlsServer::start(app()).await;
    let dns = dns_at(&server, &[HOST]);
    let url = url(&server, HOST, "/");
    let refusing = Upstream::with_dns(&server.config(false), Arc::clone(&dns)).unwrap();
    assert!(status(&refusing, &url).await.is_err());
    assert!(server.requests().is_empty());
    let allowing = Upstream::with_dns(&server.config(true), dns).unwrap();
    assert_eq!(status(&allowing, &url).await.unwrap(), StatusCode::OK);
    assert_eq!(server.requests().len(), 1);
}

#[tokio::test]
async fn a_certificate_outside_the_roots_is_refused() {
    let server = TlsServer::start(app()).await;
    let config = UpstreamConfig {
        additional_ca_file: None,
        ..server.config(true)
    };
    let upstream = Upstream::with_dns(&config, dns_at(&server, &[HOST])).unwrap();
    assert!(status(&upstream, &url(&server, HOST, "/")).await.is_err());
    assert!(server.requests().is_empty());
}

#[tokio::test]
async fn a_name_the_certificate_does_not_carry_is_refused() {
    let server = TlsServer::start(app()).await;
    let upstream =
        Upstream::with_dns(&server.config(true), dns_at(&server, &["unlisted.test"])).unwrap();
    let result = status(&upstream, &url(&server, "unlisted.test", "/")).await;
    assert!(result.is_err());
    assert!(server.requests().is_empty());
}

#[tokio::test]
async fn a_redirect_away_from_https_is_refused() {
    let server = TlsServer::start(app()).await;
    let upstream = Upstream::with_dns(&server.config(true), dns_at(&server, &[HOST])).unwrap();
    assert!(
        status(&upstream, &url(&server, HOST, "/to-http"))
            .await
            .is_err()
    );
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].ends_with("/to-http"));
}

#[tokio::test]
async fn a_redirect_to_an_address_literal_is_refused() {
    let server = TlsServer::start(app()).await;
    let upstream = Upstream::with_dns(&server.config(true), dns_at(&server, &[HOST])).unwrap();
    assert!(
        status(&upstream, &url(&server, HOST, "/to-literal"))
            .await
            .is_err()
    );
    assert_eq!(server.requests().len(), 1);
}

#[tokio::test]
async fn a_redirect_into_a_private_network_is_refused() {
    let server = TlsServer::start(app()).await;
    let mut dns = FakeDns::default();
    dns.addresses.insert(HOST.to_owned(), vec![server.address]);
    dns.addresses.insert(
        "other.test".to_owned(),
        vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 0)],
    );
    let upstream = Upstream::with_dns(&server.config(true), Arc::new(dns)).unwrap();
    assert!(
        status(&upstream, &url(&server, HOST, "/to-other"))
            .await
            .is_err()
    );
    assert_eq!(server.requests().len(), 1);
}

#[tokio::test]
async fn three_redirects_are_followed_and_a_fourth_is_refused() {
    let server = TlsServer::start(app()).await;
    let upstream = Upstream::with_dns(&server.config(true), dns_at(&server, &[HOST])).unwrap();
    let three = status(&upstream, &url(&server, HOST, "/three/0")).await;
    assert_eq!(three.unwrap(), StatusCode::OK);
    assert_eq!(server.requests().len(), 4);
    assert!(
        status(&upstream, &url(&server, HOST, "/hop/0"))
            .await
            .is_err()
    );
    let hops: Vec<String> = server
        .requests()
        .into_iter()
        .filter(|line| line.contains("/hop/"))
        .collect();
    assert_eq!(hops.len(), 4);
    assert!(hops[3].ends_with("/hop/3"));
}

#[test]
fn a_ca_file_the_roots_cannot_use_fails_the_start() {
    let empty = tempfile::NamedTempFile::new().unwrap();
    let mut garbage = tempfile::NamedTempFile::new().unwrap();
    garbage.write_all(b"not a certificate").unwrap();
    let missing = empty.path().with_extension("missing");
    for (path, empty_file) in [
        (empty.path(), true),
        (garbage.path(), true),
        (missing.as_path(), false),
    ] {
        let config = UpstreamConfig {
            additional_ca_file: Some(path.to_owned()),
            ..UpstreamConfig::default()
        };
        let error = Upstream::with_dns(&config, Arc::new(FakeDns::default()))
            .err()
            .unwrap();
        assert_eq!(
            matches!(error, UpstreamError::CaFileEmpty { .. }),
            empty_file,
            "{error}"
        );
        assert!(error.to_string().contains(&path.display().to_string()));
    }
}

#[tokio::test]
async fn resolve_pins_the_checked_addresses_on_the_port() {
    let server = TlsServer::start(app()).await;
    let mut dns = FakeDns::default();
    dns.addresses.insert(HOST.to_owned(), vec![server.address]);
    dns.addresses.insert(
        "public.test".to_owned(),
        vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)), 0)],
    );
    let upstream = Upstream::with_dns(&server.config(true), Arc::new(dns)).unwrap();
    assert_eq!(upstream.resolve(HOST, 993).await.unwrap(), [server.address]);
    assert_eq!(
        upstream.resolve("public.test", 993).await.unwrap(),
        [SocketAddr::new(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)), 993)]
    );
    assert!(
        upstream
            .resolve("unknown.test", 993)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn resolve_refuses_a_private_address_unless_its_network_is_listed() {
    let server = TlsServer::start(app()).await;
    let dns = dns_at(&server, &[HOST]);
    let refusing = Upstream::with_dns(&server.config(false), Arc::clone(&dns)).unwrap();
    let error = refusing.resolve(HOST, 993).await.unwrap_err();
    assert!(
        matches!(error, UpstreamError::PrivateNetwork { .. }),
        "{error}"
    );
    let allowing = Upstream::with_dns(&server.config(true), dns).unwrap();
    assert_eq!(allowing.resolve(HOST, 993).await.unwrap(), [server.address]);
}
