// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Signing a mail account in through Google or Microsoft: the card
//! starts the consent here, the provider sends the window back here and
//! the card polls the outcome here.

use std::sync::{Arc, LazyLock};

use axum::extract::{Path, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Json, Response};
use serde::{Deserialize, Serialize};
use url::{Url, form_urlencoded};

use super::{ApiError, ApiState, Caller, Full, internal};
use crate::accounts::{self, AccountSettings, AuthMethod, Credential, NewAccount, Provider};
use crate::discovery::Address;
use crate::ids::AccountId;
use crate::oauth::{self, Claimant, Claimed, DeniedCause, Grant, NewConsent, Outcome};
use crate::presets;
use crate::probe::Probe;
use crate::providers::{self, OauthClient, OauthProvider};
use crate::scope;
use crate::session;
use crate::store::{StoreError, now_ms};

/// The web app's catalogs, so the one sentence the server renders itself
/// is a message like any other; the card carries the real state.
const CATALOG_EN: &str = include_str!("../../../../packages/i18n/messages/en.json");
const CATALOG_NL: &str = include_str!("../../../../packages/i18n/messages/nl.json");

/// The messages this module reads from a catalog.
#[derive(Deserialize)]
struct Messages {
    consent_close_window: String,
}

struct Catalogs {
    en: Messages,
    nl: Messages,
}

static CATALOGS: LazyLock<Catalogs> = LazyLock::new(|| Catalogs {
    en: messages(CATALOG_EN),
    nl: messages(CATALOG_NL),
});

fn messages(catalog: &str) -> Messages {
    serde_json::from_str(catalog).expect("the catalog is the one the web app builds from")
}

/// What the card sends to start a consent.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct StartRequest {
    provider: Provider,
    address: String,
    /// Set for a reconnect: the account whose tokens the consent
    /// replaces.
    #[serde(default)]
    account_id: Option<AccountId>,
}

/// Where the card sends the window and what it polls.
#[derive(Serialize)]
pub(super) struct StartedView {
    url: Url,
    state: String,
}

pub(super) async fn start_consent(
    State(state): State<ApiState>,
    caller: Caller,
    Json(request): Json<StartRequest>,
) -> Result<Json<StartedView>, ApiError> {
    let provider = presets::for_provider(request.provider)
        .oauth
        .ok_or(ApiError::InvalidRequest)?;
    let address = Address::parse(&request.address).map_err(|_| ApiError::InvalidRequest)?;
    let public_url = state
        .public_url
        .clone()
        .ok_or(ApiError::ProviderNotConfigured)?;
    let user_id = caller.session.user_id.clone();
    let account_provider = request.provider;
    let account_id = request.account_id.clone();
    let store = Arc::clone(&state.store);
    let keys = Arc::clone(&state.keys);
    let client = tokio::task::spawn_blocking(move || -> Result<Option<OauthClient>, ApiError> {
        let scope = scope::resolve(&store, &caller.session.user_id, request.account_id.as_ref())?;
        session::touch(&store, &scope, &caller.session, caller.client.address)?;
        if request.account_id.is_some() {
            let row = accounts::get(&store, &scope)?;
            if row.provider != account_provider || row.auth_method != AuthMethod::Oauth2 {
                return Err(ApiError::InvalidRequest);
            }
        }
        Ok(providers::client(&store, &keys, provider)?)
    })
    .await
    .map_err(internal)??;
    let client = client.ok_or(ApiError::ProviderNotConfigured)?;
    let (started, verifier) = oauth::authorization(&client, &public_url, &address);
    let consent = NewConsent {
        state: started.state.clone(),
        verifier,
        user_id,
        provider,
        account_provider,
        address,
        account_id,
    };
    state.consents.insert(consent, now_ms());
    Ok(Json(StartedView {
        url: started.url,
        state: started.state,
    }))
}

pub(super) async fn pending_consent(
    State(state): State<ApiState>,
    auth: Full,
    Path(consent): Path<String>,
) -> Result<Json<Outcome>, ApiError> {
    state
        .consents
        .outcome(&auth.session.user_id, &consent, now_ms())
        .map(Json)
        .ok_or(ApiError::NotFound)
}

/// The parameters the provider sends back (RFC 6749 sections 4.1.2 and
/// 4.1.2.1); anything else is ignored.
#[derive(Default)]
struct Answer {
    state: Option<String>,
    code: Option<String>,
    error: Option<String>,
}

