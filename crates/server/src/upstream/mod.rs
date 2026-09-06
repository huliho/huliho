// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Reaching mail servers: one resolver, one network rule and one TLS
//! configuration behind every outbound connection.

mod dns;
mod network;

use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use hickory_resolver::net::NetError;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::redirect;
use rustls::RootCertStore;
use rustls::pki_types::CertificateDer;
use rustls::pki_types::pem::PemObject;
use thiserror::Error;
use url::Host;

pub use dns::{Dns, DnsError, HickoryDns, Lookup, SrvTarget};
use network::NetworkRule;

use crate::config::UpstreamConfig;

/// Redirect chains longer than this are refused; the providers seen so
/// far need two hops at most.
const MAX_REDIRECTS: usize = 3;

/// One upstream connection, connect and answer included, gets this
/// long; a server slower than that counts as unreachable.
pub const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Error)]
pub enum UpstreamError {
    #[error("cannot read the CA file {path}: {source}")]
    CaFile {
        path: PathBuf,
        #[source]
        source: rustls::pki_types::pem::Error,
    },
    #[error("the CA file {path} holds a certificate that cannot be a root: {source}")]
    CaCertificate {
        path: PathBuf,
        #[source]
        source: rustls::Error,
    },
    #[error("the CA file {path} holds no certificate")]
    CaFileEmpty { path: PathBuf },
    #[error("cannot set up the resolver: {0}")]
    Resolver(#[from] NetError),
    #[error("cannot set up TLS: {0}")]
    Tls(#[from] rustls::Error),
    #[error("cannot set up the HTTP client: {0}")]
    Http(#[from] reqwest::Error),
    #[error("cannot resolve {host}: {source}")]
    Resolve {
        host: String,
        #[source]
        source: DnsError,
    },
    #[error("{host} resolves to {address}, inside a network this instance does not reach")]
    PrivateNetwork { host: String, address: IpAddr },
}

/// The one way out: every outbound connection resolves, checks and
/// trusts the same way.
pub struct Upstream {
    pinned: Pinned,
    tls: Arc<rustls::ClientConfig>,
    http: reqwest::Client,
}

impl Upstream {
    /// On the system resolver.
    ///
    /// # Errors
    ///
    /// Returns an error when the resolver, the CA file or the client
    /// cannot be set up.
    pub fn new(config: &UpstreamConfig) -> Result<Self, UpstreamError> {
        Self::with_dns(config, Arc::new(HickoryDns::from_system()?))
    }

    /// On the given resolver.
    ///
    /// # Errors
    ///
    /// Returns an error when the CA file or the client cannot be set up.
    pub fn with_dns(config: &UpstreamConfig, dns: Arc<dyn Dns>) -> Result<Self, UpstreamError> {
        let pinned = Pinned {
            dns,
            rule: NetworkRule::new(&config.allow_private_networks),
        };
        let tls = Arc::new(tls_config(config.additional_ca_file.as_deref())?);
        let http = reqwest::Client::builder()
            .tls_backend_preconfigured((*tls).clone())
            .https_only(true)
            .no_proxy()
            .connect_timeout(ATTEMPT_TIMEOUT)
            .timeout(ATTEMPT_TIMEOUT)
            .redirect(redirect_policy())
            .dns_resolver(Arc::new(pinned.clone()))
            .build()?;
        Ok(Self { pinned, tls, http })
    }

    #[must_use]
    pub fn dns(&self) -> &dyn Dns {
        self.pinned.dns.as_ref()
    }

    /// The HTTPS client: TLS only, pinned addresses, twenty seconds per
    /// request, three redirects at most and only to named hosts.
    #[must_use]
    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    /// The trust every outbound connection validates against: the
    /// built-in roots plus the CA file.
    #[must_use]
    pub fn tls(&self) -> Arc<rustls::ClientConfig> {
        Arc::clone(&self.tls)
    }

    /// The addresses of `host` on `port`, each checked against the
    /// network rule, so a connect can pin them.
    ///
    /// # Errors
    ///
    /// Returns an error when the lookup fails or any address lies inside
    /// a network this instance does not reach.
    pub async fn resolve(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, UpstreamError> {
        let addresses = self.pinned.lookup(host).await?;
        Ok(addresses
            .into_iter()
            .map(|address| {
                let port = if address.port() == dns::URL_PORT {
                    port
                } else {
                    address.port()
                };
                SocketAddr::new(address.ip(), port)
            })
            .collect())
    }
}

/// The built-in roots plus the instance's CA file.
fn tls_config(ca_file: Option<&Path>) -> Result<rustls::ClientConfig, UpstreamError> {
    let mut roots = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    if let Some(path) = ca_file {
        add_ca_file(&mut roots, path)?;
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    Ok(rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_root_certificates(roots)
        .with_no_client_auth())
}

fn add_ca_file(roots: &mut RootCertStore, path: &Path) -> Result<(), UpstreamError> {
    let read_error = |source| UpstreamError::CaFile {
        path: path.to_owned(),
        source,
    };
    let before = roots.len();
    for certificate in CertificateDer::pem_file_iter(path).map_err(read_error)? {
        roots
            .add(certificate.map_err(read_error)?)
            .map_err(|source| UpstreamError::CaCertificate {
                path: path.to_owned(),
                source,
            })?;
    }
    if roots.len() == before {
        return Err(UpstreamError::CaFileEmpty {
            path: path.to_owned(),
        });
    }
    Ok(())
}

/// Follows a redirect only to another HTTPS URL on a named host and only
/// three times, so every hop resolves through the pinned resolver.
fn redirect_policy() -> redirect::Policy {
    redirect::Policy::custom(|attempt| {
        let named = matches!(attempt.url().host(), Some(Host::Domain(_)));
        if attempt.previous().len() > MAX_REDIRECTS {
            attempt.error("more redirects than allowed")
        } else if attempt.url().scheme() != "https" {
            attempt.error("a redirect away from https")
        } else if !named {
            attempt.error("a redirect to an address literal")
        } else {
            attempt.follow()
        }
    })
}

/// The body up to `limit` bytes; `None` when it is longer or fails to
/// read.
pub(crate) async fn read_bounded(mut response: reqwest::Response, limit: usize) -> Option<Vec<u8>> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if body.len() + chunk.len() > limit {
            return None;
        }
        body.extend_from_slice(&chunk);
    }
    Some(body)
}

/// Resolves through the one resolver and refuses every address the rule
/// denies, so a connect never reaches a private network by way of a
/// name.
#[derive(Clone)]
struct Pinned {
    dns: Arc<dyn Dns>,
    rule: NetworkRule,
}

impl Pinned {
    /// The addresses as the resolver answers them; a test double may
    /// name a port of its own.
    async fn lookup(&self, host: &str) -> Result<Vec<SocketAddr>, UpstreamError> {
        let addresses =
            self.dns
                .addresses(host)
                .await
                .map_err(|source| UpstreamError::Resolve {
                    host: host.to_owned(),
                    source,
                })?;
        if let Some(denied) = addresses
            .iter()
            .find(|address| !self.rule.permits(address.ip()))
        {
            return Err(UpstreamError::PrivateNetwork {
                host: host.to_owned(),
                address: denied.ip(),
            });
        }
        Ok(addresses)
    }
}

impl Resolve for Pinned {
    fn resolve(&self, name: Name) -> Resolving {
        let pinned = self.clone();
        Box::pin(async move {
            let addresses = pinned.lookup(name.as_str()).await?;
            let addrs: Addrs = Box::new(addresses.into_iter());
            Ok(addrs)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_attempt_gets_twenty_seconds() {
        assert_eq!(ATTEMPT_TIMEOUT, Duration::from_secs(20));
    }
}
