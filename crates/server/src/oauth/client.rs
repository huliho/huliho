// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The provider's endpoints through the oauth2 crate on the pinned HTTP
//! client: the consent URL, the code exchange and the refresh.

use std::fmt;
use std::future::Future;
use std::pin::Pin;

use oauth2::basic::{
    BasicClient, BasicErrorResponse, BasicErrorResponseType, BasicRequestTokenError,
    BasicTokenResponse,
};
use oauth2::{
    AsyncHttpClient, AuthType, AuthUrl, AuthorizationCode, ClientId, ClientSecret,
    CodeTokenRequest, CsrfToken, EndpointNotSet, EndpointSet, HttpRequest, HttpResponse,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, RefreshToken, RefreshTokenRequest,
    RequestTokenError, Scope as OauthScope, TokenResponse as _, TokenUrl,
};
use thiserror::Error;
use url::Url;

use crate::discovery::Address;
use crate::providers::{OauthClient, OauthProvider};
use crate::store::now_ms;
use crate::upstream::read_bounded;

/// A token response runs to a few hundred bytes; more is not one.
const MAX_TOKEN_RESPONSE_BYTES: usize = 16 * 1024;

/// RFC 7636 section 4.1: a verifier is 43 to 128 characters long; the
/// S256 challenge is 43.
pub const PKCE_MIN_LENGTH: usize = 43;

/// The client with the two endpoints a code flow needs.
type Client = BasicClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointSet>;

/// What the card gets after a start: where to send the window and the
/// state to poll.
#[derive(Debug)]
pub struct Started {
    pub url: Url,
    pub state: String,
}

/// The code the provider sent back and the verifier the start kept.
pub struct Grant {
    pub code: String,
    pub verifier: String,
}

/// What a consent or a refresh yields; the refresh token is absent when
/// the provider kept the old one.
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: i64,
}

/// Prints the expiry only, so tokens never reach a log line this way.
impl fmt::Debug for Tokens {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Tokens(expires_at={})", self.expires_at)
    }
}

/// Why a token request failed, in words that carry nothing from the
/// provider.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum TokenError {
    /// The grant is gone: consent withdrawn, refresh token expired or
    /// the code used twice.
    #[error("the provider refuses the grant")]
    Revoked,
    /// The provider refused for a reason of the client's making; the
    /// word is one of the closed set of RFC 6749 section 5.2.
    #[error("the provider refused the request as {0}")]
    Refused(&'static str),
    #[error("the token endpoint cannot be reached")]
    Unreachable,
    #[error("the token endpoint answered something else")]
    Unexpected,
}

