// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Instance configuration from one TOML file.

use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use ipnet::IpNet;
use serde::Deserialize;
use thiserror::Error;
use url::Url;

/// Environment variable naming the config file path; a named file must exist.
pub const CONFIG_PATH_VAR: &str = "HULIHO_CONFIG";

/// Read from the working directory when `HULIHO_CONFIG` is unset.
pub const DEFAULT_CONFIG_PATH: &str = "huliho.toml";

/// Unprivileged port, so a bare start needs no root.
const DEFAULT_PORT: u16 = 8080;

/// Loopback, so a bare start exposes nothing beyond the host.
const DEFAULT_LISTEN: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), DEFAULT_PORT);

/// The Vite build output, where a workspace-root start finds the SPA.
const DEFAULT_ASSETS: &str = "apps/web/dist";

/// The data volume, where all persistent state lives.
const DEFAULT_STORAGE_PATH: &str = "data";

/// One year of lifecycle facts by default.
const DEFAULT_EVENT_RETENTION_DAYS: u32 = 365;

/// Minutes per day, for the timeout defaults below.
const MINUTES_PER_DAY: u32 = 24 * 60;

/// A mail client stays signed in; two quiet weeks end a session.
const DEFAULT_IDLE_TIMEOUT_MINUTES: u32 = 14 * MINUTES_PER_DAY;

/// Ninety days ends a session outright, active or not.
const DEFAULT_ABSOLUTE_TIMEOUT_MINUTES: u32 = 90 * MINUTES_PER_DAY;

/// Fifteen minutes between checks on an account that stopped on refused
/// connections: soon enough to notice a recovered server, rare enough to
/// stay polite to one that is down. Evaluated at compile time.
const DEFAULT_PROBE_INTERVAL_MINUTES: NonZeroU32 = NonZeroU32::new(15).unwrap();

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    pub listen: SocketAddr,
    pub assets: PathBuf,
    /// The base URL users reach the instance on.
    pub public_url: Option<Url>,
    pub storage: StorageConfig,
    pub events: EventsConfig,
    pub auth: AuthConfig,
    pub upstream: UpstreamConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen: DEFAULT_LISTEN,
            assets: PathBuf::from(DEFAULT_ASSETS),
            public_url: None,
            storage: StorageConfig::default(),
            events: EventsConfig::default(),
            auth: AuthConfig::default(),
            upstream: UpstreamConfig::default(),
        }
    }
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct StorageConfig {
    pub path: PathBuf,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            path: PathBuf::from(DEFAULT_STORAGE_PATH),
        }
    }
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct EventsConfig {
    pub retention_days: u32,
}

impl Default for EventsConfig {
    fn default() -> Self {
        Self {
            retention_days: DEFAULT_EVENT_RETENTION_DAYS,
        }
    }
}

/// Session settings; the secret itself comes from the environment or the
/// named file, never from this config file.
#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct AuthConfig {
    pub secret_file: Option<PathBuf>,
    pub idle_timeout_minutes: u32,
    pub absolute_timeout_minutes: u32,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            secret_file: None,
            idle_timeout_minutes: DEFAULT_IDLE_TIMEOUT_MINUTES,
            absolute_timeout_minutes: DEFAULT_ABSOLUTE_TIMEOUT_MINUTES,
        }
    }
}

/// Rules for reaching upstream mail servers. Certificate validation is
/// never optional; the CA file only widens what counts as valid.
#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct UpstreamConfig {
    /// Private networks an upstream may resolve to; none by default.
    pub allow_private_networks: Vec<IpNet>,
    /// One PEM bundle trusted next to the built-in roots.
    pub additional_ca_file: Option<PathBuf>,
    pub probe_interval_minutes: NonZeroU32,
}

impl Default for UpstreamConfig {
    fn default() -> Self {
        Self {
            allow_private_networks: Vec::new(),
            additional_ca_file: None,
            probe_interval_minutes: DEFAULT_PROBE_INTERVAL_MINUTES,
        }
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("cannot read config file {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid config file {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
}

impl Config {
    /// Loads the config file at `path`.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be read (a missing file
    /// counts) or does not parse as valid config.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(path, &text),
            Err(source) => Err(ConfigError::Read {
                path: path.to_owned(),
                source,
            }),
        }
    }

