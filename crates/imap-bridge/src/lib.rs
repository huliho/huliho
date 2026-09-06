// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Translates JMAP Mail semantics (RFC 8620, RFC 8621) to `IMAP4rev2`
//! (RFC 9051) and SMTP submission (RFC 6409).

pub mod session;
pub mod verify;
