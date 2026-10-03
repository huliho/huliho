// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The window decoders against mail-parser's decode of a whole part
//! and against bytes no encoder writes.

use mail_parser::MessageParser;
use proptest::prelude::*;

use super::*;
use crate::testing::parts::base64_lines;

/// The longest encoded line a sender writes (RFC 2045 section 6.7).
const LINE: usize = 76;

/// How many other bytes a generated long line holds per line break, so
/// most of its lines run past `LONG_LINE`.
const LONG_LINE_WEIGHT: u32 = 2000;

/// The bytes through a decoder in windows of the given sizes, the
/// sizes taken round and round.
fn streamed(encoding: &str, bytes: &[u8], sizes: &[usize]) -> Vec<u8> {
    let mut decoder = Decoder::of(encoding);
    let mut content = Vec::new();
    let mut rest = bytes;
    let mut sizes = sizes.iter().copied().cycle();
    loop {
        let size = sizes.next().unwrap_or(1).clamp(1, rest.len().max(1));
        let (window, tail) = rest.split_at(size.min(rest.len()));
        rest = tail;
        content.extend(decoder.push(window.to_vec(), rest.is_empty()).unwrap());
        if rest.is_empty() {
            return content;
        }
    }
}

/// What mail-parser decodes of a message that is this one part.
fn whole(encoding: &str, bytes: &[u8]) -> Vec<u8> {
    let mut message = format!(
        "Content-Type: application/octet-stream\r\nContent-Transfer-Encoding: {encoding}\r\n\r\n"
    )
    .into_bytes();
    message.extend_from_slice(bytes);
    let parsed = MessageParser::default().parse(&message).unwrap();
    parsed.parts[0].contents().to_vec()
}

/// Quoted-printable of RFC 2045 section 6.7: a CRLF in the content is a
/// hard break, every other byte outside the printable set and every
/// space is escaped and a long line breaks softly.
fn quoted_printable(content: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut width = 0;
    let mut rest = content;
    while let Some((byte, tail)) = rest.split_first() {
        if rest.starts_with(b"\r\n") {
            out.extend_from_slice(b"\r\n");
            width = 0;
            rest = &rest[2..];
            continue;
        }
        let piece = match byte {
            33..=60 | 62..=126 => vec![*byte],
            _ => format!("={byte:02X}").into_bytes(),
        };
        if width + piece.len() >= LINE {
            out.extend_from_slice(b"=\r\n");
            width = 0;
        }
        width += piece.len();
        out.extend(piece);
        rest = tail;
    }
    out
}

#[test]
fn an_encoding_the_bridge_does_not_undo_passes_the_bytes_as_they_stand() {
    for encoding in ["7bit", "8bit", "binary", "x-uuencode", ""] {
        assert!(Decoder::of(encoding).is_identity(), "{encoding}");
        assert_eq!(streamed(encoding, b"a=3D\r\nYQ==", &[3]), b"a=3D\r\nYQ==");
    }
    assert!(Decoder::identity().is_identity());
    assert!(!Decoder::of("base64").is_identity());
    assert!(!Decoder::of("quoted-printable").is_identity());
}

#[test]
fn base64_decodes_across_a_window_edge_inside_a_quartet_rfc2045_6_8() {
    let encoded = b"SGVs\r\nbG8g\r\nd29ybGQ=\r\n";
    for size in 1..encoded.len() {
        assert_eq!(
            streamed("base64", encoded, &[size]),
            b"Hello world",
            "{size}"
        );
    }
    assert_eq!(quartets(b"SGVsbG8"), 4);
    // A pad ends its quartet, the four symbols after it are one more.
    assert_eq!(quartets(b"SG=sbG8gb"), 7);
    assert_eq!(quartets(b"SG="), 3);
    assert_eq!(quartets(b""), 0);
}

#[test]
fn base64_ignores_a_byte_outside_the_alphabet_and_reads_on_past_a_pad() {
    let noisy = b"SGVs!bG8g -- d29y\x00bGQ=";
    assert_eq!(streamed("base64", noisy, &[5]), b"Hello world");
    // Two bodies written one after the other, each padded.
    let joined = b"SGk=SGk=";
    assert_eq!(streamed("base64", joined, &[3]), b"HiHi");
    // A last quartet without its pads still gives its bytes.
    assert_eq!(streamed("base64", b"SGVsbG8", &[2]), b"Hello");
}

#[test]
fn quoted_printable_keeps_an_escape_a_soft_break_and_a_line_end_whole_rfc2045_6_7() {
    let encoded = b"Caf=C3=A9 om=\r\n drie  \r\nuur=3D.\r\n";
    let expected = "Caf\u{e9} om drie\r\nuur=.\r\n".as_bytes();
    for size in 1..encoded.len() {
        assert_eq!(
            streamed("quoted-printable", encoded, &[size]),
            expected,
            "{size}"
        );
    }
    assert_eq!(whole("quoted-printable", encoded), expected);
    assert_eq!(lines(b"ab\r\ncd=\r"), 4);
    assert_eq!(lines(b"abcd=4"), 0);
    assert_eq!(lines(b""), 0);
}

