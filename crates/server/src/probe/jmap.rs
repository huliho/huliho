// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The JMAP session resource as the credential check (RFC 8620
//! section 2).

use std::collections::BTreeSet;
use std::error::Error as _;
use std::fmt;
use std::io;

use reqwest::StatusCode;
use serde::de::{IgnoredAny, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use url::Url;

use super::ProbeError;
use crate::accounts::Credential;
use crate::discovery::Address;
use crate::upstream::{Upstream, read_bounded};

/// The capability every mail account needs (RFC 8621 section 1.1).
const MAIL_CAPABILITY: &str = "urn:ietf:params:jmap:mail";

/// A session object runs to a few kilobytes; more is not one.
const MAX_SESSION_BYTES: usize = 64 * 1024;

/// The part of the session object the check reads.
#[derive(Deserialize)]
struct SessionObject {
    #[serde(deserialize_with = "capability_names")]
    capabilities: BTreeSet<String>,
}

/// The keys of the capabilities object; what each one says is not read.
fn capability_names<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeSet<String>, D::Error> {
    struct Names;

    impl<'de> Visitor<'de> for Names {
        type Value = BTreeSet<String>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("an object keyed by capability")
        }

        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut names = BTreeSet::new();
            while let Some((name, IgnoredAny)) = map.next_entry::<String, IgnoredAny>()? {
                names.insert(name);
            }
            Ok(names)
        }
    }

    deserializer.deserialize_map(Names)
}

/// GET on the session URL with the credential; a pass is a session
/// object that advertises mail.
pub(super) async fn check(
    upstream: &Upstream,
    session_url: &Url,
    address: &Address,
    credential: &Credential,
) -> Result<(), ProbeError> {
    let request = upstream.http().get(session_url.clone());
    let request = match credential {
        Credential::Password { password } => {
            request.basic_auth(address.to_string(), Some(password))
        }
        Credential::Bearer { token } => request.bearer_auth(token),
    };
    let response = request
        .send()
        .await
        .map_err(|error| request_error(&error))?;
    match response.status() {
        StatusCode::OK => {}
        StatusCode::UNAUTHORIZED => return Err(ProbeError::CredentialRejected),
        status => {
            return Err(ProbeError::Unsupported(format!(
                "the session resource answered {status}"
            )));
        }
    }
    let body = read_bounded(response, MAX_SESSION_BYTES)
        .await
        .ok_or_else(|| {
            ProbeError::Unsupported("the session resource answered too much".to_owned())
        })?;
    let session: SessionObject = serde_json::from_slice(&body)
        .map_err(|_| ProbeError::Unsupported("the answer is not a session object".to_owned()))?;
    if session.capabilities.contains(MAIL_CAPABILITY) {
        Ok(())
    } else {
        Err(ProbeError::Unsupported(
            "the server offers no mail capability".to_owned(),
        ))
    }
}

/// The client's error in fixed words; its own text names the URL.
fn request_error(error: &reqwest::Error) -> ProbeError {
    if error.is_timeout() {
        return ProbeError::Unreachable("the server took too long".to_owned());
    }
    if error.is_redirect() {
        return ProbeError::Unsupported("the session resource redirects away".to_owned());
    }
    if error.is_connect() && refused_by_tls(error) {
        return ProbeError::Insecure("the certificate was refused".to_owned());
    }
    ProbeError::Unreachable("cannot connect".to_owned())
}

/// True when what rustls refused sits in the chain: the TLS layer hands
/// it up as invalid data and the client wraps that once more.
fn refused_by_tls(error: &reqwest::Error) -> bool {
    let mut source = error.source();
    while let Some(inner) = source {
        if inner.downcast_ref::<io::Error>().is_some_and(invalid_data) {
            return true;
        }
        source = inner.source();
    }
    false
}

fn invalid_data(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::InvalidData
        || error
            .get_ref()
            .and_then(|inner| inner.downcast_ref::<io::Error>())
            .is_some_and(invalid_data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_data_is_found_at_any_depth() {
        let refused = io::Error::new(io::ErrorKind::InvalidData, "unknown issuer");
        let wrapped = io::Error::other(refused);
        assert!(invalid_data(&wrapped));
        let closed = io::Error::from(io::ErrorKind::ConnectionRefused);
        assert!(!invalid_data(&io::Error::other(closed)));
    }

    #[test]
    fn a_session_object_parses_its_capabilities_only() {
        let text = concat!(
            r#"{"capabilities":{"urn:ietf:params:jmap:core":{"maxSizeUpload":1},"#,
            r#""urn:ietf:params:jmap:mail":{}},"accounts":{"a":{}}}"#
        );
        let session: SessionObject = serde_json::from_str(text).unwrap();
        assert!(session.capabilities.contains(MAIL_CAPABILITY));
        assert_eq!(session.capabilities.len(), 2);
        assert!(serde_json::from_str::<SessionObject>("<html>").is_err());
    }
}
