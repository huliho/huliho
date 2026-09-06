// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Serves the built web app and the Huliho API from one process.

pub mod accounts;
pub mod api;
pub mod app;
pub mod auth;
pub mod config;
pub mod discovery;
pub mod events;
pub mod identity;
pub mod ids;
pub mod prefs;
pub mod presets;
pub mod probe;
pub mod providers;
pub mod rate;
pub mod scope;
mod sealed;
pub mod secrets;
pub mod session;
pub mod store;
pub mod upstream;
