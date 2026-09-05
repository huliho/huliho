// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The secret an account signs in with, sealed under the account id so
//! a blob opens on no other row.

use std::fmt;

use serde::{Deserialize, Serialize};

use super::AuthMethod;
use crate::ids::AccountId;
use crate::sealed;
use crate::secrets::Keys;
use crate::store::StoreError;

/// What the upstream sign-in takes; the shape inside the sealed blob.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Credential {
    Password { password: String },
    Bearer { token: String },
}

impl Credential {
    #[must_use]
    pub fn auth_method(&self) -> AuthMethod {
        match self {
            Self::Password { .. } => AuthMethod::Password,
            Self::Bearer { .. } => AuthMethod::Bearer,
        }
    }
}

/// Prints the kind only, so a credential in a log line stays one word.
impl fmt::Debug for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Credential({})", self.auth_method().as_str())
    }
}

pub(super) fn seal(
    keys: &Keys,
    account_id: &AccountId,
    credential: &Credential,
) -> Result<Vec<u8>, StoreError> {
    let plaintext = serde_json::to_vec(credential)?;
    sealed::seal(
        keys.credentials(),
        account_id.as_str().as_bytes(),
        &plaintext,
    )
}

pub(super) fn open(keys: &Keys, account_id: &AccountId, blob: &[u8]) -> Option<Credential> {
    let plaintext = sealed::open(keys.credentials(), account_id.as_str().as_bytes(), blob)?;
    serde_json::from_slice(&plaintext).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::InstanceSecret;

    const PASSWORD: &str = "hunter2 but longer";

    fn keys() -> Keys {
        Keys::derive(
            &InstanceSecret::from_bytes(b"0123456789abcdef0123456789abcdef".to_vec()).unwrap(),
        )
    }

    fn password() -> Credential {
        Credential::Password {
            password: PASSWORD.to_owned(),
        }
    }

    fn account(id: &str) -> AccountId {
        AccountId::from(id.to_owned())
    }

    #[test]
    fn another_account_id_opens_nothing() {
        let keys = keys();
        let sealed = seal(&keys, &account("a"), &password()).unwrap();
        assert_eq!(open(&keys, &account("b"), &sealed), None);
    }

    #[test]
    fn the_blob_holds_no_plaintext() {
        let sealed = seal(&keys(), &account("a"), &password()).unwrap();
        assert!(
            !sealed
                .windows(PASSWORD.len())
                .any(|window| window == PASSWORD.as_bytes())
        );
    }

    #[test]
    fn debug_output_names_the_kind_only() {
        let printed = format!("{:?}", password());
        assert_eq!(printed, "Credential(password)");
    }

    #[test]
    fn a_credential_round_trips_under_its_account_id() {
        let keys = keys();
        let sealed = seal(&keys, &account("a"), &password()).unwrap();
        assert_eq!(open(&keys, &account("a"), &sealed), Some(password()));
        assert_eq!(password().auth_method(), AuthMethod::Password);
    }

    #[test]
    fn the_kinds_serialize_to_their_words() {
        let bearer = Credential::Bearer {
            token: "t".to_owned(),
        };
        assert_eq!(
            serde_json::to_string(&bearer).unwrap(),
            r#"{"kind":"bearer","token":"t"}"#
        );
        assert_eq!(bearer.auth_method(), AuthMethod::Bearer);
        assert_eq!(
            serde_json::to_string(&password()).unwrap(),
            format!(r#"{{"kind":"password","password":"{PASSWORD}"}}"#)
        );
    }
}
