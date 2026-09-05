// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The discovery chain against fake DNS and a TLS server: the order of
//! the steps, what each yields and what it refuses.

mod fake_dns;
mod tls_server;

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Router;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Redirect};
use axum::routing::get;
use fake_dns::FakeDns;
use huliho_server::accounts::{AccountSettings, Provider, TlsMode};
use huliho_server::discovery::{self, Address, Budget, Discovered};
use huliho_server::upstream::{SrvTarget, Upstream};
use tls_server::TlsServer;

const DOMAIN: &str = "example.test";
const LOCAL_PART: &str = "sanne";
const ISPDB: &str = "autoconfig.thunderbird.net";

/// Longer than every budget a test sets, shorter than the default step.
const SLOW: Duration = Duration::from_secs(2);

/// A budget the slow routes exhaust: two timed-out steps end it.
const SHORT: Budget = Budget {
    step: Duration::from_millis(300),
    total: Duration::from_millis(500),
};

fn address(text: &str) -> Address {
    Address::parse(text).unwrap()
}

fn srv(host: Option<&str>, port: u16) -> SrvTarget {
    SrvTarget {
        priority: 0,
        weight: 0,
        port,
        host: host.map(str::to_owned),
    }
}

/// An autoconfig document; each server is (socket type, host, port).
fn document(incoming: &[(&str, &str, u16)], outgoing: &[(&str, &str, u16)]) -> String {
    let server = |tag: &str, kind: &str, (socket, host, port): &(&str, &str, u16)| {
        format!(
            "<{tag} type=\"{kind}\"><hostname>{host}</hostname><port>{port}</port>\
             <socketType>{socket}</socketType><username>%EMAILADDRESS%</username></{tag}>"
        )
    };
    let incoming: String = incoming
        .iter()
        .map(|entry| server("incomingServer", "imap", entry))
        .collect();
    let outgoing: String = outgoing
        .iter()
        .map(|entry| server("outgoingServer", "smtp", entry))
        .collect();
    format!(
        "<clientConfig version=\"1.1\"><emailProvider id=\"{DOMAIN}\">\
         <domain>{DOMAIN}</domain>{incoming}{outgoing}</emailProvider></clientConfig>"
    )
}

fn challenge() -> impl IntoResponse {
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, "Basic realm=\"test\"")],
        "",
    )
}

/// A JMAP server: the well-known redirects to the session resource,
/// which challenges.
fn jmap_routes(router: Router) -> Router {
    router
        .route(
            "/.well-known/jmap",
            get(|| async { Redirect::permanent("/jmap/session") }),
        )
        .route("/jmap/session", get(|| async { challenge() }))
}

fn ispdb_route(router: Router, document: String) -> Router {
    router.route(
        "/v1.1/{domain}",
        get(move || async move { ([(header::CONTENT_TYPE, "text/xml")], document) }),
    )
}

fn slow_routes(router: Router) -> Router {
    async fn slow() -> &'static str {
        tokio::time::sleep(SLOW).await;
        ""
    }
    router
        .route("/.well-known/jmap", get(slow))
        .route("/mail/config-v1.1.xml", get(slow))
        .route("/.well-known/autoconfig/mail/config-v1.1.xml", get(slow))
        .route("/v1.1/{domain}", get(slow))
}

/// The fake resolver with every name at the server.
fn dns_at(server: &TlsServer, names: &[&str]) -> FakeDns {
    let mut dns = FakeDns::default();
    for name in names {
        dns.addresses
            .insert((*name).to_owned(), vec![server.address]);
    }
    dns
}

async fn run(
    server: &TlsServer,
    dns: Arc<FakeDns>,
    allow_loopback: bool,
    budget: Budget,
) -> Option<Discovered> {
    let upstream = Upstream::with_dns(&server.config(allow_loopback), dns).unwrap();
    let address = address(&format!("{LOCAL_PART}@{DOMAIN}"));
    discovery::discover(&upstream, &address, budget).await
}

fn imap_of(found: &Discovered) -> (&str, u16, TlsMode, &str, u16, TlsMode, &str) {
    match &found.target {
        AccountSettings::Imap {
            username,
            imap,
            smtp,
        } => (
            &imap.host, imap.port, imap.tls, &smtp.host, smtp.port, smtp.tls, username,
        ),
        AccountSettings::Jmap { .. } => panic!("not an IMAP target: {:?}", found.target),
    }
}

fn session_url_of(found: &Discovered) -> String {
    match &found.target {
        AccountSettings::Jmap { session_url } => session_url.to_string(),
        AccountSettings::Imap { .. } => panic!("not a JMAP target: {:?}", found.target),
    }
}

