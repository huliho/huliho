// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The instance secret and the keys derived from it.

use std::path::{Path, PathBuf};

use chacha20poly1305::{Key, KeyInit, XChaCha20Poly1305};
use hkdf::Hkdf;
use sha2::Sha256;
use thiserror::Error;

/// Environment variable carrying the instance secret value directly.
pub const SECRET_VAR: &str = "HULIHO_SECRET";

/// Fewer bytes than this cannot carry enough entropy to protect sessions.
const MIN_SECRET_BYTES: usize = 32;

/// Group and world permission bits; a secret file must carry none of them.
const GROUP_WORLD_BITS: u32 = 0o077;

/// Domain separation labels, one per purpose; a blob sealed under one
/// key opens under no other.
const SESSION_KEY_INFO: &[u8] = b"huliho session store v1";
const CREDENTIAL_KEY_INFO: &[u8] = b"huliho account credentials v1";

/// AEAD key size for XChaCha20-Poly1305.
const KEY_BYTES: usize = 32;

#[derive(Debug, Error)]
pub enum SecretError {
    #[error("no instance secret: set {SECRET_VAR} or auth.secret_file in the config")]
    Missing,
    #[error("cannot read the secret file {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("the secret file {path} is readable by group or world; make it 0600")]
    Permissions { path: PathBuf },
    #[error("the instance secret carries fewer than {MIN_SECRET_BYTES} bytes")]
    TooShort,
}

/// The operator-provided secret every stored key derives from.
pub struct InstanceSecret(Vec<u8>);

impl InstanceSecret {
    /// Reads the secret from `HULIHO_SECRET` or from `file`, in that order.
    ///
    /// # Errors
    ///
    /// Returns an error when neither source is set, the file cannot be
    /// read, the file is readable beyond its owner or the value is too
    /// short.
    pub fn load(file: Option<&Path>) -> Result<Self, SecretError> {
        if let Some(value) = std::env::var_os(SECRET_VAR) {
            return Self::from_bytes(value.into_encoded_bytes());
        }
        match file {
            Some(path) => Self::from_file(path),
            None => Err(SecretError::Missing),
        }
    }

    /// Takes the secret as raw bytes, the form the environment variable
    /// carries.
    ///
    /// # Errors
    ///
    /// Returns [`SecretError::TooShort`] below the minimum length.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, SecretError> {
        if bytes.len() < MIN_SECRET_BYTES {
            return Err(SecretError::TooShort);
        }
        Ok(Self(bytes))
    }

    fn from_file(path: &Path) -> Result<Self, SecretError> {
        let read_error = |source| SecretError::Read {
            path: path.to_owned(),
            source,
        };
        let permissions = std::fs::metadata(path).map_err(read_error)?.permissions();
        if file_mode(&permissions) & GROUP_WORLD_BITS != 0 {
            return Err(SecretError::Permissions {
                path: path.to_owned(),
            });
        }
        let mut bytes = std::fs::read(path).map_err(read_error)?;
        while bytes.last().is_some_and(u8::is_ascii_whitespace) {
            bytes.pop();
        }
        Self::from_bytes(bytes)
    }
}

/// Keys derived from the instance secret, one per purpose.
pub struct Keys {
    sessions: XChaCha20Poly1305,
    credentials: XChaCha20Poly1305,
}

impl Keys {
    /// Derives every key with HKDF-SHA256 under its own label.
    ///
    /// # Panics
    ///
    /// Only if the fixed key length fell outside the HKDF output bound,
    /// which it does not.
    #[must_use]
    pub fn derive(secret: &InstanceSecret) -> Self {
        let hkdf = Hkdf::<Sha256>::new(None, &secret.0);
        Self {
            sessions: cipher(&hkdf, SESSION_KEY_INFO),
            credentials: cipher(&hkdf, CREDENTIAL_KEY_INFO),
        }
    }

    /// The session store key.
    pub(crate) fn sessions(&self) -> &XChaCha20Poly1305 {
        &self.sessions
    }

    /// The key for the credential sealed on an account row.
    pub(crate) fn credentials(&self) -> &XChaCha20Poly1305 {
        &self.credentials
    }
}

fn cipher(hkdf: &Hkdf<Sha256>, info: &[u8]) -> XChaCha20Poly1305 {
    let mut key = [0u8; KEY_BYTES];
    hkdf.expand(info, &mut key)
        .expect("the fixed key length is within the HKDF output bound");
    XChaCha20Poly1305::new(&Key::from(key))
}

#[cfg(unix)]
fn file_mode(permissions: &std::fs::Permissions) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    permissions.mode()
}

#[cfg(not(unix))]
fn file_mode(_permissions: &std::fs::Permissions) -> u32 {
    0
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    const TEST_SECRET: &[u8] = b"0123456789abcdef0123456789abcdef";

    fn secret_file(mode: u32, content: &[u8]) -> (tempfile::TempDir, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret");
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(content).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        (dir, path)
    }

    #[test]
    fn a_private_file_loads_without_its_trailing_newline() {
        let mut content = TEST_SECRET.to_vec();
        content.push(b'\n');
        let (_dir, path) = secret_file(0o600, &content);
        let secret = InstanceSecret::from_file(&path).unwrap();
        assert_eq!(secret.0, TEST_SECRET);
    }

    #[test]
    fn a_group_readable_file_is_rejected() {
        let (_dir, path) = secret_file(0o640, TEST_SECRET);
        let result = InstanceSecret::from_file(&path);
        assert!(matches!(result, Err(SecretError::Permissions { .. })));
    }

    #[test]
    fn a_short_secret_is_rejected() {
        let (_dir, path) = secret_file(0o600, b"short");
        let result = InstanceSecret::from_file(&path);
        assert!(matches!(result, Err(SecretError::TooShort)));
    }

    #[test]
    fn an_empty_secret_is_too_short() {
        assert!(matches!(
            InstanceSecret::from_bytes(Vec::new()),
            Err(SecretError::TooShort)
        ));
    }
}
