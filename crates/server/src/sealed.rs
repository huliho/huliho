// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Seals what a row keeps private with XChaCha20-Poly1305, the nonce in
//! front and the row's identity as associated data.

use chacha20poly1305::aead::{Aead, Generate, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};

use crate::store::StoreError;

/// XChaCha20-Poly1305 prefixes its nonce to the sealed blob.
const NONCE_BYTES: usize = 24;

/// Seals `plaintext` under `cipher`, bound to `associated` so the blob
/// opens on no other row.
pub(crate) fn seal(
    cipher: &XChaCha20Poly1305,
    associated: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>, StoreError> {
    let nonce = XNonce::try_generate().map_err(|_| StoreError::Random)?;
    let ciphertext = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: plaintext,
                aad: associated,
            },
        )
        .map_err(|_| StoreError::Sealing)?;
    let mut sealed = nonce.to_vec();
    sealed.extend_from_slice(&ciphertext);
    Ok(sealed)
}

/// Opens a blob sealed under the same cipher and associated data; a
/// wrong key, another row or a changed byte gives `None`.
pub(crate) fn open(cipher: &XChaCha20Poly1305, associated: &[u8], blob: &[u8]) -> Option<Vec<u8>> {
    let (nonce, ciphertext) = blob.split_at_checked(NONCE_BYTES)?;
    let nonce = XNonce::try_from(nonce).ok()?;
    cipher
        .decrypt(
            &nonce,
            Payload {
                msg: ciphertext,
                aad: associated,
            },
        )
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::{InstanceSecret, Keys};

    const SECRET: &[u8] = b"0123456789abcdef0123456789abcdef";
    const OTHER_SECRET: &[u8] = b"fedcba9876543210fedcba9876543210";

    fn keys() -> Keys {
        Keys::derive(&InstanceSecret::from_bytes(SECRET.to_vec()).unwrap())
    }

    #[test]
    fn another_row_opens_nothing() {
        let keys = keys();
        let sealed = seal(keys.credentials(), b"row", b"secret").unwrap();
        assert_eq!(open(keys.credentials(), b"other row", &sealed), None);
    }

    #[test]
    fn another_purpose_opens_nothing() {
        let keys = keys();
        let sealed = seal(keys.sessions(), b"row", b"secret").unwrap();
        assert_eq!(open(keys.credentials(), b"row", &sealed), None);
    }

    #[test]
    fn another_instance_secret_opens_nothing() {
        let sealed = seal(keys().credentials(), b"row", b"secret").unwrap();
        let other = Keys::derive(&InstanceSecret::from_bytes(OTHER_SECRET.to_vec()).unwrap());
        assert_eq!(open(other.credentials(), b"row", &sealed), None);
    }

    #[test]
    fn a_changed_byte_or_a_short_blob_opens_nothing() {
        let keys = keys();
        let mut sealed = seal(keys.credentials(), b"row", b"secret").unwrap();
        let last = sealed.len() - 1;
        sealed[last] ^= 1;
        assert_eq!(open(keys.credentials(), b"row", &sealed), None);
        assert_eq!(
            open(keys.credentials(), b"row", &sealed[..NONCE_BYTES - 1]),
            None
        );
    }

    #[test]
    fn a_blob_opens_under_its_own_key_and_row() {
        let keys = keys();
        let sealed = seal(keys.credentials(), b"row", b"secret").unwrap();
        assert_eq!(
            open(keys.credentials(), b"row", &sealed),
            Some(b"secret".to_vec())
        );
    }

    #[test]
    fn every_seal_uses_a_fresh_nonce() {
        let keys = keys();
        let first = seal(keys.credentials(), b"row", b"secret").unwrap();
        let second = seal(keys.credentials(), b"row", b"secret").unwrap();
        assert_ne!(first, second);
    }
}
