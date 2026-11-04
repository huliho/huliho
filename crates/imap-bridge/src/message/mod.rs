// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A draft as the create object of `Email/set` gives it (RFC 8621 section
//! 4.6) and the message built from it.

mod address;
mod build;
mod draft;

pub use address::fits_address;
pub use build::{Blobs, MAX_MESSAGE_BYTES, build, weight};
pub use draft::{Content, Draft, Part, Stamp};
