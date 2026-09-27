// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The remote-image proxy: a URL a sender wrote, fetched with nothing of
//! the reader's on the request under the rules every outbound
//! connection follows, every redirect it follows checked the same way,
//! the bytes read within a bound and named by their own signature.

use std::time::Duration;

use reqwest::StatusCode;
use reqwest::header::{LOCATION, USER_AGENT};
use thiserror::Error;
use url::{Host, Url};

use super::detect::raster_type;
use crate::rate::Rate;
use crate::upstream::{BodyError, Upstream, UpstreamError, read_bounded};

/// The bound on a URL's length in bytes; a longer one is refused.
pub const REMOTE_URL_BYTES: usize = 2048;

/// Redirects followed for one image; a CDN needs one or two.
pub const REMOTE_REDIRECTS: usize = 3;

/// One attempt at an image host, connect to last byte; an image slower
/// than this stays blocked.
pub const REMOTE_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(10);

/// One image at most; larger than any picture a newsletter carries.
pub const REMOTE_IMAGE_LIMIT: u64 = 5 * 1024 * 1024;

/// Images fetched and buffered at once, one process wide, each read
/// within the byte bound.
pub const REMOTE_FETCHES_IN_FLIGHT: usize = 32;

/// Images one session may fetch at once: one heavy newsletter.
pub const REMOTE_IMAGE_BURST: u32 = 400;

/// How fast that allowance comes back.
pub const REMOTE_IMAGES_PER_MINUTE: u32 = 120;

/// The allowance of remote images per session.
pub const REMOTE_IMAGE_RATE: Rate = Rate {
    burst: REMOTE_IMAGE_BURST,
    per_minute: REMOTE_IMAGES_PER_MINUTE,
};

/// What an image host learns about the reader: the product's name and
/// nothing else, no version, no browser, no system.
pub const REMOTE_USER_AGENT: &str = "Huliho";

/// An image as the proxy hands it on: the type its bytes carry.
pub struct Image {
    pub media_type: &'static str,
    pub bytes: Vec<u8>,
}

/// Why no image came back.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum RemoteError {
    /// The URL or the target of a followed redirect is off the rules:
    /// its length or its scheme, user information, a host that is no
    /// name or one inside a network this instance does not reach.
    #[error("the URL is off the rules")]
    Refused,
    #[error("the image is larger than the proxy carries")]
    TooLarge,
    #[error("the bytes are no raster image")]
    NotAnImage,
    #[error("the image host answered no image")]
    Unreachable,
}

/// The URL as the proxy fetches it: within the bound, `https` with an
/// `http` scheme raised, without user information, on a named host.
///
/// # Errors
///
/// Returns [`RemoteError::Refused`] for a URL off those rules.
pub fn checked(given: &str) -> Result<Url, RemoteError> {
    let url = Url::parse(given).map_err(|_| RemoteError::Refused)?;
    admitted(url)
}

fn admitted(mut url: Url) -> Result<Url, RemoteError> {
    if url.as_str().len() > REMOTE_URL_BYTES {
        return Err(RemoteError::Refused);
    }
    match url.scheme() {
        "https" => {}
        "http" => url.set_scheme("https").map_err(|()| RemoteError::Refused)?,
        _ => return Err(RemoteError::Refused),
    }
    let anonymous = url.username().is_empty() && url.password().is_none();
    if !anonymous || !matches!(url.host(), Some(Host::Domain(_))) {
        return Err(RemoteError::Refused);
    }
    Ok(url)
}

/// The image behind a checked URL: every hop resolved through the
/// pinned resolver and refused inside the private networks, each
/// redirect checked like the first URL before it is followed, the body
/// read within the bound, the type read from its bytes.
///
/// # Errors
///
/// Returns [`RemoteError::Refused`] for a redirect within the allowed
/// count that points off the rules, [`RemoteError::TooLarge`] past the
/// bound, [`RemoteError::NotAnImage`] for bytes without a raster
/// signature and [`RemoteError::Unreachable`] for a host that answers
/// no image within the redirects allowed, whatever a redirect past them
/// points at.
pub async fn fetch(upstream: &Upstream, mut url: Url) -> Result<Image, RemoteError> {
    for followed in 0..=REMOTE_REDIRECTS {
        match hop(upstream, &url).await? {
            Hop::Image(image) => return Ok(image),
            Hop::Redirect(_) if followed == REMOTE_REDIRECTS => break,
            Hop::Redirect(next) => url = admitted(next)?,
        }
    }
    Err(RemoteError::Unreachable)
}

/// What one request answered: the image or the target of its redirect,
/// joined and unchecked.
enum Hop {
    Image(Image),
    Redirect(Url),
}

