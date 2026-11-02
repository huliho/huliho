// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Gmail's own rules on a label's name, which no RFC holds. The check before
//! a command answers what Gmail refuses on every try; a pass reads the names
//! beside each other after its NO.

use unicode_normalization::UnicodeNormalization as _;

use crate::utf7;

/// The UTF-16 code units of a whole path Gmail takes, its parents and
/// delimiters counted; past them it answers `NO [CANNOT]`.
const MAX_PATH_UNITS: usize = 225;

/// The bytes of UTF-8 of a whole path Gmail takes; past them it answers
/// `NO System Error`.
const MAX_PATH_BYTES: usize = 255;

/// Whether two white space characters of any kind stand in a row, which
/// Gmail answers with `NO [CANNOT]`.
pub(super) fn crowded(name: &str) -> bool {
    let spaces = name.chars().map(char::is_whitespace);
    spaces
        .clone()
        .zip(spaces.skip(1))
        .any(|(first, second)| first && second)
}

/// Whether a whole path, spelled as on the wire, passes what Gmail takes.
pub(super) fn past_limit(wire: &str) -> bool {
    let path = utf7::decode(wire);
    path.len() > MAX_PATH_BYTES || path.encode_utf16().count() > MAX_PATH_UNITS
}

/// A name as a pass reads it once Gmail refused one: whatever its case and
/// width, with every character that is no letter or digit as a space, since
/// Gmail reads `-`, `&` and some others as one. A name Gmail takes never gets
/// here, so this reading may reach wider than Gmail's own fold.
pub(super) fn beside(name: &str) -> String {
    name.nfkc()
        .map(|found| if found.is_alphanumeric() { found } else { ' ' })
        .collect::<String>()
        .to_lowercase()
}

#[cfg(test)]
mod tests;
