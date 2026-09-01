// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Instance configuration from one TOML file.

use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

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

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    pub listen: SocketAddr,
    pub assets: PathBuf,
    pub storage: StorageConfig,
    pub events: EventsConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen: DEFAULT_LISTEN,
            assets: PathBuf::from(DEFAULT_ASSETS),
            storage: StorageConfig::default(),
            events: EventsConfig::default(),
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
        let toml = "listen = \"0.0.0.0:9000\"\nassets = \"web\"\n\n[storage]\npath = \"volume\"\n\n[events]\nretention_days = 30";
        let config = Config::parse(Path::new(ABSENT_PATH), toml).unwrap();
        assert_eq!(config.listen, "0.0.0.0:9000".parse().unwrap());
        assert_eq!(config.assets, PathBuf::from("web"));
        assert_eq!(config.storage.path, PathBuf::from("volume"));
        assert_eq!(config.events.retention_days, 30);
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
    fn malformed_toml_is_rejected() {
        assert!(Config::parse(Path::new(ABSENT_PATH), "listen = ").is_err());
    }
}
