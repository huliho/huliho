// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Name resolution behind one trait, so tests answer records of their
//! own.

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;

use hickory_resolver::TokioResolver;
use hickory_resolver::lookup::Lookup as Answer;
use hickory_resolver::net::NetError;
use hickory_resolver::proto::rr::{Name, RData, Record};
use thiserror::Error;

/// A lookup in flight; boxed so the resolver can be a trait object.
pub type Lookup<'a, T> = Pin<Box<dyn Future<Output = Result<T, DnsError>> + Send + 'a>>;

/// Port 0 in an answer means "the URL's port"; hyper fills it in.
const URL_PORT: u16 = 0;

/// A lookup that failed for a reason other than "no such record".
#[derive(Debug, Error)]
#[error("{0}")]
pub struct DnsError(String);

impl From<NetError> for DnsError {
    fn from(error: NetError) -> Self {
        Self(error.to_string())
    }
}

/// One SRV record. `host` is `None` when the record names the root,
/// which says the service is absent (RFC 2782, RFC 6186 section 3.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SrvTarget {
    pub priority: u16,
    pub weight: u16,
    pub port: u16,
    pub host: Option<String>,
}

/// The lookups discovery and the connector need.
pub trait Dns: Send + Sync {
    /// The addresses of `host`, each with port 0 so the URL's port
    /// applies. A test double may answer a port of its own, which hyper
    /// keeps when the URL names none.
    fn addresses<'a>(&'a self, host: &'a str) -> Lookup<'a, Vec<SocketAddr>>;

    /// The SRV records of `service`, empty when there are none.
    fn srv<'a>(&'a self, service: &'a str) -> Lookup<'a, Vec<SrvTarget>>;

    /// The MX names of `domain` in ASCII lowercase, most preferred
    /// first, empty when there are none.
    fn mx<'a>(&'a self, domain: &'a str) -> Lookup<'a, Vec<String>>;
}

/// The system resolver.
pub struct HickoryDns {
    resolver: TokioResolver,
}

impl HickoryDns {
    /// Reads the system's resolver configuration and hosts file.
    ///
    /// # Errors
    ///
    /// Returns an error when that configuration cannot be read.
    pub fn from_system() -> Result<Self, NetError> {
        Ok(Self {
            resolver: TokioResolver::builder_tokio()?.build()?,
        })
    }
}

impl Dns for HickoryDns {
    fn addresses<'a>(&'a self, host: &'a str) -> Lookup<'a, Vec<SocketAddr>> {
        Box::pin(async move {
            let lookup = self.resolver.lookup_ip(host).await?;
            Ok(lookup
                .iter()
                .map(|address| SocketAddr::new(address, URL_PORT))
                .collect())
        })
    }

    fn srv<'a>(&'a self, service: &'a str) -> Lookup<'a, Vec<SrvTarget>> {
        Box::pin(async move {
            let records = answers(self.resolver.srv_lookup(service).await)?;
            Ok(records
                .iter()
                .filter_map(|record| match &record.data {
                    RData::SRV(srv) => Some(SrvTarget {
                        priority: srv.priority,
                        weight: srv.weight,
                        port: srv.port,
                        host: host_of(&srv.target),
                    }),
                    _ => None,
                })
                .collect())
        })
    }

    fn mx<'a>(&'a self, domain: &'a str) -> Lookup<'a, Vec<String>> {
        Box::pin(async move {
            let records = answers(self.resolver.mx_lookup(domain).await)?;
            let mut exchanges: Vec<(u16, String)> = records
                .iter()
                .filter_map(|record| match &record.data {
                    RData::MX(mx) => host_of(&mx.exchange).map(|host| (mx.preference, host)),
                    _ => None,
                })
                .collect();
            exchanges.sort();
            Ok(exchanges.into_iter().map(|(_, host)| host).collect())
        })
    }
}

/// The answer records; none when the name has no such record.
fn answers(result: Result<Answer, NetError>) -> Result<Vec<Record>, DnsError> {
    match result {
        Ok(lookup) => Ok(lookup.answers().to_vec()),
        Err(error) if error.is_no_records_found() => Ok(Vec::new()),
        Err(error) => Err(error.into()),
    }
}

/// The name in ASCII lowercase without its trailing dot; `None` for the
/// root.
fn host_of(name: &Name) -> Option<String> {
    if name.is_root() {
        return None;
    }
    Some(name.to_ascii().trim_end_matches('.').to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_root_names_no_host() {
        assert_eq!(host_of(&Name::root()), None);
    }

    #[test]
    fn a_name_loses_its_dot_and_its_case() {
        let name = Name::from_ascii("API.Fastmail.com.").unwrap();
        assert_eq!(host_of(&name), Some("api.fastmail.com".to_owned()));
    }
}
