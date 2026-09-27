// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What a blob answer says about itself: the type from the first bytes
//! against the type the client asked for, the disposition toward the
//! download, the name it saves under, the headers every blob carries
//! and the one range the route honors.

use std::time::Duration;

use axum::http::header::{
    CACHE_CONTROL, CONTENT_SECURITY_POLICY, HeaderName, HeaderValue, REFERRER_POLICY,
    X_CONTENT_TYPE_OPTIONS,
};
use icu_properties::{CodePointSetData, props::DefaultIgnorableCodePoint};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

use super::detect::raster_type;

/// Downloads in flight per account, beside the request cap, so a long
/// download starves no request and one account holds two lanes at most.
pub const MAX_CONCURRENT_DOWNLOADS: usize = 2;

/// One blob at most; larger than any attachment a mail server accepts.
pub const BLOB_DOWNLOAD_LIMIT: u64 = 64 * 1024 * 1024;

/// A blob never changes under its id, so the browser may keep it for a
/// day.
pub const BLOB_CACHE_SECONDS: u64 = 86_400;

/// A stream that stays silent this long is dead; the same patience a
/// download waits for a free lane.
pub const DOWNLOAD_IDLE_TIMEOUT: Duration = Duration::from_secs(20);

/// One download in total, connect to last byte; the largest blob on the
/// slowest link the route serves.
pub const DOWNLOAD_TOTAL_TIMEOUT: Duration = Duration::from_mins(10);

/// A file name longer than this is a paragraph; every file system stops
/// here too.
pub const MAX_NAME_BYTES: usize = 255;

/// Every character outside the unreserved set of RFC 3986 section 2.3,
/// percent-encoded wherever a value goes into a URL template (RFC 6570
/// level 1) or a header parameter (RFC 8187), so no value reaches past
/// its slot.
pub const ENCODED: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// What a name becomes when nothing printable is left of it.
const NAMELESS: &str = "download";

/// Whether a character carries the Unicode property
/// `Default_Ignorable_Code_Point`: the format controls, joiners, fillers
/// and variation selectors that show nothing of their own. With one of
/// them an extension reads as another.
fn invisible(c: char) -> bool {
    CodePointSetData::new::<DefaultIgnorableCodePoint>().contains(c)
}

/// The type of every blob that is not a raster image asked as itself.
const OCTET_STREAM: &str = "application/octet-stream";

/// A blob opened in a tab of its own renders nothing and fetches
/// nothing.
const BLOB_POLICY: &str = "sandbox; default-src 'none'";

const CROSS_ORIGIN_RESOURCE_POLICY: HeaderName =
    HeaderName::from_static("cross-origin-resource-policy");

/// The type the answer carries and whether it renders inline: a raster
/// type the bytes carry, asked as that type, inline; everything else a
/// download as bytes, whatever the sender or the request said.
#[must_use]
pub fn served_type(head: &[u8], requested: Option<&str>) -> (&'static str, bool) {
    let requested = requested
        .and_then(|value| value.split(';').next())
        .map(str::trim);
    match raster_type(head) {
        Some(raster) if requested.is_some_and(|asked| asked.eq_ignore_ascii_case(raster)) => {
            (raster, true)
        }
        _ => (OCTET_STREAM, false),
    }
}

/// The name a blob saves under: the given one without control or
/// invisible characters, cut at a character boundary within the bound.
#[must_use]
pub fn clean_name(name: &str) -> String {
    let mut cleaned: String = name
        .chars()
        .filter(|c| !c.is_control() && !invisible(*c))
        .collect();
    let mut cut = cleaned.len().min(MAX_NAME_BYTES);
    while !cleaned.is_char_boundary(cut) {
        cut -= 1;
    }
    cleaned.truncate(cut);
    if cleaned.is_empty() {
        cleaned.push_str(NAMELESS);
    }
    cleaned
}

/// The disposition with the name encoded as RFC 6266 section 4.3
/// through RFC 8187 has it, so any name travels.
#[must_use]
pub fn disposition(inline: bool, name: &str) -> HeaderValue {
    let kind = if inline { "inline" } else { "attachment" };
    let encoded = utf8_percent_encode(name, ENCODED);
    HeaderValue::from_str(&format!("{kind}; filename*=UTF-8''{encoded}"))
        .unwrap_or_else(|_| HeaderValue::from_static("attachment"))
}

