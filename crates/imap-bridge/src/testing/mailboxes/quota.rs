// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! GETQUOTAROOT of the scripted server behind QUOTA (RFC 9208 section
//! 4.1.2): the lines a test wrote, or a QUOTAROOT that names no root.

use super::{Extension, Mailboxes, quote};

impl Mailboxes {
    /// The answer to GETQUOTAROOT with its tagged line; BAD while the
    /// extension is off.
    pub(in crate::testing) fn quota_root(&self, tag: &str) -> String {
        if !self.has(Extension::Quota) {
            return format!("{tag} BAD not offered\r\n");
        }
        let lines = self.quota.as_deref().unwrap_or("* QUOTAROOT INBOX\r\n");
        format!("{lines}{tag} OK done\r\n")
    }
}

/// The two lines of one quota root over the inbox: its storage in units
/// of 1024 octets and its message count, each as used and limit.
#[must_use]
pub fn quota_lines(root: &str, storage: (u64, u64), messages: (u64, u64)) -> String {
    let root = quote(root);
    format!(
        "* QUOTAROOT INBOX {root}\r\n* QUOTA {root} (STORAGE {} {} MESSAGE {} {})\r\n",
        storage.0, storage.1, messages.0, messages.1
    )
}
