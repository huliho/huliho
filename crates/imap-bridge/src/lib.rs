// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Translates JMAP Mail semantics (RFC 8620, RFC 8621) to `IMAP4rev1`
//! (RFC 3501) with its extensions and SMTP submission (RFC 6409).

mod dates;
pub mod jmap;
pub mod mailboxes;
pub mod runtime;
pub mod seal;
pub mod session;
pub mod smtp;
pub mod store;
pub mod sync;
#[cfg(feature = "test-support")]
pub mod testing;
pub mod utf7;
pub mod verify;
