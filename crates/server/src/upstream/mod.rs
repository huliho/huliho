// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Reaching mail servers: one resolver, one network rule and one TLS
//! configuration behind every outbound connection.

mod dns;
mod network;

use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use hickory_resolver::net::NetError;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::redirect;
use rustls::RootCertStore;
use rustls::pki_types::CertificateDer;
use rustls::pki_types::pem::PemObject;
use thiserror::Error;
use url::Host;

pub use dns::{Dns, DnsError, HickoryDns, Lookup, SrvTarget};
pub use network::NetworkRule;

use crate::config::UpstreamConfig;

/// Redirect chains longer than this are refused; the providers seen so
/// far need two hops at most.
const MAX_REDIRECTS: usize = 3;

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
    #[error("{host} resolves to {address}, inside a network this instance does not reach")]
    PrivateNetwork { host: String, address: IpAddr },
}

/// The one way out: every outbound connection resolves, checks and
/// trusts the same way.
pub struct Upstream {
    dns: Arc<dyn Dns>,
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
        let resolver = PinnedResolver {
            dns: Arc::clone(&dns),
            rule: NetworkRule::new(&config.allow_private_networks),
        };
        let http = reqwest::Client::builder()
            .tls_backend_preconfigured(tls_config(config.additional_ca_file.as_deref())?)
            .https_only(true)
            .no_proxy()
            .redirect(redirect_policy())
            .dns_resolver(Arc::new(resolver))
            .build()?;
        Ok(Self { dns, http })
    }

    #[must_use]
    pub fn dns(&self) -> &dyn Dns {
        self.dns.as_ref()
    }

    /// The HTTPS client: TLS only, pinned addresses, three redirects at
    /// most and only to named hosts.
    #[must_use]
    pub fn http(&self) -> &reqwest::Client {
        &self.http
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

/// Resolves through the one resolver and refuses every address the rule
/// denies, so a connect never reaches a private network by way of a
/// name.
struct PinnedResolver {
    dns: Arc<dyn Dns>,
    rule: NetworkRule,
}

impl Resolve for PinnedResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let dns = Arc::clone(&self.dns);
        let rule = self.rule.clone();
        Box::pin(async move {
            let host = name.as_str().to_owned();
            let addresses = dns.addresses(&host).await?;
            if let Some(denied) = addresses.iter().find(|address| !rule.permits(address.ip())) {
                let refused = UpstreamError::PrivateNetwork {
                    host,
                    address: denied.ip(),
                };
                return Err(refused.into());
            }
            let addrs: Addrs = Box::new(addresses.into_iter());
            Ok(addrs)
        })
    }
}
