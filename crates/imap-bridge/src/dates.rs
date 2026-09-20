// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Seconds since the epoch as the `UTCDate` of RFC 8620 section 1.4.

use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// The timestamp in RFC 3339 form with `Z`; `None` outside the years
/// the form can write.
pub(crate) fn utc_date(timestamp: i64) -> Option<String> {
    OffsetDateTime::from_unix_timestamp(timestamp)
        .ok()?
        .format(&Rfc3339)
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_timestamp_renders_as_a_utc_date_rfc8620_1_4() {
        assert_eq!(utc_date(0).as_deref(), Some("1970-01-01T00:00:00Z"));
        assert_eq!(
            utc_date(836_732_665).as_deref(),
            Some("1996-07-07T09:44:25Z")
        );
        assert_eq!(utc_date(i64::MAX), None);
        assert_eq!(utc_date(-100_000_000_000), None);
    }
}
