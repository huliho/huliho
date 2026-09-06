// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Finding the mail server behind an address: the chain of lookups and
//! what each step yields.

mod address;
mod autoconfig;

use std::cmp::Reverse;
use std::time::Duration;

use reqwest::StatusCode;
use reqwest::header::WWW_AUTHENTICATE;
use url::Url;

pub(crate) use address::named_host;
pub use address::{Address, InvalidAddress, MAX_ADDRESS_BYTES};

use crate::accounts::{AccountSettings, Endpoint, Provider, TlsMode};
use crate::presets;
use crate::upstream::{Lookup, SrvTarget, Upstream, read_bounded};

/// One step waits this long before the chain moves on.
const STEP_TIMEOUT: Duration = Duration::from_secs(5);

/// The whole chain ends here, hit or not.
const TOTAL_TIMEOUT: Duration = Duration::from_secs(15);

/// An autoconfig document runs to a few kilobytes; more is not one.
const MAX_AUTOCONFIG_BYTES: usize = 64 * 1024;

/// How long the chain may take: each step on its own and all together.
#[derive(Debug, Clone, Copy)]
pub struct Budget {
    pub step: Duration,
    pub total: Duration,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            step: STEP_TIMEOUT,
            total: TOTAL_TIMEOUT,
        }
    }
}

/// What the chain found: the preset it matches and where the account
/// connects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovered {
    pub provider: Provider,
    pub target: AccountSettings,
}

impl Discovered {
    /// The host the user confirms before a credential moves.
    #[must_use]
    pub fn host(&self) -> &str {
        match &self.target {
            AccountSettings::Jmap { session_url } => session_url.host_str().unwrap_or_default(),
            AccountSettings::Imap { imap, .. } => &imap.host,
        }
    }

    fn jmap(session_url: Url) -> Self {
        Self {
            provider: presets::provider_for_host(session_url.host_str().unwrap_or_default()),
            target: AccountSettings::Jmap { session_url },
        }
    }

    fn imap(imap: Endpoint, smtp: Endpoint, username: String) -> Self {
        Self {
            provider: presets::provider_for_host(&imap.host),
            target: AccountSettings::Imap {
                username,
                imap,
                smtp,
            },
        }
    }
}

/// Finds the mail server for `address`: a well-known mail domain names
/// its preset outright; every other domain runs the chain within
/// `budget`.
pub async fn discover(
    upstream: &Upstream,
    address: &Address,
    budget: Budget,
) -> Option<Discovered> {
    let domain = address.domain();
    if let Some(provider) = presets::provider_for_domain(domain) {
        tracing::info!(domain, step = "preset", "mail server found");
        return presets::fixed_target(provider, address)
            .map(|target| Discovered { provider, target });
    }
    let chain = Chain {
        upstream,
        address,
        budget,
    };
    let Ok(found) = tokio::time::timeout(budget.total, chain.run()).await else {
        tracing::info!(domain, "discovery ran out of time");
        return None;
    };
    found
}

/// The steps in order; the first that finds a server ends the chain.
#[derive(Clone, Copy)]
enum Step {
    Jmap,
    Srv,
    Autoconfig,
    Mx,
}

impl Step {
    fn as_str(self) -> &'static str {
        match self {
            Self::Jmap => "jmap",
            Self::Srv => "srv",
            Self::Autoconfig => "autoconfig",
            Self::Mx => "mx",
        }
    }
}

struct Chain<'a> {
    upstream: &'a Upstream,
    address: &'a Address,
    budget: Budget,
}

