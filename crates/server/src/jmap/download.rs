// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The native side of the download route: the template the upstream
//! session object named, expanded with the blob's coordinates, checked
//! like every upstream URL and fetched with the account's credential,
//! its first bytes read. The rest streams on from here; the route
//! decides what the bytes are.

use percent_encoding::utf8_percent_encode;
use reqwest::StatusCode;
use thiserror::Error;

use super::{Proxy, Stored, unsupported};
use crate::accounts::Account;
use crate::gate::{AttemptError, Fault};
use crate::mail::download::{
    BLOB_DOWNLOAD_LIMIT, DOWNLOAD_IDLE_TIMEOUT, DOWNLOAD_TOTAL_TIMEOUT, ENCODED,
};
use crate::mail::stream::{self, BOUNDS, Blob, Source};
use crate::probe::{self, ProbeError};
use crate::scope::Scope;

/// The variables a download template carries (RFC 8620 section 2), in
/// the order [`BlobAsk::values`] answers them.
const VARIABLES: [&str; 4] = ["accountId", "blobId", "name", "type"];

/// The blob the route asks for, in the upstream's terms.
#[derive(Debug, Clone, Copy)]
pub struct BlobAsk<'a> {
    /// The upstream account id.
    pub account_id: &'a str,
    pub blob_id: &'a str,
    pub name: &'a str,
    /// The type the client expects back, empty when it named none.
    pub media_type: &'a str,
}

impl BlobAsk<'_> {
    fn values(&self) -> [&str; 4] {
        [self.account_id, self.blob_id, self.name, self.media_type]
    }
}

/// Why a blob did not come back.
#[derive(Debug, Error)]
pub enum BlobError {
    /// The upstream holds no blob of that id in that account.
    #[error("the blob is not there")]
    NotFound,
    /// The upstream declares more bytes than the route carries.
    #[error("the blob is larger than the route carries")]
    TooLarge,
    #[error(transparent)]
    Attempt(#[from] AttemptError),
}

impl BlobError {
    /// The rule the failure falls under; a blob that is not there or
    /// too large says nothing about the account.
    fn fault(&self) -> Fault {
        match self {
            Self::NotFound | Self::TooLarge => Fault::Undecided,
            Self::Attempt(error) => error.fault(),
        }
    }
}

impl Proxy {
    /// The upstream's blob with its status judged, its declared size
    /// within the bound and its first bytes in hand. The gate sees the
    /// outcome, a body that ends before its first bytes as a connection
    /// fault. The caller read the row as running.
    ///
    /// # Errors
    ///
    /// Returns [`BlobError::NotFound`] for a blob the upstream does not
    /// hold, [`BlobError::TooLarge`] for one declared past the bound
    /// and otherwise as [`Proxy::session`], plus
    /// [`ProbeError::Unsupported`] for a template the proxy cannot
    /// expand.
    pub async fn blob(
        &self,
        account: Account,
        scope: &Scope,
        ask: &BlobAsk<'_>,
    ) -> Result<Blob, BlobError> {
        let stored = self.read(scope, account).await?;
        let outcome = self.fetch_blob(&stored, scope, ask).await;
        self.observed(scope, outcome.as_ref().err().map(BlobError::fault))
            .await?;
        outcome
    }

    async fn fetch_blob(
        &self,
        stored: &Stored,
        scope: &Scope,
        ask: &BlobAsk<'_>,
    ) -> Result<Blob, BlobError> {
        let credential = self
            .live_credential(scope, stored.credential.clone())
            .await?;
        let template = self
            .upstream_urls(stored, &credential)
            .await?
            .download_url
            .ok_or_else(|| unsupported("the session object names no download template"))?;
        let url = self
            .checked_endpoint(&stored.session_url, &expand(&template, ask)?)
            .await?;
        let request = self
            .wiring
            .upstream
            .http()
            .get(url)
            .timeout(DOWNLOAD_TOTAL_TIMEOUT);
        // The total covers the whole download; the headers get the same
        // patience as one chunk of the body.
        let response = tokio::time::timeout(
            DOWNLOAD_IDLE_TIMEOUT,
            probe::send(request, &stored.address, &credential),
        )
        .await
        .unwrap_or_else(|_elapsed| {
            Err(ProbeError::Unreachable(
                "the blob endpoint took too long to answer".to_owned(),
            ))
        })
        .map_err(AttemptError::from)?;
        let status = response.status();
        if status != StatusCode::OK {
            tracing::debug!(
                account = stored.account_id.as_str(),
                status = status.as_u16(),
                "the upstream answered a blob request with an error"
            );
        }
        match status {
            StatusCode::OK => {}
            StatusCode::NOT_FOUND => return Err(BlobError::NotFound),
            StatusCode::UNAUTHORIZED => {
                return Err(AttemptError::from(ProbeError::CredentialRejected).into());
            }
            status if status.is_server_error() => {
                return Err(AttemptError::Failed(status).into());
            }
            status => {
                return Err(unsupported(format!("the blob endpoint answered {status}")).into());
            }
        }
        if response
            .content_length()
            .is_some_and(|declared| declared > BLOB_DOWNLOAD_LIMIT)
        {
            return Err(BlobError::TooLarge);
        }
        stream::open(Source::Upstream(response), &BOUNDS)
            .await
            .map_err(|_| {
                AttemptError::from(ProbeError::Unreachable("the blob ended early".to_owned()))
                    .into()
            })
    }
}

/// The template with its four variables expanded (RFC 6570 level 1),
/// every value percent-encoded outside the unreserved set.
///
/// # Errors
///
/// Returns [`ProbeError::Unsupported`] for a template that lacks a
/// variable, names another or leaves a brace open.
pub fn expand(template: &str, ask: &BlobAsk<'_>) -> Result<String, AttemptError> {
    let values = ask.values();
    let mut seen = [false; VARIABLES.len()];
    let mut expanded = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let close = rest[open..]
            .find('}')
            .map(|close| open + close)
            .ok_or_else(|| unsupported("the download template leaves a brace open"))?;
        let name = &rest[open + 1..close];
        let index = VARIABLES
            .iter()
            .position(|variable| *variable == name)
            .ok_or_else(|| unsupported("the download template names an unknown variable"))?;
        seen[index] = true;
        expanded.push_str(&rest[..open]);
        expanded.extend(utf8_percent_encode(values[index], ENCODED));
        rest = &rest[close + 1..];
    }
    if rest.contains('}') || seen.contains(&false) {
        return Err(unsupported("the download template lacks a variable"));
    }
    expanded.push_str(rest);
    Ok(expanded)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    const TEMPLATE: &str =
        "https://api.example.test/jmap/download/{accountId}/{blobId}/{name}?accept={type}";

