// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The client record a session keeps: browser and OS family, phone and
//! installed-app flags.

use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ValueRef};
use serde::{Deserialize, Serialize};

/// Longest User-Agent prefix inspected; the rest says nothing new.
const MAX_USER_AGENT_CHARS: usize = 512;

/// Browser markers, most specific first: Edge carries Chrome's marker
/// and both carry Safari's; the iOS builds carry their own.
const BROWSERS: [(&str, &str); 8] = [
    ("Edg/", "Edge"),
    ("EdgA/", "Edge"),
    ("EdgiOS/", "Edge"),
    ("Firefox/", "Firefox"),
    ("FxiOS/", "Firefox"),
    ("Chrome/", "Chrome"),
    ("CriOS/", "Chrome"),
    ("Safari/", "Safari"),
];

/// OS markers, most specific first: Android carries the Linux marker
/// and an iPhone says "like Mac OS X".
const SYSTEMS: [(&str, &str); 6] = [
    ("Android", "Android"),
    ("iPhone", "iOS"),
    ("iPad", "iOS"),
    ("Windows", "Windows"),
    ("Mac OS X", "macOS"),
    ("Linux", "Linux"),
];

/// Unknown families stay `None`; an empty JSON object reads as unknown.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Device {
    pub browser: Option<String>,
    pub os: Option<String>,
    pub phone: bool,
    pub installed: bool,
}

impl FromSql for Device {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        serde_json::from_str(value.as_str()?).map_err(|error| FromSqlError::Other(error.into()))
    }
}

/// Reads the browser and OS families from a User-Agent header. The
/// installed-app flag comes from the client, since the header does not
/// carry it.
#[must_use]
pub fn from_user_agent(user_agent: &str, installed: bool) -> Device {
    let inspected: String = user_agent.chars().take(MAX_USER_AGENT_CHARS).collect();
    Device {
        browser: family(&inspected, &BROWSERS),
        os: family(&inspected, &SYSTEMS),
        phone: inspected.contains("Mobile") || inspected.contains("iPhone"),
        installed,
    }
}

fn family(user_agent: &str, table: &[(&str, &str)]) -> Option<String> {
    table
        .iter()
        .find(|(marker, _)| user_agent.contains(marker))
        .map(|(_, name)| (*name).to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIREFOX_LINUX: &str =
        "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0";
    const CHROME_WINDOWS: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                                  (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";
    const SAFARI_MAC: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 \
                              (KHTML, like Gecko) Version/17.5 Safari/605.1.15";
    const EDGE_WINDOWS: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                                (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36 Edg/126.0.0.0";
    const CHROME_ANDROID: &str = "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 \
                                  (KHTML, like Gecko) Chrome/126.0.0.0 Mobile Safari/537.36";
    const SAFARI_IPHONE: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) \
                                 AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 \
                                 Mobile/15E148 Safari/604.1";

    fn families(user_agent: &str) -> (Option<String>, Option<String>, bool) {
        let device = from_user_agent(user_agent, false);
        (device.browser, device.os, device.phone)
    }

    fn known(browser: &str, os: &str, phone: bool) -> (Option<String>, Option<String>, bool) {
        (Some(browser.to_owned()), Some(os.to_owned()), phone)
    }

    #[test]
    fn desktop_browsers_resolve_to_their_families() {
        assert_eq!(families(FIREFOX_LINUX), known("Firefox", "Linux", false));
        assert_eq!(families(CHROME_WINDOWS), known("Chrome", "Windows", false));
        assert_eq!(families(SAFARI_MAC), known("Safari", "macOS", false));
        assert_eq!(families(EDGE_WINDOWS), known("Edge", "Windows", false));
    }

    #[test]
    fn phones_resolve_with_the_phone_flag() {
        assert_eq!(families(CHROME_ANDROID), known("Chrome", "Android", true));
        assert_eq!(families(SAFARI_IPHONE), known("Safari", "iOS", true));
    }

    #[test]
    fn an_unknown_agent_stays_unknown() {
        assert_eq!(families(""), (None, None, false));
        assert_eq!(families("curl/8.6.0"), (None, None, false));
    }

    #[test]
    fn the_installed_flag_comes_from_the_caller() {
        assert!(from_user_agent(FIREFOX_LINUX, true).installed);
        assert!(!from_user_agent(FIREFOX_LINUX, false).installed);
    }

    #[test]
    fn a_device_round_trips_through_json_and_an_empty_object_is_unknown() {
        let device = from_user_agent(CHROME_ANDROID, true);
        let json = serde_json::to_string(&device).unwrap();
        assert_eq!(serde_json::from_str::<Device>(&json).unwrap(), device);
        assert_eq!(
            serde_json::from_str::<Device>("{}").unwrap(),
            Device::default()
        );
    }
}