impl Answer {
    fn parse(query: Option<&str>) -> Self {
        let mut answer = Self::default();
        for (key, value) in form_urlencoded::parse(query.unwrap_or_default().as_bytes()) {
            match &*key {
                "state" => answer.state = Some(value.into_owned()),
                "code" => answer.code = Some(value.into_owned()),
                "error" => answer.error = Some(value.into_owned()),
                _ => {}
            }
        }
        answer
    }
}

/// The browser navigation the provider ends the consent with. Every
/// refusal renders the same page with 404, so a stranger learns nothing.
pub(super) async fn callback(
    State(state): State<ApiState>,
    Path(provider): Path<String>,
    caller: Result<Caller, ApiError>,
    request: Request,
) -> Response {
    let language = Language::preferred(request.headers().get(header::ACCEPT_LANGUAGE));
    let answer = Answer::parse(request.uri().query());
    let (Some(provider), Ok(caller), Some(consent_state)) = (
        OauthProvider::from_word(&provider),
        caller,
        answer.state.clone(),
    ) else {
        return page(StatusCode::NOT_FOUND, language);
    };
    let claimant = Claimant {
        user_id: &caller.session.user_id,
        provider,
    };
    let Some(claimed) = state.consents.claim(claimant, &consent_state, now_ms()) else {
        return page(StatusCode::NOT_FOUND, language);
    };
    let outcome = finish(&state, &caller, claimed, answer).await;
    if let Err(cause) = &outcome {
        tracing::debug!(provider = provider.as_str(), ?cause, "consent denied");
    }
    state.consents.settle(&consent_state, outcome);
    page(StatusCode::OK, language)
}

/// The checked target and the tokens that passed it.
struct Ready {
    target: AccountSettings,
    credential: Credential,
}

/// From the code to the row: exchange, check, store. A refusal anywhere
/// is the cause the card renders.
async fn finish(
    state: &ApiState,
    caller: &Caller,
    mut claimed: Claimed,
    answer: Answer,
) -> Result<AccountId, DeniedCause> {
    if answer.error.is_some() {
        return Err(DeniedCause::AccessDenied);
    }
    let code = answer.code.ok_or(DeniedCause::AccessDenied)?;
    let public_url = state.public_url.clone().ok_or(DeniedCause::Failed)?;
    let client = registered_client(state, claimed.provider).await?;
    let grant = Grant {
        code,
        verifier: std::mem::take(&mut claimed.verifier),
    };
    let tokens = oauth::exchange(state.upstream.http(), &client, &public_url, grant)
        .await
        .map_err(|error| {
            tracing::debug!(%error, "token exchange failed");
            DeniedCause::ExchangeFailed
        })?;
    let refresh_token = tokens.refresh_token.ok_or(DeniedCause::NoRefreshToken)?;
    let credential = Credential::Oauth2 {
        provider: claimed.provider,
        refresh_token,
        access_token: tokens.access_token,
        expires_at: tokens.expires_at,
    };
    let target = target_of(state, caller, &claimed).await?;
    Probe::new(Arc::clone(&state.upstream))
        .check(&claimed.address, &target, &credential)
        .await
        .map_err(DeniedCause::from)?;
    store_tokens(state, caller, claimed, Ready { target, credential }).await
}

async fn registered_client(
    state: &ApiState,
    provider: OauthProvider,
) -> Result<OauthClient, DeniedCause> {
    let store = Arc::clone(&state.store);
    let keys = Arc::clone(&state.keys);
    tokio::task::spawn_blocking(move || providers::client(&store, &keys, provider))
        .await
        .map_err(|error| failed(&error))?
        .map_err(|error| failed(&error))?
        .ok_or(DeniedCause::Failed)
}

/// The target of the check: the stored settings for a reconnect, the
/// preset's fixed servers for a new account; never one the client named.
async fn target_of(
    state: &ApiState,
    caller: &Caller,
    claimed: &Claimed,
) -> Result<AccountSettings, DeniedCause> {
    let Some(account_id) = claimed.account_id.clone() else {
        return presets::fixed_target(claimed.account_provider, &claimed.address)
            .ok_or(DeniedCause::Failed);
    };
    let store = Arc::clone(&state.store);
    let user_id = caller.session.user_id.clone();
    tokio::task::spawn_blocking(move || {
        let scope = scope::resolve(&store, &user_id, Some(&account_id))?;
        accounts::settings(&store, &scope)
    })
    .await
    .map_err(|error| failed(&error))?
    .map_err(|error| failed(&error))
}

