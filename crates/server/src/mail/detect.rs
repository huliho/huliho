// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What a blob is, read from its first bytes rather than from the name
//! the sender or the request gave it.

/// The raster types a browser renders in place; asked as that type and
/// carrying its signature, a blob answers inline. Everything else is a
/// download.
pub const RASTER_TYPES: [&str; 6] = [
    "image/png",
    "image/jpeg",
    "image/gif",
    "image/webp",
    "image/avif",
    "image/bmp",
];

/// Enough of a blob to hold any raster signature: the longest sits
/// inside the first sixteen bytes and a container's box header well
/// within this.
pub const DETECT_BYTES: usize = 512;

/// The raster type the first bytes name, as one of [`RASTER_TYPES`];
/// `None` for any other bytes, an SVG and an HTML document included.
#[must_use]
pub fn raster_type(head: &[u8]) -> Option<&'static str> {
    let found = infer::get(head)?.mime_type();
    RASTER_TYPES.into_iter().find(|raster| *raster == found)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";
    const JPEG: &[u8] = b"\xff\xd8\xff\xe0\0\x10JFIF";
    const GIF: &[u8] = b"GIF89a\x01\0\x01\0";
    const WEBP: &[u8] = b"RIFF\x24\0\0\0WEBPVP8 ";
    const AVIF: &[u8] = b"\0\0\0\x18ftypavif\0\0\0\0avifmif1";
    const BMP: &[u8] = b"BM\x3a\0\0\0\0\0\0\0\x36\0\0\0";

    #[test]
    fn the_six_raster_types_are_read_from_their_signatures() {
        for (bytes, expected) in [
            (PNG, "image/png"),
            (JPEG, "image/jpeg"),
            (GIF, "image/gif"),
            (WEBP, "image/webp"),
            (AVIF, "image/avif"),
            (BMP, "image/bmp"),
        ] {
            assert_eq!(raster_type(bytes), Some(expected));
        }
    }

    #[test]
    fn text_a_document_an_svg_and_a_pdf_are_no_raster_type() {
        for bytes in [
            &b""[..],
            b"<html><body>x</body></html>",
            b"<?xml version=\"1.0\"?><svg xmlns=\"http://www.w3.org/2000/svg\"/>",
            b"%PDF-1.7\n",
            b"\x89PN",
            b"plain words",
        ] {
            assert_eq!(raster_type(bytes), None, "{bytes:?}");
        }
    }
}