#[tokio::test]
async fn a_jmap_hit_stops_the_chain() {
    let server =
        TlsServer::start(ispdb_route(jmap_routes(Router::new()), document(&[], &[]))).await;
    let mut dns = dns_at(&server, &[DOMAIN, ISPDB]);
    let imaps = format!("_imaps._tcp.{DOMAIN}");
    dns.srv
        .insert(imaps.clone(), vec![srv(Some("imap.example.test"), 993)]);
    dns.mx
        .insert(DOMAIN.to_owned(), vec!["aspmx.l.google.com".to_owned()]);
    let dns = Arc::new(dns);
    let found = run(&server, Arc::clone(&dns), true, Budget::default())
        .await
        .unwrap();
    assert_eq!(
        session_url_of(&found),
        format!("https://{DOMAIN}/jmap/session")
    );
    assert_eq!(found.provider, Provider::Generic);
    assert_eq!(found.host(), DOMAIN);
    let queries = dns.queries.lock().unwrap().clone();
    assert!(
        !queries
            .iter()
            .any(|query| query.contains(&imaps) || query.starts_with("mx"))
    );
    assert!(!server.requests().iter().any(|line| line.contains("/v1.1/")));
}

#[tokio::test]
async fn the_srv_record_finds_jmap_when_the_well_known_does_not() {
    let server = TlsServer::start(jmap_routes(Router::new())).await;
    let mut dns = dns_at(&server, &["jmap.example.test"]);
    dns.srv.insert(
        format!("_jmap._tcp.{DOMAIN}"),
        vec![srv(Some("jmap.example.test"), server.address.port())],
    );
    let found = run(&server, Arc::new(dns), true, Budget::default())
        .await
        .unwrap();
    assert_eq!(
        session_url_of(&found),
        format!(
            "https://jmap.example.test:{}/jmap/session",
            server.address.port()
        )
    );
    assert_eq!(found.host(), "jmap.example.test");
}

#[tokio::test]
async fn a_well_known_that_answers_a_page_is_no_hit() {
    let router = Router::new().route(
        "/.well-known/jmap",
        get(|| async { "<html>welcome</html>" }),
    );
    let server = TlsServer::start(router).await;
    let dns = Arc::new(dns_at(&server, &[DOMAIN, ISPDB]));
    assert_eq!(run(&server, dns, true, Budget::default()).await, None);
    let requests = server.requests();
    assert!(
        requests
            .iter()
            .any(|line| line.ends_with(&format!("/v1.1/{DOMAIN}")))
    );
}

#[tokio::test]
async fn an_srv_root_target_counts_as_absent() {
    let server = TlsServer::start(Router::new()).await;
    let mut dns = dns_at(&server, &[DOMAIN]);
    dns.srv
        .insert(format!("_imaps._tcp.{DOMAIN}"), vec![srv(None, 0)]);
    dns.srv.insert(
        format!("_imap._tcp.{DOMAIN}"),
        vec![srv(Some("imap.example.test"), 1143)],
    );
    dns.srv.insert(
        format!("_submissions._tcp.{DOMAIN}"),
        vec![srv(Some("smtp.example.test"), 1465)],
    );
    let found = run(&server, Arc::new(dns), true, Budget::default())
        .await
        .unwrap();
    assert_eq!(
        imap_of(&found),
        (
            "imap.example.test",
            1143,
            TlsMode::Starttls,
            "smtp.example.test",
            1465,
            TlsMode::Implicit,
            "sanne@example.test",
        )
    );
    assert_eq!(found.provider, Provider::Generic);
    assert_eq!(found.host(), "imap.example.test");
}

#[tokio::test]
async fn an_srv_step_without_a_submission_record_moves_on() {
    let ispdb = document(
        &[("SSL", "imap.example.test", 993)],
        &[("STARTTLS", "smtp.example.test", 587)],
    );
    let server = TlsServer::start(ispdb_route(Router::new(), ispdb)).await;
    let mut dns = dns_at(&server, &[DOMAIN, ISPDB]);
    dns.srv.insert(
        format!("_imaps._tcp.{DOMAIN}"),
        vec![srv(Some("imap.example.test"), 993)],
    );
    let found = run(&server, Arc::new(dns), true, Budget::default())
        .await
        .unwrap();
    assert_eq!(imap_of(&found).0, "imap.example.test");
    assert_eq!(imap_of(&found).5, TlsMode::Starttls);
    assert!(server.requests().iter().any(|line| line.contains("/v1.1/")));
}

#[tokio::test]
async fn a_plain_autoconfig_entry_is_dropped() {
    let mixed = document(
        &[
            ("plain", "plain.example.test", 143),
            ("SSL", "imap.example.test", 993),
        ],
        &[
            ("plain", "plain.example.test", 25),
            ("STARTTLS", "smtp.example.test", 587),
        ],
    );
    let server = TlsServer::start(ispdb_route(Router::new(), mixed)).await;
    let dns = Arc::new(dns_at(&server, &[DOMAIN, ISPDB]));
    let found = run(&server, dns, true, Budget::default()).await.unwrap();
    let (imap, _, imap_tls, smtp, _, smtp_tls, _) = imap_of(&found);
    assert_eq!((imap, imap_tls), ("imap.example.test", TlsMode::Implicit));
    assert_eq!((smtp, smtp_tls), ("smtp.example.test", TlsMode::Starttls));

    let plain_only = document(
        &[("plain", "plain.example.test", 143)],
        &[("plain", "plain.example.test", 25)],
    );
    let server = TlsServer::start(ispdb_route(Router::new(), plain_only)).await;
    let dns = Arc::new(dns_at(&server, &[DOMAIN, ISPDB]));
    assert_eq!(run(&server, dns, true, Budget::default()).await, None);
}

