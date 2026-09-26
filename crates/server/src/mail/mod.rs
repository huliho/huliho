// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What the proxy does with mail on its way to the browser: the
//! sanitizer and the pass that runs it over a body answer.

pub(crate) mod bodies;
pub mod sanitize;
