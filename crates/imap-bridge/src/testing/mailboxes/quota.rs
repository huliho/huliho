// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! GETQUOTAROOT of the scripted server behind QUOTA (RFC 9208 section
//! 4.1.2): the lines a test wrote, a QUOTAROOT that names no root or a
//! refusal.

use std::sync::PoisonError;

use super::{Extension, Mailboxes, quote};

/// What a GETQUOTAROOT answers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum QuotaAnswer {
    /// A QUOTAROOT that names no root.
    #[default]
    Unrooted,
    /// These untagged lines, as the wire carries them.
    Lines(String),
    /// A tagged NO.
    Refused,
}

impl Mailboxes {
    /// What the next GETQUOTAROOT answers.
    pub fn set_quota(&self, answer: QuotaAnswer) {
        *self.quota.lock().unwrap_or_else(PoisonError::into_inner) = answer;
    }

    /// The answer to GETQUOTAROOT with its tagged line; BAD while the
    /// extension is off.
    pub(in crate::testing) fn quota_root(&self, tag: &str) -> String {
        if !self.has(Extension::Quota) {
            return format!("{tag} BAD not offered\r\n");
        }
        match &*self.quota.lock().unwrap_or_else(PoisonError::into_inner) {
            QuotaAnswer::Unrooted => format!("* QUOTAROOT INBOX\r\n{tag} OK done\r\n"),
            QuotaAnswer::Lines(lines) => format!("{lines}{tag} OK done\r\n"),
            QuotaAnswer::Refused => format!("{tag} NO refused\r\n"),
        }
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
