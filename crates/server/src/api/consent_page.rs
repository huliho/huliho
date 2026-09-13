// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The one page the server renders itself: the sentence the consent
//! window ends on, in the browser's language.

use std::sync::LazyLock;

use axum::http::{HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use serde::Deserialize;

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

/// The instance's locales; the page follows the browser's preference,
/// which the card's default follows too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Language {
    En,
    Nl,
}

impl Language {
    /// The first listed tag among the known ones wins; browsers list in
    /// order of preference.
    pub(super) fn preferred(header: Option<&HeaderValue>) -> Self {
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
pub(super) fn page(status: StatusCode, language: Language) -> Response {
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
    use axum::http::header;

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