#[tokio::test]
async fn the_ispdb_request_carries_no_address() {
    let ispdb = document(
        &[("SSL", "imap.example.test", 993)],
        &[("SSL", "smtp.example.test", 465)],
    );
    let server = TlsServer::start(ispdb_route(Router::new(), ispdb)).await;
    let dns = Arc::new(dns_at(&server, &[DOMAIN, ISPDB]));
    assert!(run(&server, dns, true, Budget::default()).await.is_some());
    let requests = server.requests();
    assert!(requests.contains(&format!("GET {ISPDB} /v1.1/{DOMAIN}")));
    for line in &requests {
        assert!(!line.contains(LOCAL_PART), "{line}");
        assert!(!line.to_lowercase().contains("emailaddress"), "{line}");
    }
}

#[tokio::test]
async fn a_private_address_from_dns_is_refused_before_the_connect() {
    let server = TlsServer::start(jmap_routes(Router::new())).await;
    let dns = Arc::new(dns_at(&server, &[DOMAIN]));
    assert_eq!(
        run(&server, Arc::clone(&dns), false, Budget::default()).await,
        None
    );
    assert!(server.requests().is_empty());
    assert!(run(&server, dns, true, Budget::default()).await.is_some());
}

#[tokio::test]
async fn a_redirect_away_from_https_is_refused() {
    let router = Router::new()
        .route(
            "/.well-known/jmap",
            get(|| async { Redirect::permanent("http://example.test/jmap/session") }),
        )
        .route("/jmap/session", get(|| async { challenge() }));
    let server = TlsServer::start(router).await;
    let dns = Arc::new(dns_at(&server, &[DOMAIN]));
    assert_eq!(run(&server, dns, true, Budget::default()).await, None);
    assert!(
        !server
            .requests()
            .iter()
            .any(|line| line.ends_with("/jmap/session"))
    );
}

#[tokio::test]
async fn the_mx_step_names_the_provider() {
    let server = TlsServer::start(Router::new()).await;
    let mut dns = dns_at(&server, &[DOMAIN]);
    dns.mx.insert(
        DOMAIN.to_owned(),
        vec![
            "alt1.aspmx.l.google.com".to_owned(),
            "aspmx.l.google.com".to_owned(),
        ],
    );
    let found = run(&server, Arc::new(dns), true, Budget::default())
        .await
        .unwrap();
    assert_eq!(found.provider, Provider::Gmail);
    assert_eq!(imap_of(&found).0, "imap.gmail.com");
    assert_eq!(imap_of(&found).6, "sanne@example.test");
}

#[tokio::test]
async fn a_well_known_domain_makes_no_lookup() {
    let server = TlsServer::start(Router::new()).await;
    let dns = Arc::new(FakeDns::default());
    let upstream = Upstream::with_dns(&server.config(true), Arc::<FakeDns>::clone(&dns)).unwrap();
    let found = discovery::discover(&upstream, &address("sanne@gmail.com"), Budget::default())
        .await
        .unwrap();
    assert_eq!(found.provider, Provider::Gmail);
    assert!(dns.queries.lock().unwrap().is_empty());
    assert!(server.requests().is_empty());
}

#[tokio::test]
async fn a_hit_takes_its_provider_from_its_host() {
    let ispdb = document(
        &[("SSL", "imap.gmail.com", 993)],
        &[("SSL", "smtp.gmail.com", 465)],
    );
    let server = TlsServer::start(ispdb_route(Router::new(), ispdb)).await;
    let dns = Arc::new(dns_at(&server, &[DOMAIN, ISPDB]));
    let found = run(&server, dns, true, Budget::default()).await.unwrap();
    assert_eq!(found.provider, Provider::Gmail);
}

#[tokio::test]
async fn the_total_budget_ends_a_slow_chain() {
    let server = TlsServer::start(slow_routes(Router::new())).await;
    let mut dns = dns_at(&server, &[DOMAIN, ISPDB, "autoconfig.example.test"]);
    dns.mx
        .insert(DOMAIN.to_owned(), vec!["aspmx.l.google.com".to_owned()]);
    let dns = Arc::new(dns);
    let started = Instant::now();
    assert_eq!(run(&server, Arc::clone(&dns), true, SHORT).await, None);
    assert!(started.elapsed() < Duration::from_secs(1));
    let patient = Budget {
        total: Duration::from_secs(5),
        ..SHORT
    };
    let found = run(&server, dns, true, patient).await.unwrap();
    assert_eq!(found.provider, Provider::Gmail);
}
