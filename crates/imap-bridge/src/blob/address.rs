// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The ids blobs go by: the email id for a whole message; for a part
//! the email id, a hyphen and the part number with its dots as
//! underscores, every character inside the alphabet of RFC 8620 section
//! 1.2. An id is computed both ways and never stored.

use crate::store::ID_LENGTH;

/// What stands between the email id and the part number.
const PART_MARK: char = '-';

/// What stands for a dot of the part number.
const LEVEL_MARK: char = '_';

/// What a blob id names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Address {
    pub email: String,
    /// The part number with its dots; `None` for the whole message.
    pub part: Option<String>,
}

impl Address {
    /// Reads a blob id; `None` for one the bridge never hands out. An
    /// email id has one length, so the part number starts at a fixed
    /// place behind it.
    pub(super) fn parse(blob_id: &str) -> Option<Self> {
        let (email, rest) = blob_id.split_at_checked(ID_LENGTH)?;
        let part = if rest.is_empty() {
            None
        } else {
            let number = rest.strip_prefix(PART_MARK)?;
            let levels = number
                .split(LEVEL_MARK)
                .all(|level| !level.is_empty() && level.bytes().all(|byte| byte.is_ascii_digit()));
            if !levels {
                return None;
            }
            Some(number.replace(LEVEL_MARK, "."))
        };
        Some(Self {
            email: email.to_owned(),
            part,
        })
    }
}

/// The blob id of a part: the email id, a hyphen and the part number
/// with its dots as underscores.
pub(crate) fn part_blob_id(email: &str, part_id: &str) -> String {
    format!("{email}{PART_MARK}{}", part_id.replace('.', "_"))
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::store::EmailId;

    #[test]
    fn an_email_id_names_the_whole_message_and_a_part_number_follows_a_hyphen_rfc8620_1_2() {
        let email = EmailId::generate().to_string();
        assert_eq!(
            Address::parse(&email),
            Some(Address {
                email: email.clone(),
                part: None
            })
        );
        let blob = part_blob_id(&email, "1.2.10");
        assert_eq!(blob, format!("{email}-1_2_10"));
        assert_eq!(
            Address::parse(&blob),
            Some(Address {
                email,
                part: Some("1.2.10".to_owned())
            })
        );
    }

    #[test]
    fn an_id_the_bridge_never_hands_out_names_nothing() {
        let email = EmailId::generate().to_string();
        let short = &email[..ID_LENGTH - 1];
        for tail in [
            "-", "-1_", "-_1", "-1__2", "-1.2", "-a", "-1_2 ", "1", "_1", "--1", "-1-2", "-\u{661}",
        ] {
            assert_eq!(Address::parse(&format!("{email}{tail}")), None, "{tail}");
        }
        assert_eq!(Address::parse(short), None);
        assert_eq!(Address::parse(""), None);
        // A character that straddles the fixed place of the split.
        assert_eq!(Address::parse(&format!("{short}\u{e9}-1")), None);
    }

    proptest! {
        #[test]
        fn a_part_blob_id_reads_back_as_its_email_and_its_part_number(
            levels in prop::collection::vec(1u32..2000, 1..8),
        ) {
            let email = EmailId::generate().to_string();
            let part: Vec<String> = levels.iter().map(u32::to_string).collect();
            let part = part.join(".");
            let blob = part_blob_id(&email, &part);
            prop_assert!(blob.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
            prop_assert_eq!(Address::parse(&blob), Some(Address { email, part: Some(part) }));
        }

        #[test]
        fn any_text_reads_as_an_address_or_as_none(text in any::<String>()) {
            let _ = Address::parse(&text);
        }
    }
}