/// The headers every blob answer carries: no sniffing, a policy that
/// renders and fetches nothing, no use across origins, no referrer and
/// a day in the browser's cache; under `strict` the browser keeps
/// nothing.
#[must_use]
pub fn blob_headers(strict: bool) -> [(HeaderName, HeaderValue); 5] {
    let cache = if strict {
        HeaderValue::from_static("no-store")
    } else {
        HeaderValue::from_str(&format!("private, max-age={BLOB_CACHE_SECONDS}"))
            .unwrap_or_else(|_| HeaderValue::from_static("no-store"))
    };
    [
        (X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")),
        (
            CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(BLOB_POLICY),
        ),
        (
            CROSS_ORIGIN_RESOURCE_POLICY,
            HeaderValue::from_static("same-origin"),
        ),
        (REFERRER_POLICY, HeaderValue::from_static("no-referrer")),
        (CACHE_CONTROL, cache),
    ]
}

/// The last byte of a `Range` from byte zero (`bytes=0-N`). Every other
/// range is one RFC 9110 section 14.2 lets a server ignore; the route
/// then answers the whole blob.
#[must_use]
pub fn range_end(header: Option<&HeaderValue>) -> Option<u64> {
    header?
        .to_str()
        .ok()?
        .trim()
        .strip_prefix("bytes=0-")?
        .parse()
        .ok()
}

/// How many bytes a range from zero cuts the blob to: only where the
/// length is declared and the range ends before it, so the answer's
/// range header is exact.
#[must_use]
pub fn range_cut(declared: Option<u64>, end: Option<u64>) -> Option<u64> {
    let wanted = end?.checked_add(1)?;
    (declared? > wanted).then_some(wanted)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";

    fn header(value: &str) -> HeaderValue {
        HeaderValue::from_str(value).unwrap()
    }

    #[test]
    fn a_raster_asked_as_itself_is_inline_and_everything_else_a_download() {
        assert_eq!(served_type(PNG, Some("image/png")), ("image/png", true));
        assert_eq!(
            served_type(PNG, Some("IMAGE/PNG; q=1")),
            ("image/png", true)
        );
        assert_eq!(served_type(PNG, Some("image/jpeg")), (OCTET_STREAM, false));
        assert_eq!(served_type(PNG, None), (OCTET_STREAM, false));
        assert_eq!(
            served_type(
                b"<svg xmlns='http://www.w3.org/2000/svg'/>",
                Some("image/svg+xml")
            ),
            (OCTET_STREAM, false)
        );
        assert_eq!(
            served_type(b"<html>x</html>", Some("image/png")),
            (OCTET_STREAM, false)
        );
    }

    #[test]
    fn a_name_loses_its_control_characters_and_is_cut_within_the_bound() {
        assert_eq!(clean_name("report\r\n.pdf\u{7f}"), "report.pdf");
        assert_eq!(clean_name("photo\u{202E}gnp.exe"), "photognp.exe");
        assert_eq!(clean_name("\u{FEFF}a\u{200B}b\u{2066}c"), "abc");
        assert_eq!(
            clean_name("re\u{00AD}port\u{061C}.\u{180E}p\u{2060}d\u{2063}f"),
            "report.pdf"
        );
        assert_eq!(
            clean_name("re\u{034F}port\u{115F}.\u{FE0F}pd\u{E0041}f"),
            "report.pdf"
        );
        assert_eq!(clean_name("\u{1}\u{2}\u{200F}"), NAMELESS);
        assert_eq!(clean_name(""), NAMELESS);
        let long = format!("{}é", "a".repeat(MAX_NAME_BYTES - 1));
        let cut = clean_name(&long);
        assert_eq!(cut.len(), MAX_NAME_BYTES - 1);
        assert!(cut.is_char_boundary(cut.len()));
        assert_eq!(clean_name(&"x".repeat(1000)).len(), MAX_NAME_BYTES);
    }

    #[test]
    fn the_disposition_encodes_the_name_rfc6266_4_3() {
        assert_eq!(
            disposition(true, "photo.png"),
            "inline; filename*=UTF-8''photo.png"
        );
        assert_eq!(
            disposition(false, "Q3 été \"report\".pdf"),
            "attachment; filename*=UTF-8''Q3%20%C3%A9t%C3%A9%20%22report%22.pdf"
        );
    }

    #[test]
    fn every_blob_answer_carries_the_five_headers_and_no_store_under_strict() {
        let open = blob_headers(false);
        let names: Vec<&str> = open.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            names,
            [
                "x-content-type-options",
                "content-security-policy",
                "cross-origin-resource-policy",
                "referrer-policy",
                "cache-control"
            ]
        );
        assert_eq!(open[1].1, BLOB_POLICY);
        assert_eq!(open[4].1, "private, max-age=86400");
        assert_eq!(blob_headers(true)[4].1, "no-store");
    }

    #[test]
    fn only_a_range_from_byte_zero_counts_rfc9110_14_2() {
        assert_eq!(range_end(Some(&header("bytes=0-99"))), Some(99));
        assert_eq!(range_end(Some(&header(" bytes=0-0 "))), Some(0));
        for other in [
            "bytes=1-99",
            "bytes=0-",
            "bytes=-100",
            "bytes=0-99,200-",
            "items=0-9",
            "",
        ] {
            assert_eq!(range_end(Some(&header(other))), None, "{other}");
        }
        assert_eq!(range_end(None), None);
    }

    #[test]
    fn a_range_cuts_only_a_declared_blob_that_runs_past_it() {
        assert_eq!(range_cut(Some(1000), Some(99)), Some(100));
        assert_eq!(range_cut(Some(100), Some(99)), None);
        assert_eq!(range_cut(Some(50), Some(99)), None);
        assert_eq!(range_cut(None, Some(99)), None);
        assert_eq!(range_cut(Some(1000), None), None);
        assert_eq!(range_cut(Some(1000), Some(u64::MAX)), None);
    }
}
