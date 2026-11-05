// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A draft as the create object of `Email/set` gives it (RFC 8621 section
//! 4.6), the message built from it and the two copies a send writes of it.

mod address;
mod build;
mod draft;
mod envelope;
mod header;

pub use address::fits_address;
pub use build::{Blobs, MAX_MESSAGE_BYTES, build, weight};
pub use draft::{Content, Draft, Part, Stamp};
pub use envelope::{Unsendable, recipients};
#[cfg(feature = "test-support")]
pub(crate) use header::names;
pub use header::{Forms, sendable};