/// Why the pinned client could not answer.
#[derive(Debug, Error)]
enum HttpError {
    #[error(transparent)]
    Request(#[from] reqwest::Error),
    #[error("the answer is not a token response")]
    Body,
}

type Sending<'a> = Pin<Box<dyn Future<Output = Result<HttpResponse, HttpError>> + Send + 'a>>;

/// A token request as the client library answers it.
type TokenOutcome = Result<BasicTokenResponse, BasicRequestTokenError<HttpError>>;

/// `{public_url}/auth/{provider}/callback`, under whatever path the
/// instance lives at.
#[must_use]
fn redirect_url(public_url: &Url, provider: OauthProvider) -> Url {
    let mut url = public_url.clone();
    let base = url.path().trim_end_matches('/').to_owned();
    url.set_path(&format!("{base}/auth/{}/callback", provider.as_str()));
    url.set_query(None);
    url.set_fragment(None);
    url
}

/// The consent URL with PKCE, the scopes and the address as the hint;
/// the verifier stays with the pending consent for the exchange.
#[must_use]
pub fn authorization(
    client: &OauthClient,
    public_url: &Url,
    address: &Address,
) -> (Started, String) {
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let oauth = oauth_client(client, Some(redirect_url(public_url, client.provider)));
    let mut request = oauth
        .authorize_url(CsrfToken::new_random)
        .set_pkce_challenge(challenge)
        .add_extra_param("login_hint", address.to_string());
    for scope in client.provider.scopes() {
        request = request.add_scope(OauthScope::new((*scope).to_owned()));
    }
    for (name, value) in client.provider.extra_params() {
        request = request.add_extra_param(*name, *value);
    }
    let (url, state) = request.url();
    (
        Started {
            url,
            state: state.into_secret(),
        },
        verifier.into_secret(),
    )
}

/// Turns the code into tokens; the redirect URI rides along as RFC 6749
/// section 4.1.3 requires.
///
/// # Errors
///
/// Returns the cause in fixed words; the provider's own text stays out.
pub async fn exchange(
    http: &reqwest::Client,
    client: &OauthClient,
    public_url: &Url,
    grant: Grant,
) -> Result<Tokens, TokenError> {
    let oauth = oauth_client(client, Some(redirect_url(public_url, client.provider)));
    let send = Pinned(http);
    let request = oauth
        .exchange_code(AuthorizationCode::new(grant.code))
        .set_pkce_verifier(PkceCodeVerifier::new(grant.verifier));
    let response = code_request(request, &send).await.map_err(token_error)?;
    Ok(tokens(&response))
}

/// A fresh access token; the provider may rotate the refresh token too.
///
/// # Errors
///
/// Returns [`TokenError::Revoked`] when the provider refuses the grant,
/// the other variants when the request itself failed.
pub async fn refresh(
    http: &reqwest::Client,
    client: &OauthClient,
    refresh_token: &str,
) -> Result<Tokens, TokenError> {
    let oauth = oauth_client(client, None);
    let send = Pinned(http);
    let token = RefreshToken::new(refresh_token.to_owned());
    let response = refresh_request(oauth.exchange_refresh_token(&token), &send)
        .await
        .map_err(token_error)?;
    Ok(tokens(&response))
}

fn oauth_client(client: &OauthClient, redirect: Option<Url>) -> Client {
    let built = BasicClient::new(ClientId::new(client.id.clone()))
        .set_client_secret(ClientSecret::new(client.secret.clone()))
        // Both providers document the client credentials in the body.
        .set_auth_type(AuthType::RequestBody)
        .set_auth_uri(AuthUrl::from_url(endpoint(
            client.provider.authorization_url(),
        )))
        .set_token_uri(TokenUrl::from_url(endpoint(client.provider.token_url())));
    match redirect {
        Some(url) => built.set_redirect_uri(RedirectUrl::from_url(url)),
        None => built,
    }
}

/// A fixed endpoint as a URL; a test parses every one.
fn endpoint(text: &str) -> Url {
    Url::parse(text).expect("a fixed endpoint URL parses")
}

fn tokens(response: &BasicTokenResponse) -> Tokens {
    let lifetime_ms = response
        .expires_in()
        .and_then(|lifetime| i64::try_from(lifetime.as_millis()).ok())
        .unwrap_or_default();
    Tokens {
        access_token: response.access_token().secret().clone(),
        refresh_token: response.refresh_token().map(|token| token.secret().clone()),
        expires_at: now_ms().saturating_add(lifetime_ms),
    }
}

/// The error in fixed words. The response type is a closed vocabulary,
/// so its word may travel; descriptions never do.
fn token_error(error: BasicRequestTokenError<HttpError>) -> TokenError {
    match error {
        RequestTokenError::ServerResponse(answer) => match answer.error() {
            BasicErrorResponseType::InvalidGrant => TokenError::Revoked,
            BasicErrorResponseType::InvalidClient => TokenError::Refused("invalid_client"),
            BasicErrorResponseType::InvalidRequest => TokenError::Refused("invalid_request"),
            BasicErrorResponseType::InvalidScope => TokenError::Refused("invalid_scope"),
            BasicErrorResponseType::UnauthorizedClient => {
                TokenError::Refused("unauthorized_client")
            }
            BasicErrorResponseType::UnsupportedGrantType => {
                TokenError::Refused("unsupported_grant_type")
            }
            BasicErrorResponseType::Extension(_) => {
                TokenError::Refused("an error outside the standard")
            }
        },
        RequestTokenError::Request(_) => TokenError::Unreachable,
        RequestTokenError::Parse(..) | RequestTokenError::Other(_) => TokenError::Unexpected,
    }
}

/// The pinned client as the oauth2 crate calls it: the same resolver,
/// network rule, trust and time limit as every other outbound request.
struct Pinned<'a>(&'a reqwest::Client);

impl<'c> AsyncHttpClient<'c> for Pinned<'_> {
    type Error = HttpError;
    type Future = Sending<'c>;

    fn call(&'c self, request: HttpRequest) -> Sending<'c> {
        Box::pin(send(self.0, request))
    }
}

/// The exchange as a future the runtime may move between threads; the
/// bound is spelled out here because an async block cannot prove it for
/// the client library's opaque future.
fn code_request<'a>(
    request: CodeTokenRequest<'a, BasicErrorResponse, BasicTokenResponse>,
    send: &'a Pinned<'a>,
) -> impl Future<Output = TokenOutcome> + Send + 'a {
    request.request_async(send)
}

/// The refresh the same way.
fn refresh_request<'a>(
    request: RefreshTokenRequest<'a, BasicErrorResponse, BasicTokenResponse>,
    send: &'a Pinned<'a>,
) -> impl Future<Output = TokenOutcome> + Send + 'a {
    request.request_async(send)
}

