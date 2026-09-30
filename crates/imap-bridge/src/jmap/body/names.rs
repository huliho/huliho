// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The name and the Content-ID of a part as RFC 8621 section 4.1.4
//! wants them: the file name decoded per RFC 2231 and RFC 2047, the id
//! without its angle brackets.

use mail_parser::{HeaderName, MessageParser};

use crate::jmap::values::parameter;
use crate::session::Disposition;

/// Content-ID without its angle brackets.
pub(super) fn cid(id: &str) -> &str {
    id.trim()
        .strip_prefix('<')
        .and_then(|inner| inner.strip_suffix('>'))
        .unwrap_or(id.trim())
}

/// The decoded `filename` of the disposition, else the `name` of the
/// type, RFC 2231 and RFC 2047 undone by the header parser: the
/// parameters go back on the line they came from and are read again.
pub(crate) fn name(
    disposition: Option<&Disposition>,
    parameters: &[(String, String)],
) -> Option<String> {
    let filename = disposition.and_then(|disposition| {
        decoded_parameter(
            HeaderName::ContentDisposition,
            &disposition.parameters,
            "filename",
        )
    });
    filename.or_else(|| decoded_parameter(HeaderName::ContentType, parameters, "name"))
}

fn decoded_parameter(
    header: HeaderName<'static>,
    parameters: &[(String, String)],
    name: &str,
) -> Option<String> {
    parameter(parameters, name).or_else(|| {
        let extended = format!("{name}*");
        parameters
            .iter()
            .find(|(found, _)| {
                found
                    .get(..extended.len())
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case(&extended))
            })
            .map(|(_, value)| value.clone())
    })?;
    let mut line = format!("{}: x", header.as_str());
    for (found, value) in parameters {
        line.push_str("; ");
        line.push_str(found);
        line.push('=');
        if found.ends_with('*') {
            line.push_str(value);
        } else {
            line.push('"');
            line.push_str(&value.replace('\\', "\\\\").replace('"', "\\\""));
            line.push('"');
        }
    }
    line.push_str("\r\n\r\n");
    let parsed = MessageParser::default().parse_headers(line.as_bytes())?;
    let content = parsed.header(header)?.as_content_type()?;
    content.attribute(name).map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(name: &str, value: &str) -> Vec<(String, String)> {
        vec![(name.to_owned(), value.to_owned())]
    }

    fn attached(parameters: Vec<(String, String)>) -> Disposition {
        Disposition {
            kind: "attachment".to_owned(),
            parameters,
        }
    }

    #[test]
    fn a_name_comes_from_the_disposition_first_and_decodes_rfc2231_and_rfc2047() {
        assert_eq!(
            name(
                Some(&attached(named("FILENAME", "a.pdf"))),
                &named("NAME", "b.pdf")
            ),
            Some("a.pdf".to_owned())
        );
        assert_eq!(
            name(None, &named("NAME", "b.pdf")),
            Some("b.pdf".to_owned())
        );
        assert_eq!(name(None, &[]), None);
        assert_eq!(
            name(
                Some(&attached(named("FILENAME*", "utf-8''caf%C3%A9.pdf"))),
                &[]
            ),
            Some("caf\u{e9}.pdf".to_owned())
        );
        let continued = vec![
            ("FILENAME*1*".to_owned(), "%C3%A9%20om.pdf".to_owned()),
            ("FILENAME*0*".to_owned(), "utf-8''caf".to_owned()),
        ];
        assert_eq!(
            name(Some(&attached(continued)), &[]),
            Some("caf\u{e9} om.pdf".to_owned())
        );
        assert_eq!(
            name(None, &named("name", "=?UTF-8?Q?Caf=C3=A9_om.pdf?=")),
            Some("Caf\u{e9} om.pdf".to_owned())
        );
        assert_eq!(
            name(None, &named("name", "a \"quoted\" \\ name.pdf")),
            Some("a \"quoted\" \\ name.pdf".to_owned())
        );
    }

    #[test]
    fn a_parameter_name_outside_ascii_is_passed_over() {
        assert_eq!(name(None, &named("name\u{e9}", "x.pdf")), None);
        assert_eq!(name(None, &named("nam\u{e9}*", "x.pdf")), None);
        let beside = vec![
            ("name\u{e9}".to_owned(), "x.pdf".to_owned()),
            ("NAME*".to_owned(), "utf-8''b.pdf".to_owned()),
        ];
        assert_eq!(name(None, &beside), Some("b.pdf".to_owned()));
    }

    #[test]
    fn a_content_id_loses_its_brackets_and_its_spaces() {
        assert_eq!(cid(" <a@b> "), "a@b");
        assert_eq!(cid("a@b"), "a@b");
        assert_eq!(cid("<a@b"), "<a@b");
    }
}
