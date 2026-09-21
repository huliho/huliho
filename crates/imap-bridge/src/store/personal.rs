// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The personal fields of an email: the JSON the host seals into the
//! blob of a row.

use serde::{Deserialize, Serialize};

/// One address as `Email/get` renders it (RFC 8621 section 4.1.2.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Address {
    pub name: Option<String>,
    pub email: String,
}

/// The body part a preview is read from, chosen when the structure of
/// the message was read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewPart {
    /// The part number as a FETCH section names it; empty for a message
    /// that is one part.
    pub path: String,
    /// Whether the part is HTML, plain text otherwise.
    pub html: bool,
    /// The size of the part as the server gave it.
    pub bytes: u32,
}

/// The personal fields of an email, the JSON inside the sealed blob.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Personal {
    pub from: Option<Vec<Address>>,
    pub to: Option<Vec<Address>>,
    pub cc: Option<Vec<Address>>,
    pub bcc: Option<Vec<Address>>,
    pub reply_to: Option<Vec<Address>>,
    pub sender: Option<Vec<Address>>,
    pub subject: Option<String>,
    pub message_id: Option<Vec<String>>,
    pub in_reply_to: Option<Vec<String>>,
    pub references: Option<Vec<String>>,
    /// The preview once it was fetched; empty for a body without text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_part: Option<PreviewPart>,
}

impl Personal {
    /// Every message id the header names: its own, then the ones it
    /// answers and refers to. Threads are built over them.
    pub fn named_ids(&self) -> impl Iterator<Item = &str> {
        [&self.message_id, &self.in_reply_to, &self.references]
            .into_iter()
            .flatten()
            .flatten()
            .map(String::as_str)
    }
}
