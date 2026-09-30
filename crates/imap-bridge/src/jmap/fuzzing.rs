// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What the fuzz targets call: the readers of the bytes a body fetch
//! brings back, which nothing outside the JMAP layer reaches otherwise.

use super::body::name;
use super::headers::{Form, HeaderAsk, value};
use super::values::decode;
use crate::session::{Disposition, Leaf};

/// The cap the fuzzed values are cut at, so a cut on a character border
/// runs on most inputs.
const FUZZ_CAP: u32 = 64;

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