#[test]
fn an_equals_sign_that_opens_no_escape_is_read_as_itself_and_nothing_is_lost_rfc2045_6_7() {
    let cases: [(&[u8], &[u8]); 9] = [
        (b"a==b", b"a==b"),
        (b"a=4=b", b"a=4=b"),
        (b"a=zzb", b"a=zzb"),
        (
            b"sig=xyz== x\r\nnext=C3=A9\r\n",
            b"sig=xyz== x\r\nnext\xc3\xa9\r\n",
        ),
        // The last `=` of a line is its soft break.
        (b"sig=xyz==\r\nnext", b"sig=xyz=next"),
        (b"dangling=A\r\nB", b"dangling=A\r\nB"),
        (b"end=", b"end="),
        (b"soft= \t\r\nbreak", b"softbreak"),
        (b"bare\nline\r\n", b"bare\r\nline\r\n"),
    ];
    for (encoded, expected) in cases {
        for size in 1..=encoded.len() {
            assert_eq!(
                streamed("quoted-printable", encoded, &[size]),
                expected,
                "{encoded:?} in windows of {size}"
            );
        }
    }
    assert!(opens(b"C3"));
    assert!(opens(b"\n"));
    assert!(!opens(b"C"));
    assert!(!opens(b""));
    assert!(!opens(b" x\n"));
}

#[test]
fn a_quoted_printable_line_past_the_longest_a_message_may_carry_is_cut_where_it_stands() {
    let long = vec![b'x'; LONG_LINE];
    assert_eq!(lines(&long), LONG_LINE);
    for tail in [&b"=4"[..], b"=", b"\r"] {
        let open = [&long[..], tail].concat();
        assert_eq!(lines(&open), LONG_LINE, "{tail:?}");
    }
    let encoded = [&long[..], b"=41=\r\n", &long[..], b"=42"].concat();
    let expected = [&long[..], b"A", &long[..], b"B"].concat();
    for size in [1, 7, LONG_LINE, LONG_LINE + 1] {
        assert_eq!(
            streamed("quoted-printable", &encoded, &[size]),
            expected,
            "{size}"
        );
    }
    assert_eq!(whole("quoted-printable", &encoded), expected);
}

proptest! {
    #[test]
    fn any_bytes_under_base64_decode_as_the_symbols_among_them_in_any_windows(
        bytes in prop::collection::vec(any::<u8>(), 0..400),
        sizes in prop::collection::vec(1usize..90, 1..6),
    ) {
        let symbols: Vec<u8> = bytes
            .iter()
            .copied()
            .filter(|byte| symbol(*byte) || *byte == PAD)
            .collect();
        prop_assert_eq!(
            streamed("base64", &bytes, &sizes),
            base64_decode(&symbols).unwrap()
        );
    }

    #[test]
    fn a_base64_part_in_any_windows_equals_the_decode_of_the_whole_part(
        content in prop::collection::vec(any::<u8>(), 0..2000),
        sizes in prop::collection::vec(1usize..300, 1..6),
    ) {
        let encoded = base64_lines(&content).into_bytes();
        let decoded = streamed("base64", &encoded, &sizes);
        prop_assert_eq!(&decoded, &whole("base64", &encoded));
        prop_assert_eq!(decoded, content);
    }

    #[test]
    fn a_quoted_printable_part_in_any_windows_equals_the_decode_of_the_whole_part(
        lines in prop::collection::vec(prop::collection::vec(any::<u8>(), 0..120), 1..12),
        sizes in prop::collection::vec(1usize..300, 1..6),
    ) {
        let content = lines.join(&b"\r\n"[..]);
        let encoded = quoted_printable(&content);
        let decoded = streamed("quoted-printable", &encoded, &sizes);
        prop_assert_eq!(&decoded, &whole("quoted-printable", &encoded));
        prop_assert_eq!(decoded, content);
    }

    #[test]
    fn any_bytes_under_quoted_printable_decode_the_same_in_any_windows_and_none_is_given_up(
        bytes in prop::collection::vec(
            prop_oneof![any::<u8>(), prop::sample::select(b"==\r\n \tA4z".to_vec())],
            0..400,
        ),
        sizes in prop::collection::vec(1usize..90, 1..6),
    ) {
        let whole = streamed("quoted-printable", &bytes, &[bytes.len().max(1)]);
        prop_assert_eq!(&streamed("quoted-printable", &bytes, &sizes), &whole);
        prop_assert_eq!(unquoted(&bytes), Some(whole.clone()));
        // A line end decodes to CRLF, so a byte gives two at most.
        prop_assert!(whole.len() <= 2 * bytes.len());
        // What is neither an escape, a line end nor white space stays.
        let plain = |byte: &&u8| byte.is_ascii_alphanumeric() && !byte.is_ascii_hexdigit();
        prop_assert!(whole.iter().filter(plain).count() >= bytes.iter().filter(plain).count());
    }

    #[test]
    fn quoted_printable_lines_past_the_longest_give_no_window_up_and_keep_every_plain_byte(
        bytes in prop::collection::vec(
            prop_oneof![
                LONG_LINE_WEIGHT => prop::sample::select(b"== \t\rA4zxq".to_vec()),
                1 => Just(b'\n'),
            ],
            LONG_LINE..3 * LONG_LINE,
        ),
        sizes in prop::collection::vec(1usize..LONG_LINE, 1..6),
    ) {
        // `streamed` fails on a window its decoder gives up.
        let decoded = streamed("quoted-printable", &bytes, &sizes);
        let plain = |byte: &&u8| matches!(byte, b'z' | b'x' | b'q');
        prop_assert_eq!(
            decoded.iter().filter(plain).count(),
            bytes.iter().filter(plain).count()
        );
    }
}