    /// Loads the config file when it exists; a missing file yields the
    /// defaults.
    ///
    /// # Errors
    ///
    /// Returns an error when the file exists but cannot be read or does
    /// not parse as valid config.
    pub fn load_or_default(path: &Path) -> Result<Self, ConfigError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(path, &text),
            Err(source) if source.kind() == ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(ConfigError::Read {
                path: path.to_owned(),
                source,
            }),
        }
    }

    fn parse(path: &Path, text: &str) -> Result<Self, ConfigError> {
        toml::from_str(text).map_err(|source| ConfigError::Parse {
            path: path.to_owned(),
            source,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ABSENT_PATH: &str = "does-not-exist.toml";

    #[test]
    fn missing_default_file_yields_defaults() {
        let config = Config::load_or_default(Path::new(ABSENT_PATH)).unwrap();
        assert_eq!(config, Config::default());
    }

    #[test]
    fn missing_named_file_is_an_error_carrying_the_path() {
        let error = Config::load(Path::new(ABSENT_PATH)).unwrap_err();
        assert!(error.to_string().contains(ABSENT_PATH));
    }

    #[test]
    fn values_override_defaults() {
        let toml = "listen = \"0.0.0.0:9000\"\nassets = \"web\"\npublic_url = \"https://mail.example.com\"\n\n[storage]\npath = \"volume\"\n\n[events]\nretention_days = 30\n\n[auth]\nsecret_file = \"secret\"\nidle_timeout_minutes = 30\nabsolute_timeout_minutes = 60\n\n[upstream]\nallow_private_networks = [\"127.0.0.0/8\", \"::1/128\"]\nadditional_ca_file = \"data/dev-certs/ca.pem\"\nprobe_interval_minutes = 5";
        let config = Config::parse(Path::new(ABSENT_PATH), toml).unwrap();
        assert_eq!(config.listen, "0.0.0.0:9000".parse().unwrap());
        assert_eq!(config.assets, PathBuf::from("web"));
        assert_eq!(
            config.public_url,
            Some(Url::parse("https://mail.example.com").unwrap())
        );
        assert_eq!(config.storage.path, PathBuf::from("volume"));
        assert_eq!(config.events.retention_days, 30);
        assert_eq!(config.auth.secret_file, Some(PathBuf::from("secret")));
        assert_eq!(config.auth.idle_timeout_minutes, 30);
        assert_eq!(config.auth.absolute_timeout_minutes, 60);
        let loopback: [IpNet; 2] = ["127.0.0.0/8".parse().unwrap(), "::1/128".parse().unwrap()];
        assert_eq!(config.upstream.allow_private_networks, loopback);
        assert_eq!(
            config.upstream.additional_ca_file,
            Some(PathBuf::from("data/dev-certs/ca.pem"))
        );
        assert_eq!(config.upstream.probe_interval_minutes.get(), 5);
    }

    #[test]
    fn upstream_defaults_trust_nothing_extra() {
        let upstream = Config::default().upstream;
        assert!(upstream.allow_private_networks.is_empty());
        assert!(upstream.additional_ca_file.is_none());
        assert_eq!(
            upstream.probe_interval_minutes,
            DEFAULT_PROBE_INTERVAL_MINUTES
        );
    }

    #[test]
    fn public_url_without_a_scheme_is_rejected() {
        let toml = "public_url = \"mail.example.com\"";
        assert!(Config::parse(Path::new(ABSENT_PATH), toml).is_err());
    }

    #[test]
    fn private_network_without_a_prefix_length_is_rejected() {
        let toml = "[upstream]\nallow_private_networks = [\"10.0.0.1\"]";
        assert!(Config::parse(Path::new(ABSENT_PATH), toml).is_err());
    }

    #[test]
    fn probe_interval_of_zero_is_rejected() {
        let toml = "[upstream]\nprobe_interval_minutes = 0";
        assert!(Config::parse(Path::new(ABSENT_PATH), toml).is_err());
    }

    #[test]
    fn unknown_field_is_rejected() {
        assert!(Config::parse(Path::new(ABSENT_PATH), "surprise = true").is_err());
    }

    #[test]
    fn unknown_nested_field_is_rejected() {
        assert!(Config::parse(Path::new(ABSENT_PATH), "[storage]\nsurprise = true").is_err());
    }

    #[test]
    fn unknown_upstream_field_is_rejected() {
        assert!(Config::parse(Path::new(ABSENT_PATH), "[upstream]\nsurprise = true").is_err());
    }

    #[test]
    fn malformed_toml_is_rejected() {
        assert!(Config::parse(Path::new(ABSENT_PATH), "listen = ").is_err());
    }
}
