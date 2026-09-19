// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Mailbox names in modified UTF-7 (RFC 3501 section 5.1.3): printable
//! ASCII as itself, `&` as `&-` and every other run as `&`, the base64
//! of its UTF-16 with `,` in place of `/` and no padding, then `-`.

use base64::Engine as _;
use base64::alphabet::Alphabet;
use base64::engine::DecodePaddingMode;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig};

const MODIFIED_ALPHABET: Alphabet =
    match Alphabet::new("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+,") {
        Ok(alphabet) => alphabet,
        Err(_) => panic!("the modified base64 alphabet is 64 distinct printable characters"),
    };

const MODIFIED_BASE64: GeneralPurpose = GeneralPurpose::new(
    &MODIFIED_ALPHABET,
    GeneralPurposeConfig::new()
        .with_encode_padding(false)
        .with_decode_padding_mode(DecodePaddingMode::RequireNone),
);

const SHIFT: char = '&';
const UNSHIFT: char = '-';

/// Whether a character travels as itself: printable ASCII except the
/// shift character.
fn is_direct(c: char) -> bool {
    matches!(c, ' '..='~') && c != SHIFT
}

/// Decodes a name the server sent. A run that is not valid modified
/// UTF-7 stays as received, so a name never disappears from the tree.
#[must_use]
pub fn decode(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut rest = name;
    while let Some(start) = rest.find(SHIFT) {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find(UNSHIFT) else {
            out.push_str(&rest[start..]);
            return out;
        };
        let run = &after[..end];
        match decode_run(run) {
            Some(text) => out.push_str(&text),
            None => out.push_str(&rest[start..=start + 1 + end]),
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// The text of one shifted run; the empty run is the shift character
/// itself.
fn decode_run(run: &str) -> Option<String> {
    if run.is_empty() {
        return Some(SHIFT.to_string());
    }
    let bytes = MODIFIED_BASE64.decode(run).ok()?;
    if bytes.len() % 2 != 0 {
        return None;
    }
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .copied()
        .map(u16::from_be_bytes)
        .collect();
    String::from_utf16(&units).ok()
}

/// Encodes a name for the wire.
#[must_use]
pub fn encode(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut run: Vec<u8> = Vec::new();
    for c in name.chars() {
        if is_direct(c) {
            flush(&mut out, &mut run);
            out.push(c);
        } else if c == SHIFT {
            flush(&mut out, &mut run);
            out.push(SHIFT);
            out.push(UNSHIFT);
        } else {
            let mut units = [0u16; 2];
            for unit in c.encode_utf16(&mut units) {
                run.extend_from_slice(&unit.to_be_bytes());
            }
        }
    }
    flush(&mut out, &mut run);
    out
}

fn flush(out: &mut String, run: &mut Vec<u8>) {
    if run.is_empty() {
        return;
    }
    out.push(SHIFT);
    out.push_str(&MODIFIED_BASE64.encode(&run));
    out.push(UNSHIFT);
    run.clear();
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn the_examples_of_rfc3501_5_1_3_decode_and_encode() {
        assert_eq!(
            decode("~peter/mail/&U,BTFw-/&ZeVnLIqe-"),
            "~peter/mail/台北/日本語"
        );
        assert_eq!(
            encode("~peter/mail/台北/日本語"),
            "~peter/mail/&U,BTFw-/&ZeVnLIqe-"
        );
        assert_eq!(decode("Rock &- Roll"), "Rock & Roll");
        assert_eq!(encode("Rock & Roll"), "Rock &- Roll");
    }

    #[test]
    fn a_run_that_is_not_utf7_stays_as_received() {
        assert_eq!(decode("Odd &*-"), "Odd &*-");
        assert_eq!(decode("Open &U,BT"), "Open &U,BT");
        assert_eq!(decode("Short &QQ-"), "Short &QQ-");
        assert_eq!(decode("Lone &"), "Lone &");
        assert_eq!(decode("Half &2D0-"), "Half &2D0-");
    }

    #[test]
    fn a_character_outside_the_basic_plane_travels_as_a_surrogate_pair() {
        let name = "Mail 📬";
        assert_eq!(decode(&encode(name)), name);
    }

    proptest! {
        #[test]
        fn every_name_survives_a_round_trip(name in ".*") {
            prop_assert_eq!(decode(&encode(&name)), name);
        }

        #[test]
        fn an_encoded_name_is_printable_ascii(name in ".*") {
            prop_assert!(encode(&name).chars().all(|c| matches!(c, ' '..='~')));
        }
    }
}