/// The row: new for a first consent, the credential replaced for a
/// reconnect.
async fn store_tokens(
    state: &ApiState,
    caller: &Caller,
    claimed: Claimed,
    ready: Ready,
) -> Result<AccountId, DeniedCause> {
    let store = Arc::clone(&state.store);
    let keys = Arc::clone(&state.keys);
    let user_id = caller.session.user_id.clone();
    tokio::task::spawn_blocking(move || -> Result<AccountId, StoreError> {
        let scope = scope::resolve(&store, &user_id, claimed.account_id.as_ref())?;
        if claimed.account_id.is_some() {
            return Ok(accounts::replace_credential(&store, &keys, &scope, &ready.credential)?.id);
        }
        let new = NewAccount {
            address: claimed.address.to_string(),
            name: presets::default_name(claimed.account_provider, &claimed.address),
            provider: claimed.account_provider,
            settings: ready.target,
            credential: ready.credential,
        };
        Ok(accounts::add(&store, &keys, &scope, &new)?.id)
    })
    .await
    .map_err(|error| failed(&error))?
    .map_err(|error| failed(&error))
}

/// Logs an instance failure and hands the card the one word for it.
fn failed(error: &impl std::fmt::Display) -> DeniedCause {
    tracing::error!(%error, "consent could not be completed");
    DeniedCause::Failed
}

/// The instance's locales; the page follows the browser's preference,
/// which the card's default follows too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Language {
    En,
    Nl,
}

impl Language {
    /// The first listed tag among the known ones wins; browsers list in
    /// order of preference.
    fn preferred(header: Option<&HeaderValue>) -> Self {
        header
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .split(',')
            .map(|entry| entry.split(';').next().unwrap_or_default().trim())
            .find_map(|tag| {
                match tag
                    .split('-')
                    .next()
                    .unwrap_or_default()
                    .to_ascii_lowercase()
                    .as_str()
                {
                    "nl" => Some(Self::Nl),
                    "en" => Some(Self::En),
                    _ => None,
                }
            })
            .unwrap_or(Self::En)
    }

    fn tag(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Nl => "nl",
        }
    }

    fn close_window(self) -> &'static str {
        match self {
            Self::En => &CATALOGS.en.consent_close_window,
            Self::Nl => &CATALOGS.nl.consent_close_window,
        }
    }
}

/// The page every callback ends on: no script, no style, no product
/// name, so the instance's policy holds as it is.
fn page(status: StatusCode, language: Language) -> Response {
    let sentence = escaped(language.close_window());
    let tag = language.tag();
    let html = format!(
        "<!doctype html><html lang=\"{tag}\"><head><meta charset=\"utf-8\"><title>{sentence}</title></head><body><p>{sentence}</p></body></html>"
    );
    (status, Html(html)).into_response()
}

/// The sentence is text content, so three characters need escaping.
fn escaped(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(text: &str) -> HeaderValue {
        HeaderValue::from_str(text).unwrap()
    }

    #[test]
    fn the_first_known_language_wins_and_english_is_the_fallback() {
        for (text, language) in [
            ("nl-NL,nl;q=0.9,en;q=0.8", Language::Nl),
            ("NL", Language::Nl),
            ("de-DE,en-US;q=0.5", Language::En),
            ("fr", Language::En),
            ("", Language::En),
        ] {
            assert_eq!(Language::preferred(Some(&header(text))), language, "{text}");
        }
        assert_eq!(Language::preferred(None), Language::En);
    }

    #[test]
    fn the_answer_reads_the_three_parameters_and_ignores_the_rest() {
        let answer = Answer::parse(Some("code=4%2Fabc&state=s1&scope=x&authuser=0"));
        assert_eq!(answer.code.as_deref(), Some("4/abc"));
        assert_eq!(answer.state.as_deref(), Some("s1"));
        assert_eq!(answer.error, None);
        let denied = Answer::parse(Some("error=access_denied&state=s1"));
        assert_eq!(denied.error.as_deref(), Some("access_denied"));
        assert_eq!(denied.code, None);
        let empty = Answer::parse(None);
        assert_eq!(empty.state, None);
    }

    #[test]
    fn both_catalogs_carry_the_sentence_in_their_own_words() {
        assert!(!Language::En.close_window().is_empty());
        assert!(!Language::Nl.close_window().is_empty());
        assert_ne!(Language::En.close_window(), Language::Nl.close_window());
    }

    #[test]
    fn the_page_carries_the_sentence_and_nothing_else() {
        let response = page(StatusCode::OK, Language::Nl);
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("text/html")
        );
    }
}