    fn ask() -> BlobAsk<'static> {
        BlobAsk {
            account_id: "u1",
            blob_id: "G4c6f/bA",
            name: "report Q3 été.pdf",
            media_type: "application/pdf",
        }
    }

    #[test]
    fn a_template_expands_with_every_value_encoded_rfc6570_3_2_2() {
        assert_eq!(
            expand(TEMPLATE, &ask()).unwrap(),
            "https://api.example.test/jmap/download/u1/G4c6f%2FbA/report%20Q3%20%C3%A9t%C3%A9.pdf?accept=application%2Fpdf"
        );
        let hostile = BlobAsk {
            blob_id: "../../api/session",
            name: "a?b=c#d",
            ..ask()
        };
        let expanded = expand(TEMPLATE, &hostile).unwrap();
        assert!(!expanded.contains("../"), "{expanded}");
        assert!(
            expanded.contains("/u1/..%2F..%2Fapi%2Fsession/a%3Fb%3Dc%23d?"),
            "{expanded}"
        );
    }

    #[test]
    fn a_template_missing_a_variable_naming_another_or_left_open_is_not_usable() {
        for template in [
            "https://api.example.test/jmap/download/{accountId}/{blobId}/{name}",
            "https://api.example.test/jmap/download/{accountId}/{blobId}/{name}?accept={type}&x={extra}",
            "https://api.example.test/jmap/download/{+path}/{blobId}/{name}?accept={type}",
            "https://api.example.test/jmap/download/{accountId/{blobId}/{name}?accept={type}",
            "https://api.example.test/jmap/download/{accountId}/{blobId}/{name}?accept={type}}",
        ] {
            assert!(
                matches!(
                    expand(template, &ask()),
                    Err(AttemptError::Upstream(ProbeError::Unsupported(_)))
                ),
                "{template}"
            );
        }
    }

    #[test]
    fn a_blob_that_is_not_there_or_too_large_decides_nothing_at_the_gate() {
        assert_eq!(BlobError::NotFound.fault(), Fault::Undecided);
        assert_eq!(BlobError::TooLarge.fault(), Fault::Undecided);
        let refused = BlobError::from(AttemptError::from(ProbeError::CredentialRejected));
        assert_eq!(refused.fault(), Fault::Credential);
        let cut = BlobError::from(AttemptError::from(ProbeError::Unreachable(String::new())));
        assert_eq!(cut.fault(), Fault::Connection);
    }

    /// One literal between the four variables, so every value sits
    /// between separators an encoded value never carries.
    const SLOTTED: &str = "{accountId}/{blobId}/{name}?{type}";

    /// Whether an expansion of [`SLOTTED`] kept its three separators
    /// with only unreserved bytes (RFC 3986 section 2.3) or escapes in
    /// each slot.
    fn slotted_cleanly(expanded: &str) -> bool {
        let separators: String = expanded.matches(['/', '?']).collect();
        separators == "//?"
            && expanded.split(['/', '?']).all(|slot| {
                slot.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'%')
                })
            })
    }

    fn ask_of(values: &[String; 4]) -> BlobAsk<'_> {
        let [account_id, blob_id, name, media_type] = values;
        BlobAsk {
            account_id,
            blob_id,
            name,
            media_type,
        }
    }

    /// Templates built from the variables, stray braces and literal
    /// text.
    fn templates() -> impl Strategy<Value = String> {
        let piece = prop_oneof![
            prop::sample::select(VARIABLES.to_vec()).prop_map(|variable| format!("{{{variable}}}")),
            "[a-z/?=.:{}]{0,8}",
        ];
        prop::collection::vec(piece, 0..12).prop_map(|pieces| pieces.concat())
    }

    /// Values as any string or as one loaded with the separators and
    /// braces a template carries.
    fn values() -> impl Strategy<Value = [String; 4]> {
        prop::array::uniform4(prop_oneof![any::<String>(), "[a-z /?#=&%{}.~-]{0,10}"])
    }

    proptest! {
        #[test]
        fn any_template_expands_or_is_refused_without_a_panic(
            template in templates(),
            values in values(),
        ) {
            let _ = expand(&template, &ask_of(&values));
        }

        #[test]
        fn every_value_stays_in_its_slot_rfc3986_2_3(values in values()) {
            let expanded = expand(SLOTTED, &ask_of(&values)).unwrap();
            prop_assert!(slotted_cleanly(&expanded), "{expanded}");
        }
    }
}
