// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Fixtures for the download route of a native account: the instance
//! over the fixture upstream, the path the session object's template
//! expands to and the blob requests that upstream saw.

use huliho_imap_bridge::testing::TOKEN;
use huliho_server::accounts::Credential;
use huliho_server::gate::RUN_WINDOW;

use crate::jmap_upstream::UPSTREAM_ACCOUNT;
use crate::proxy_rig::{Instance, Setup};

/// The blob id and the name the tests ask for.
pub const BLOB: &str = "b1";
pub const NAME: &str = "photo.png";

pub fn bearer() -> Credential {
    Credential::Bearer {
        token: TOKEN.to_owned(),
    }
}

pub async fn instance() -> Instance {
    Instance::start(Setup {
        window: RUN_WINDOW,
        servers: &[],
    })
    .await
}

/// The path of the route as the session object's template expands it,
/// the name already encoded as a browser sends it.
pub fn path(id: &str, name: &str, media_type: Option<&str>) -> String {
    let query = media_type.map_or(String::new(), |media_type| format!("?type={media_type}"));
    format!("/api/jmap/{id}/download/{UPSTREAM_ACCOUNT}/{BLOB}/{name}{query}")
}

/// The blob requests the fixture upstream saw.
pub fn downloads(instance: &Instance) -> Vec<String> {
    instance
        .upstream
        .lines()
        .into_iter()
        .filter(|line| line.contains("/jmap/download/"))
        .collect()
}
