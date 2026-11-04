// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A draft after its create object was read: the fields a header carries,
//! the tree of its body and what the row of its message holds.

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use uuid::Uuid;

use crate::store::Personal;
use crate::sync::{mapping, preview};

/// The domain of a Message-ID where a draft names no sender yet (RFC 2606
/// section 2).
const NO_DOMAIN: &str = "draft.invalid";

/// What the first bytes of a text part say of its type, for its preview.
const PLAIN_HEADER: &[u8] = b"Content-Type: text/plain; charset=utf-8";
const HTML_HEADER: &[u8] = b"Content-Type: text/html; charset=utf-8";

const PLAIN: &str = "text/plain";
const HTML: &str = "text/html";

/// A draft as its create object gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    /// The one mailbox: an id, or a creation id after `#`.
    pub mailbox: String,
    /// The keywords it is stored under, each a storable system flag's.
    pub keywords: Vec<String>,
    /// The fields a header carries; the Message-ID is the bridge's.
    pub personal: Personal,
    pub body: Part,
}

/// One part of a draft's tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    /// In lower case, `text/plain` for one.
    pub media_type: String,
    /// The Content-Type parameters the object declares.
    pub parameters: Vec<(String, String)>,
    pub name: Option<String>,
    /// `attachment` or `inline`.
    pub disposition: Option<String>,
    /// Without its angle brackets.
    pub cid: Option<String>,
    pub content: Content,
}

/// What a part holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Content {
    /// Text from `bodyValues`.
    Value(String),
    /// The bytes behind a blob id.
    Blob(String),
    Parts(Vec<Part>),
}

/// What the bridge stamps on one message: the Message-ID without its
/// angle brackets and the moment it was built, in seconds since the epoch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stamp {
    pub message_id: String,
    pub date: i64,
}

impl Stamp {
    /// A fresh id at the domain of the sender and the time as it stands
    /// (RFC 5322 sections 3.6.1 and 3.6.4).
    #[must_use]
    pub fn now(draft: &Draft) -> Self {
        let senders = draft.personal.from.as_deref().unwrap_or_default();
        let domain = senders
            .first()
            .and_then(|sender| sender.email.rsplit_once('@'))
            .map_or(NO_DOMAIN, |(_, domain)| domain);
        let date = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|elapsed| i64::try_from(elapsed.as_secs()).ok())
            .unwrap_or_default();
        Self {
            message_id: format!("{}@{domain}", Uuid::new_v4()),
            date,
        }
    }
}

impl Draft {
    /// Every blob id the tree names, each once, in the order of the tree.
    #[must_use]
    pub fn blob_ids(&self) -> Vec<&str> {
        let mut found = Vec::new();
        self.body.blob_ids(&mut found);
        found
    }

    /// The flags of an APPEND.
    #[must_use]
    pub fn flags(&self) -> Vec<String> {
        self.keywords
            .iter()
            .map(|keyword| mapping::flag(keyword))
            .collect()
    }

    /// The keywords as a row holds them.
    #[must_use]
    pub fn keyword_map(&self) -> BTreeMap<String, bool> {
        self.keywords
            .iter()
            .map(|keyword| (keyword.clone(), true))
            .collect()
    }

    /// Whether the message counts as holding an attachment (RFC 8621
    /// section 4.1.4), as the sync reads a structure.
    #[must_use]
    pub fn has_attachment(&self) -> bool {
        self.body.attaches(false)
    }

    /// The personal fields of the row: the header's and the preview.
    #[must_use]
    pub fn stamped(&self, stamp: &Stamp) -> Personal {
        Personal {
            message_id: Some(vec![stamp.message_id.clone()]),
            preview: Some(self.preview()),
            ..self.personal.clone()
        }
    }

    /// The preview as the sync reads one: the first plain text, else the
    /// first HTML, a part sent as an attachment never.
    fn preview(&self) -> String {
        let text = |wanted: &str| self.body.first_text(wanted);
        let found = text(PLAIN)
            .map(|value| (PLAIN_HEADER, value))
            .or_else(|| text(HTML).map(|value| (HTML_HEADER, value)));
        let Some((header, value)) = found else {
            return String::new();
        };
        let cap = usize::try_from(preview::PREVIEW_HTML_PART_BYTES).unwrap_or(usize::MAX);
        let start = value.floor_char_boundary(cap.min(value.len()));
        preview::text(header, &value.as_bytes()[..start])
    }
}

impl Part {
    fn blob_ids<'a>(&'a self, found: &mut Vec<&'a str>) {
        match &self.content {
            Content::Blob(id) if !found.contains(&id.as_str()) => found.push(id),
            Content::Parts(parts) => parts.iter().for_each(|part| part.blob_ids(found)),
            Content::Value(_) | Content::Blob(_) => {}
        }
    }

    /// Every leaf that is not text and every part sent as an attachment
    /// count, outside the inline images of a multipart/related body.
    fn attaches(&self, related: bool) -> bool {
        if let Content::Parts(parts) = &self.content {
            let inner = self.media_type == "multipart/related";
            return parts.iter().any(|part| part.attaches(inner));
        }
        let text = self.media_type.starts_with("text/");
        let inline_image = related && self.media_type.starts_with("image/");
        self.disposition.as_deref() == Some("attachment") || !(text || inline_image)
    }

    fn first_text(&self, wanted: &str) -> Option<&str> {
        match &self.content {
            Content::Value(value)
                if self.media_type == wanted
                    && self.disposition.as_deref() != Some("attachment") =>
            {
                Some(value)
            }
            Content::Parts(parts) => parts.iter().find_map(|part| part.first_text(wanted)),
            Content::Value(_) | Content::Blob(_) => None,
        }
    }
}
