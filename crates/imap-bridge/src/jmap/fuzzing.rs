// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What the fuzz targets call: the readers of the bytes a body fetch
//! brings back, of a create object and of the message a submission
//! fetched back, which nothing outside the JMAP layer reaches otherwise.

use mail_parser::HeaderName;
use serde_json::Value;

use super::body::name;
use super::email_set::create;
use super::headers::{Form, HeaderAsk, value};
use super::values::decode;
use crate::message::{Blobs, Draft, Stamp, build, names, recipients, sendable};
use crate::session::{Disposition, Leaf};

/// The cap the fuzzed values are cut at, so a cut on a character border
/// runs on most inputs.
const FUZZ_CAP: u32 = 64;

/// The address of the one identity the fuzzed messages are read against.
const IDENTITY: &str = "sanne@example.test";

/// Header bytes a sender wrote through both forms, the same bytes as a
/// parameter name and value through the name decoder, then as a text
/// part under every transfer encoding through the decoder, whole and
/// cut.
pub fn body_bytes(bytes: &[u8]) {
    for form in [Form::Raw, Form::Text] {
        for all in [false, true] {
            let ask = HeaderAsk {
                key: String::new(),
                name: "Authentication-Results".to_owned(),
                form,
                all,
            };
            let _ = value(&ask, bytes);
        }
    }
    let text = String::from_utf8_lossy(bytes).into_owned();
    let parameters = vec![
        (text.clone(), text.clone()),
        ("name*".to_owned(), text.clone()),
    ];
    let disposition = Disposition {
        kind: "attachment".to_owned(),
        parameters: vec![
            (text.clone(), text.clone()),
            ("filename*0*".to_owned(), text),
        ],
    };
    let _ = name(Some(&disposition), &parameters);
    for encoding in ["7bit", "base64", "quoted-printable", "x-unknown"] {
        let leaf = Leaf {
            media_type: "text".to_owned(),
            subtype: "html".to_owned(),
            parameters: vec![("charset".to_owned(), "iso-2022-jp".to_owned())],
            encoding: encoding.to_owned(),
            bytes: u32::try_from(bytes.len()).unwrap_or(u32::MAX),
            ..Leaf::default()
        };
        let _ = decode(&leaf, bytes, FUZZ_CAP, true);
        let _ = decode(&leaf, bytes, u32::MAX, false);
    }
}

/// The create object of a client through the reader and, where it reads,
/// the builder; every blob it names reads as these bytes.
pub fn draft_object(bytes: &[u8]) {
    let Ok(object) = serde_json::from_slice::<Value>(bytes) else {
        return;
    };
    let Ok(draft) = create::read(&object) else {
        return;
    };
    let blobs = blobs_of(&draft, bytes);
    let stamp = Stamp::now(&draft);
    let _ = build(&draft, &stamp, &blobs);
    let _ = draft.stamped(&stamp);
    let _ = draft.has_attachment();
}

/// The message of a submission as the bridge fetched it back, through
/// the recipient reader, with the recipients of the message and with a
/// given list, then through the header rewrite into both forms behind
/// the check of the send path. Whether that check refuses the message
/// or the wire form, read by the recipient reader's parser, holds no
/// Bcc field and names the draft's fields in order, Bcc and Date aside,
/// with one Date.
#[must_use]
pub fn submitted_message(bytes: &[u8]) -> bool {
    let _ = recipients(bytes, IDENTITY, None);
    let given = [IDENTITY.to_owned()];
    let _ = recipients(bytes, IDENTITY, Some(&given));
    let Ok(forms) = sendable(bytes, 0) else {
        return true;
    };
    let blind = |name: &HeaderName<'_>| matches!(name, HeaderName::Bcc | HeaderName::ResentBcc);
    let dated = |name: &HeaderName<'_>| *name == HeaderName::Date;
    let draft = names(bytes);
    let wire = names(&forms.wire);
    let kept = draft.iter().filter(|name| !blind(name) && !dated(name));
    !wire.iter().any(blind)
        && wire.iter().filter(|name| dated(name)).count() == 1
        && wire.iter().filter(|name| !dated(name)).eq(kept)
}

fn blobs_of(draft: &Draft, bytes: &[u8]) -> Blobs {
    let ids = draft.blob_ids().into_iter();
    ids.map(|id| (id.to_owned(), bytes.to_vec())).collect()
}
