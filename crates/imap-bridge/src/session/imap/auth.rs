// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The sign-in on the client library: the XOAUTH2 exchange and how a
//! refusal reads, without the server's own words.

use async_imap::Authenticator;
use async_imap::error::Error as ImapError;

use super::command_error;
use crate::session::SessionError;

/// The response code of a server whose sign-in backend is down (RFC 5530
/// section 3); the client library folds it into the text of its error.
const UNAVAILABLE_CODE: &str = "[UNAVAILABLE]";

/// The SASL XOAUTH2 exchange: the identity once, then an empty line so
/// the server's error challenge ends in its NO.
pub(super) struct Xoauth2 {
    pub(super) initial: Option<String>,
}

impl Authenticator for Xoauth2 {
    type Response = String;

    fn process(&mut self, _challenge: &[u8]) -> String {
        self.initial.take().unwrap_or_default()
    }
}

/// A NO answers the credential, unless its code says the server could
/// not judge it; a BAD is about the command.
pub(super) fn auth_error(error: ImapError) -> SessionError {
    match error {
        ImapError::No(text) if unavailable(&text) => SessionError::Unavailable,
        ImapError::No(_) | ImapError::Validate(_) => SessionError::CredentialRejected,
        ImapError::Bad(_) => SessionError::Protocol("the sign-in command was not accepted"),
        other => command_error(other),
    }
}

/// A response code is an atom, so its case is the server's choice.
fn unavailable(text: &str) -> bool {
    text.to_ascii_uppercase().contains(UNAVAILABLE_CODE)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The client library's text for a tagged NO: the code it parsed
    /// (it knows none from RFC 5530) and the information verbatim.
    fn no(information: &str) -> ImapError {
        ImapError::No(format!("code: None, info: Some({information:?})"))
    }

    #[test]
    fn a_no_with_the_unavailable_code_is_not_a_verdict_on_the_credential_rfc5530_3() {
        for text in [
            "[UNAVAILABLE] Temporary authentication failure.",
            "[unavailable] backend down",
        ] {
            assert!(
                matches!(auth_error(no(text)), SessionError::Unavailable),
                "{text}"
            );
        }
        for text in [
            "[AUTHENTICATIONFAILED] Authentication failed.",
            "Authentication failed.",
        ] {
            assert!(
                matches!(auth_error(no(text)), SessionError::CredentialRejected),
                "{text}"
            );
        }
    }

    #[test]
    fn a_no_to_any_other_command_is_a_refusal_and_a_bad_a_protocol_failure_rfc3501_7_1_2() {
        assert!(matches!(
            command_error(no("not now")),
            SessionError::Refused
        ));
        assert!(matches!(
            command_error(ImapError::Bad("unknown command".to_owned())),
            SessionError::Protocol("the server answered BAD")
        ));
    }
}
