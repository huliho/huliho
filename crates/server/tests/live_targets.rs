// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Discovery against the compose Cyrus and the public internet.

#![cfg(feature = "live-targets")]

mod fake_dns;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use fake_dns::FakeDns;
use huliho_server::accounts::{AccountSettings, Provider};
use huliho_server::config::UpstreamConfig;
use huliho_server::discovery::{self, Address, Budget, Discovered};
use huliho_server::upstream::{Dns, SrvTarget, Upstream};
use tracing_subscriber::EnvFilter;

/// The compose Cyrus: JMAP over TLS on the host's loopback.
const CYRUS_HOST: &str = "localhost";
const CYRUS_PORT: u16 = 8443;

fn logging() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new("huliho_server=debug"))
        .with_test_writer()
        .try_init();
}

fn dev_ca() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/dev-certs/ca.pem")
}

fn compose_config(allow_loopback: bool, trust_dev_ca: bool) -> UpstreamConfig {
    let allow_private_networks = if allow_loopback {
        vec!["127.0.0.0/8".parse().unwrap(), "::1/128".parse().unwrap()]
    } else {
        Vec::new()
    };
    UpstreamConfig {
        allow_private_networks,
        additional_ca_file: trust_dev_ca.then(dev_ca),
        ..UpstreamConfig::default()
    }
}

/// `huliho.test` has no public records; its `_jmap._tcp` names Cyrus.
fn cyrus_dns() -> Arc<dyn Dns> {
    let mut dns = FakeDns::default();
    dns.addresses.insert(
        CYRUS_HOST.to_owned(),
        vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)],
    );
    dns.srv.insert(
        "_jmap._tcp.huliho.test".to_owned(),
        vec![SrvTarget {
            priority: 0,
            weight: 0,
            port: CYRUS_PORT,
            host: Some(CYRUS_HOST.to_owned()),
        }],
    );
    Arc::new(dns)
}

async fn discover(upstream: &Upstream, address: &str) -> Option<Discovered> {
    logging();
    discovery::discover(
        upstream,
        &Address::parse(address).unwrap(),
        Budget::default(),
    )
    .await
}

fn session_url_of(found: &Discovered) -> String {
    match &found.target {
        AccountSettings::Jmap { session_url } => session_url.to_string(),
        AccountSettings::Imap { .. } => panic!("not a JMAP target: {:?}", found.target),
    }
}

#[tokio::test]
async fn cyrus_is_found_through_its_srv_record_with_the_dev_ca() {
    let upstream = Upstream::with_dns(&compose_config(true, true), cyrus_dns()).unwrap();
    let found = discover(&upstream, "sanne@huliho.test").await.unwrap();
    assert_eq!(
        session_url_of(&found),
        format!("https://{CYRUS_HOST}:{CYRUS_PORT}/jmap")
    );
    assert_eq!(found.provider, Provider::Generic);
    assert_eq!(found.host(), CYRUS_HOST);
}

#[tokio::test]
async fn cyrus_is_refused_without_the_private_network_rule() {
    let upstream = Upstream::with_dns(&compose_config(false, true), cyrus_dns()).unwrap();
    assert_eq!(discover(&upstream, "sanne@huliho.test").await, None);
}

#[tokio::test]
async fn cyrus_is_refused_without_the_dev_ca() {
    let upstream = Upstream::with_dns(&compose_config(true, false), cyrus_dns()).unwrap();
    assert_eq!(discover(&upstream, "sanne@huliho.test").await, None);
}

#[tokio::test]
async fn fastmail_fm_is_found_through_the_chain_live() {
    let upstream = Upstream::new(&UpstreamConfig::default()).unwrap();
    let found = discover(&upstream, "mira@fastmail.fm").await.unwrap();
    assert_eq!(
        session_url_of(&found),
        "https://api.fastmail.com/jmap/session"
    );
    assert_eq!(found.provider, Provider::Fastmail);
}

#[tokio::test]
async fn gmail_com_is_decided_without_a_lookup() {
    let upstream = Upstream::new(&UpstreamConfig::default()).unwrap();
    let found = discover(&upstream, "sanne@gmail.com").await.unwrap();
    assert_eq!(found.provider, Provider::Gmail);
    assert_eq!(found.host(), "imap.gmail.com");
}

#[tokio::test]
async fn google_com_is_found_through_the_chain_live() {
    let upstream = Upstream::new(&UpstreamConfig::default()).unwrap();
    let found = discover(&upstream, "someone@google.com").await.unwrap();
    assert_eq!(found.provider, Provider::Gmail);
    assert_eq!(found.host(), "imap.gmail.com");
}