impl Chain<'_> {
    async fn run(&self) -> Option<Discovered> {
        if let Some(found) = self.step(Step::Jmap, self.jmap()).await {
            return Some(found);
        }
        if let Some(found) = self.step(Step::Srv, self.srv()).await {
            return Some(found);
        }
        if let Some(found) = self.step(Step::Autoconfig, self.autoconfig()).await {
            return Some(found);
        }
        self.step(Step::Mx, self.mx()).await
    }

    async fn step(
        &self,
        step: Step,
        lookup: impl Future<Output = Option<Discovered>>,
    ) -> Option<Discovered> {
        let domain = self.address.domain();
        match tokio::time::timeout(self.budget.step, lookup).await {
            Ok(Some(found)) => {
                tracing::info!(domain, step = step.as_str(), "mail server found");
                Some(found)
            }
            Ok(None) => {
                tracing::debug!(domain, step = step.as_str(), "nothing found");
                None
            }
            Err(_) => {
                tracing::debug!(domain, step = step.as_str(), "step timed out");
                None
            }
        }
    }

    /// The well-known resource on the domain, then on the host its
    /// `_jmap._tcp` record names (RFC 8620 section 2.2).
    async fn jmap(&self) -> Option<Discovered> {
        let domain = self.address.domain();
        if let Some(session_url) = self
            .session_url(&format!("https://{domain}/.well-known/jmap"))
            .await
        {
            return Some(Discovered::jmap(session_url));
        }
        let (host, port) = self.srv_target(&format!("_jmap._tcp.{domain}")).await?;
        let session_url = self
            .session_url(&format!("https://{host}:{port}/.well-known/jmap"))
            .await?;
        Some(Discovered::jmap(session_url))
    }

    /// The RFC 6186 records; a hit needs an IMAP and a submission server.
    async fn srv(&self) -> Option<Discovered> {
        let imap = self.srv_endpoint("_imaps._tcp", "_imap._tcp").await?;
        let smtp = self
            .srv_endpoint("_submissions._tcp", "_submission._tcp")
            .await?;
        Some(Discovered::imap(imap, smtp, self.address.to_string()))
    }

    /// The provider's own document, the well-known path, then the ISPDB;
    /// the request never carries the address.
    async fn autoconfig(&self) -> Option<Discovered> {
        let domain = self.address.domain();
        let urls = [
            format!("https://autoconfig.{domain}/mail/config-v1.1.xml"),
            format!("https://{domain}/.well-known/autoconfig/mail/config-v1.1.xml"),
            format!("https://autoconfig.thunderbird.net/v1.1/{domain}"),
        ];
        for url in urls {
            let Some(response) = self.get(&url).await else {
                continue;
            };
            if response.status() != StatusCode::OK {
                continue;
            }
            let Some(document) = read_bounded(response, MAX_AUTOCONFIG_BYTES).await else {
                continue;
            };
            if let Some(servers) = autoconfig::parse(&document) {
                let username = servers.username.of(self.address);
                return Some(Discovered::imap(servers.imap, servers.smtp, username));
            }
        }
        None
    }

    /// The MX names of the domain, matched to a provider by suffix.
    async fn mx(&self) -> Option<Discovered> {
        let names = self
            .records(self.upstream.dns().mx(self.address.domain()))
            .await;
        let provider = names
            .iter()
            .map(|name| presets::provider_for_host(name))
            .find(|provider| *provider != Provider::Generic)?;
        let target = presets::fixed_target(provider, self.address)?;
        Some(Discovered { provider, target })
    }

    /// The service's implicit-TLS record first, the STARTTLS one
    /// otherwise.
    async fn srv_endpoint(&self, implicit: &str, starttls: &str) -> Option<Endpoint> {
        let domain = self.address.domain();
        if let Some((host, port)) = self.srv_target(&format!("{implicit}.{domain}")).await {
            return Some(Endpoint {
                host,
                port,
                tls: TlsMode::Implicit,
            });
        }
        let (host, port) = self.srv_target(&format!("{starttls}.{domain}")).await?;
        Some(Endpoint {
            host,
            port,
            tls: TlsMode::Starttls,
        })
    }

    async fn srv_target(&self, service: &str) -> Option<(String, u16)> {
        let records = self.records(self.upstream.dns().srv(service)).await;
        select_srv(records)
    }

    /// The records a lookup answers; none when it fails, with the
    /// failure logged.
    async fn records<T>(&self, lookup: Lookup<'_, Vec<T>>) -> Vec<T> {
        lookup.await.unwrap_or_else(|error| {
            tracing::debug!(domain = self.address.domain(), %error, "lookup failed");
            Vec::new()
        })
    }

    /// A JMAP session resource answers an unauthenticated GET with 401
    /// and a challenge (RFC 8620 section 2, RFC 9110 section 15.5.2);
    /// the URL after the redirects is the session URL.
    async fn session_url(&self, url: &str) -> Option<Url> {
        let response = self.get(url).await?;
        let challenged = response.status() == StatusCode::UNAUTHORIZED
            && response.headers().contains_key(WWW_AUTHENTICATE);
        challenged.then(|| response.url().clone())
    }

    async fn get(&self, url: &str) -> Option<reqwest::Response> {
        match self.upstream.http().get(url).send().await {
            Ok(response) => Some(response),
            Err(error) => {
                tracing::debug!(domain = self.address.domain(), %error, "request failed");
                None
            }
        }
    }
}

/// The record to use: lowest priority, then highest weight, naming a
/// host rather than the root.
fn select_srv(mut records: Vec<SrvTarget>) -> Option<(String, u16)> {
    records.sort_by_key(|record| (record.priority, Reverse(record.weight)));
    let first = records.into_iter().next()?;
    let host = named_host(&first.host?)?;
    Some((host, first.port))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(priority: u16, weight: u16, host: Option<&str>) -> SrvTarget {
        SrvTarget {
            priority,
            weight,
            port: 993,
            host: host.map(str::to_owned),
        }
    }

    #[test]
    fn the_lowest_priority_and_then_the_highest_weight_wins() {
        let records = vec![
            record(10, 0, Some("backup.example.test")),
            record(0, 1, Some("light.example.test")),
            record(0, 5, Some("heavy.example.test")),
        ];
        assert_eq!(
            select_srv(records),
            Some(("heavy.example.test".to_owned(), 993))
        );
    }

    #[test]
    fn a_root_target_in_front_means_absent() {
        assert_eq!(select_srv(vec![record(0, 0, None)]), None);
        assert_eq!(select_srv(Vec::new()), None);
    }

    #[test]
    fn an_address_literal_target_is_refused() {
        assert_eq!(select_srv(vec![record(0, 0, Some("127.0.0.1"))]), None);
    }

    #[test]
    fn the_host_is_the_server_the_user_confirms() {
        let jmap = Discovered::jmap(Url::parse("https://api.fastmail.com/jmap/session").unwrap());
        assert_eq!(jmap.host(), "api.fastmail.com");
        assert_eq!(jmap.provider, Provider::Fastmail);
        let endpoint = |host: &str| Endpoint {
            host: host.to_owned(),
            port: 993,
            tls: TlsMode::Implicit,
        };
        let imap = Discovered::imap(
            endpoint("imap.example.test"),
            endpoint("smtp.example.test"),
            "sanne@example.test".to_owned(),
        );
        assert_eq!(imap.host(), "imap.example.test");
        assert_eq!(imap.provider, Provider::Generic);
    }

    #[test]
    fn the_default_budget_is_five_seconds_a_step_and_fifteen_in_all() {
        let budget = Budget::default();
        assert_eq!(budget.step, Duration::from_secs(5));
        assert_eq!(budget.total, Duration::from_secs(15));
    }
}