async fn hop(upstream: &Upstream, url: &Url) -> Result<Hop, RemoteError> {
    resolved(upstream, url).await?;
    let response = upstream
        .http_no_redirect()
        .get(url.clone())
        .header(USER_AGENT, REMOTE_USER_AGENT)
        .timeout(REMOTE_ATTEMPT_TIMEOUT)
        .send()
        .await
        .map_err(|_| RemoteError::Unreachable)?;
    let status = response.status();
    if status.is_redirection() {
        let location = response
            .headers()
            .get(LOCATION)
            .and_then(|value| value.to_str().ok())
            .ok_or(RemoteError::Unreachable)?;
        let target = url.join(location).map_err(|_| RemoteError::Unreachable)?;
        return Ok(Hop::Redirect(target));
    }
    if status != StatusCode::OK {
        tracing::debug!(
            status = status.as_u16(),
            "the image host answered with an error"
        );
        return Err(RemoteError::Unreachable);
    }
    if response
        .content_length()
        .is_some_and(|declared| declared > REMOTE_IMAGE_LIMIT)
    {
        return Err(RemoteError::TooLarge);
    }
    let limit = usize::try_from(REMOTE_IMAGE_LIMIT).unwrap_or(usize::MAX);
    let bytes = read_bounded(response, limit)
        .await
        .map_err(|error| match error {
            BodyError::TooLarge => RemoteError::TooLarge,
            BodyError::ReadFailed => RemoteError::Unreachable,
        })?;
    let media_type = raster_type(&bytes).ok_or(RemoteError::NotAnImage)?;
    Ok(Hop::Image(Image { media_type, bytes }))
}

/// The host resolved and checked before anything connects; the client
/// resolves once more for the connect and pins what that answers.
async fn resolved(upstream: &Upstream, url: &Url) -> Result<(), RemoteError> {
    let Some(Host::Domain(host)) = url.host() else {
        return Err(RemoteError::Refused);
    };
    let port = url.port_or_known_default().ok_or(RemoteError::Refused)?;
    match upstream.resolve(host, port).await {
        Err(UpstreamError::PrivateNetwork { .. }) => Err(RemoteError::Refused),
        Ok(addresses) if !addresses.is_empty() => Ok(()),
        Ok(_) | Err(_) => Err(RemoteError::Unreachable),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_passes_and_http_is_raised_to_it() {
        assert_eq!(
            checked("https://cdn.example.net/a.png").unwrap().as_str(),
            "https://cdn.example.net/a.png"
        );
        assert_eq!(
            checked("http://cdn.example.net/a.png?w=1")
                .unwrap()
                .as_str(),
            "https://cdn.example.net/a.png?w=1"
        );
        assert_eq!(
            checked("http://cdn.example.net:80/a.png").unwrap().as_str(),
            "https://cdn.example.net/a.png"
        );
        assert_eq!(
            checked("HTTP://CDN.Example.net:8443/a.png")
                .unwrap()
                .as_str(),
            "https://cdn.example.net:8443/a.png"
        );
        assert_eq!(
            checked("https://münchen.example/a.png").unwrap().host_str(),
            Some("xn--mnchen-3ya.example")
        );
    }

    #[test]
    fn a_scheme_off_the_rules_user_information_an_address_or_a_long_url_is_refused() {
        for given in [
            "file:///etc/passwd",
            "ftp://cdn.example.net/a.png",
            "data:image/png;base64,iVBOR",
            "javascript:alert(1)",
            "https://user:secret@cdn.example.net/a.png",
            "https://user@cdn.example.net/a.png",
            "https://127.0.0.1/a.png",
            "https://[::1]/a.png",
            "https://10.0.0.1:8080/a.png",
            "//cdn.example.net/a.png",
            "/a.png",
            "not a url",
            "",
        ] {
            assert_eq!(checked(given), Err(RemoteError::Refused), "{given:?}");
        }
        let long = format!("https://cdn.example.net/{}", "a".repeat(REMOTE_URL_BYTES));
        assert_eq!(checked(&long), Err(RemoteError::Refused));
        let fitting = format!(
            "https://cdn.example.net/{}",
            "a".repeat(REMOTE_URL_BYTES - "https://cdn.example.net/".len())
        );
        assert!(checked(&fitting).is_ok());
    }

    #[test]
    fn the_bounds_are_the_documented_ones() {
        assert_eq!(REMOTE_URL_BYTES, 2048);
        assert_eq!(REMOTE_REDIRECTS, 3);
        assert_eq!(REMOTE_ATTEMPT_TIMEOUT, Duration::from_secs(10));
        assert_eq!(REMOTE_IMAGE_LIMIT, 5 * 1024 * 1024);
        assert_eq!(REMOTE_FETCHES_IN_FLIGHT, 32);
        assert_eq!(REMOTE_IMAGE_RATE.burst, 400);
        assert_eq!(REMOTE_IMAGE_RATE.per_minute, 120);
        assert!(!REMOTE_USER_AGENT.chars().any(|c| c.is_ascii_digit()));
    }
}