async fn send(http: &reqwest::Client, request: HttpRequest) -> Result<HttpResponse, HttpError> {
    let response = http.execute(reqwest::Request::try_from(request)?).await?;
    let mut answer = oauth2::http::Response::builder().status(response.status());
    for (name, value) in response.headers() {
        answer = answer.header(name, value);
    }
    let body = read_bounded(response, MAX_TOKEN_RESPONSE_BYTES)
        .await
        .ok_or(HttpError::Body)?;
    answer.body(body).map_err(|_| HttpError::Body)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use oauth2::StandardErrorResponse;

    use super::*;

    fn client(provider: OauthProvider) -> OauthClient {
        OauthClient {
            provider,
            id: "client-id".to_owned(),
            secret: "client-secret".to_owned(),
        }
    }

    fn public_url() -> Url {
        Url::parse("https://mail.example.test").unwrap()
    }

    fn pairs(url: &Url) -> HashMap<String, String> {
        url.query_pairs()
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect()
    }

    #[test]
    fn the_redirect_url_lives_under_the_public_url_whatever_its_path() {
        assert_eq!(
            redirect_url(&public_url(), OauthProvider::Google).as_str(),
            "https://mail.example.test/auth/google/callback"
        );
        let prefixed = Url::parse("https://example.test/mail/?x=1#top").unwrap();
        assert_eq!(
            redirect_url(&prefixed, OauthProvider::Microsoft).as_str(),
            "https://example.test/mail/auth/microsoft/callback"
        );
    }

    #[test]
    fn a_google_consent_url_carries_pkce_the_scope_the_hint_and_the_offline_parameters() {
        let address = Address::parse("sanne@gmail.com").unwrap();
        let (started, verifier) =
            authorization(&client(OauthProvider::Google), &public_url(), &address);
        assert_eq!(
            started.url.origin().ascii_serialization(),
            "https://accounts.google.com"
        );
        assert_eq!(started.url.path(), "/o/oauth2/v2/auth");
        let pairs = pairs(&started.url);
        assert_eq!(pairs["response_type"], "code");
        assert_eq!(pairs["client_id"], "client-id");
        assert_eq!(pairs["state"], started.state);
        assert_eq!(pairs["code_challenge_method"], "S256");
        let challenge =
            PkceCodeChallenge::from_code_verifier_sha256(&PkceCodeVerifier::new(verifier));
        assert_eq!(pairs["code_challenge"], challenge.as_str());
        assert_eq!(
            pairs["redirect_uri"],
            "https://mail.example.test/auth/google/callback"
        );
        assert_eq!(pairs["scope"], "https://mail.google.com/");
        assert_eq!(pairs["access_type"], "offline");
        assert_eq!(pairs["prompt"], "consent");
        assert_eq!(pairs["login_hint"], "sanne@gmail.com");
        assert!(!pairs.contains_key("client_secret"));
    }

    #[test]
    fn a_microsoft_consent_url_asks_for_offline_access_as_a_scope() {
        let address = Address::parse("noor@outlook.com").unwrap();
        let (started, _) =
            authorization(&client(OauthProvider::Microsoft), &public_url(), &address);
        assert_eq!(started.url.path(), "/common/oauth2/v2.0/authorize");
        let pairs = pairs(&started.url);
        assert!(
            pairs["scope"]
                .split(' ')
                .any(|scope| scope == "offline_access")
        );
        assert!(!pairs.contains_key("access_type"));
        assert!(!pairs.contains_key("prompt"));
    }

    #[test]
    fn every_start_draws_a_fresh_state_and_verifier() {
        let address = Address::parse("sanne@gmail.com").unwrap();
        let (first, first_verifier) =
            authorization(&client(OauthProvider::Google), &public_url(), &address);
        let (second, second_verifier) =
            authorization(&client(OauthProvider::Google), &public_url(), &address);
        assert_ne!(first.state, second.state);
        assert_ne!(first_verifier, second_verifier);
        assert!(first_verifier.len() >= PKCE_MIN_LENGTH);
    }

    #[test]
    fn every_token_error_has_fixed_words() {
        let server = |kind| {
            RequestTokenError::ServerResponse(StandardErrorResponse::new(
                kind,
                Some("the provider's own words".to_owned()),
                None,
            ))
        };
        assert_eq!(
            token_error(server(BasicErrorResponseType::InvalidGrant)),
            TokenError::Revoked
        );
        assert_eq!(
            token_error(server(BasicErrorResponseType::InvalidClient)),
            TokenError::Refused("invalid_client")
        );
        assert_eq!(
            token_error(server(BasicErrorResponseType::Extension("odd".to_owned()))),
            TokenError::Refused("an error outside the standard")
        );
        assert_eq!(
            token_error(RequestTokenError::Request(HttpError::Body)),
            TokenError::Unreachable
        );
        assert_eq!(
            token_error(RequestTokenError::Other("x".to_owned())),
            TokenError::Unexpected
        );
        for error in [
            TokenError::Revoked,
            TokenError::Refused("invalid_client"),
            TokenError::Unreachable,
            TokenError::Unexpected,
        ] {
            assert!(!error.to_string().contains("own words"));
        }
    }

    #[test]
    fn the_fixed_endpoints_parse_and_the_tokens_print_no_secret() {
        for provider in [OauthProvider::Google, OauthProvider::Microsoft] {
            let built = oauth_client(&client(provider), None);
            assert_eq!(built.token_uri().url().as_str(), provider.token_url());
        }
        let tokens = Tokens {
            access_token: "ya29.secret".to_owned(),
            refresh_token: Some("1//secret".to_owned()),
            expires_at: 7,
        };
        assert_eq!(format!("{tokens:?}"), "Tokens(expires_at=7)");
    }
}
