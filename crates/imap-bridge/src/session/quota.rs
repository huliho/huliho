// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! GETQUOTAROOT of the inbox (RFC 9208 section 4.1.2): the quota roots
//! it falls under, each with the usage and the limit of its storage and
//! its message count. The QUOTAROOT and QUOTA lines are read under the
//! line bounds and every value under the bounds of RFC 9208.

use async_imap::imap_proto::{self, QuotaResourceName, Response};

use super::read::{Room, Selection, wire_name};
use super::{QuotaRoot, SessionError, Usage};

/// The roots one mailbox may fall under; the servers met name one, a
/// user root beside a domain root at most.
pub const MAX_QUOTA_ROOTS: usize = 16;

/// The octets in one unit of STORAGE (RFC 9208 section 5.1).
const STORAGE_UNIT: u64 = 1024;

/// The QUOTAROOT line and a QUOTA line per root.
const QUOTA_LINES: usize = MAX_QUOTA_ROOTS + 1;

const ROOT_NAME: &str = "a quota root name cannot be kept";
const TOO_MANY_ROOTS: &str = "the answer names too many quota roots";
const PAST_BOUND: &str = "a quota value passes its bound";

/// What one answer named so far: the roots of the inbox and the
/// resources of each root a QUOTA line described.
#[derive(Debug, Default)]
struct Answered {
    roots: Option<Vec<String>>,
    quotas: Vec<QuotaRoot>,
}

/// The quota roots of the inbox in the order the server names them; a
/// root without a QUOTA line has neither resource.
pub(super) async fn quota_root(
    selection: &mut Selection<'_>,
    room: Room,
) -> Result<Vec<QuotaRoot>, SessionError> {
    let mut answered = Answered::default();
    selection
        .collect("GETQUOTAROOT INBOX", room.bounds(QUOTA_LINES), |response| {
            answered.note(response)
        })
        .await?;
    answered.roots()
}

impl Answered {
    /// Keeps what one line says; a QUOTAROOT line for another mailbox
    /// and every other line pass by.
    fn note(&mut self, response: &Response<'_>) -> Result<(), SessionError> {
        match response {
            Response::QuotaRoot(found) if found.mailbox_name.eq_ignore_ascii_case("INBOX") => {
                if found.quota_root_names.len() > MAX_QUOTA_ROOTS {
                    return Err(SessionError::Protocol(TOO_MANY_ROOTS));
                }
                let names = found.quota_root_names.iter().map(|name| kept(name));
                let names = names.collect::<Result<Vec<_>, _>>()?;
                self.roots.get_or_insert(names);
            }
            Response::Quota(quota) => {
                if self.quotas.len() == MAX_QUOTA_ROOTS {
                    return Err(SessionError::Protocol(TOO_MANY_ROOTS));
                }
                self.quotas.push(described(quota)?);
            }
            _ => {}
        }
        Ok(())
    }

    /// Every root of the QUOTAROOT line with what its QUOTA line said.
    fn roots(self) -> Result<Vec<QuotaRoot>, SessionError> {
        let names = self.roots.ok_or(SessionError::Protocol(
            "GETQUOTAROOT answered without QUOTAROOT",
        ))?;
        let quotas = self.quotas;
        Ok(names
            .into_iter()
            .map(|name| {
                let found = quotas.iter().find(|quota| quota.name == name);
                found.cloned().unwrap_or(QuotaRoot {
                    name,
                    storage: None,
                    messages: None,
                })
            })
            .collect())
    }
}

/// A root name the bridge keeps: one it could send back on a command
/// line.
fn kept(name: &str) -> Result<String, SessionError> {
    wire_name(name).ok_or(SessionError::Protocol(ROOT_NAME))
}

/// One QUOTA line: STORAGE in octets, MESSAGE as it is, any other
/// resource passed by. Of a resource named twice the first counts.
fn described(quota: &imap_proto::Quota<'_>) -> Result<QuotaRoot, SessionError> {
    let mut root = QuotaRoot {
        name: kept(&quota.root_name)?,
        storage: None,
        messages: None,
    };
    for resource in &quota.resources {
        let (held, unit) = match resource.name {
            QuotaResourceName::Storage => (&mut root.storage, STORAGE_UNIT),
            QuotaResourceName::Message => (&mut root.messages, 1),
            QuotaResourceName::Atom(_) => continue,
        };
        let usage = Usage {
            used: value(resource.usage, unit)?,
            limit: value(resource.limit, unit)?,
        };
        held.get_or_insert(usage);
    }
    Ok(root)
}

/// A value of a QUOTA line in the bridge's unit. RFC 9208 section 7
/// gives usage and limit as `number64`, an unsigned 63-bit integer
/// (RFC 9051 section 9); in octets it may not pass 64 bits.
fn value(found: u64, unit: u64) -> Result<u64, SessionError> {
    i64::try_from(found)
        .ok()
        .and_then(|_| found.checked_mul(unit))
        .ok_or(SessionError::Protocol(PAST_BOUND))
}

/// One line through the quota reader, for the fuzz targets.
#[cfg(feature = "test-support")]
pub(super) fn line(bytes: &[u8]) {
    if let Ok((_, response)) = Response::from_bytes(bytes) {
        let _ = Answered::default().note(&response);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What an answer of these lines reads as.
    fn read(lines: &[&str]) -> Result<Vec<QuotaRoot>, SessionError> {
        let mut answered = Answered::default();
        for line in lines {
            let response = Response::from_bytes(line.as_bytes()).unwrap().1;
            answered.note(&response)?;
        }
        answered.roots()
    }

    #[test]
    fn storage_reads_in_octets_and_the_message_count_as_it_is_rfc9208_5() {
        let roots = read(&[
            "* QUOTAROOT INBOX \"User quota\"\r\n",
            "* QUOTA \"User quota\" (STORAGE 420 1024 MESSAGE 17 1000 MAILBOX 3 50)\r\n",
        ])
        .unwrap();
        assert_eq!(
            roots,
            [QuotaRoot {
                name: "User quota".to_owned(),
                storage: Some(Usage {
                    used: 420 * 1024,
                    limit: 1024 * 1024
                }),
                messages: Some(Usage {
                    used: 17,
                    limit: 1000
                }),
            }]
        );
    }

    #[test]
    fn a_root_without_a_quota_line_and_a_mailbox_under_none_rfc9208_4_2_2() {
        let roots = read(&[
            "* QUOTAROOT INBOX \"\" user\r\n",
            "* QUOTA \"\" (STORAGE 1 10)\r\n",
            "* QUOTA other (STORAGE 2 20)\r\n",
        ])
        .unwrap();
        let names: Vec<&str> = roots.iter().map(|root| root.name.as_str()).collect();
        assert_eq!(names, ["", "user"]);
        assert_eq!(roots[0].storage.map(|usage| usage.used), Some(1024));
        assert_eq!(roots[1].storage, None);
        assert_eq!(read(&["* QUOTAROOT INBOX\r\n"]).unwrap(), []);
        assert_eq!(read(&["* QUOTAROOT inbox x\r\n"]).unwrap().len(), 1);
    }

    #[test]
    fn a_value_past_its_bound_is_a_protocol_failure_rfc9208_7() {
        let root = "* QUOTAROOT INBOX \"\"\r\n";
        // 2^63, then 2^54 units of 1024 octets, which pass 64 bits.
        for resources in [
            "STORAGE 1 9223372036854775808",
            "STORAGE 18014398509481984 1",
        ] {
            let line = format!("* QUOTA \"\" ({resources})\r\n");
            assert!(
                matches!(
                    read(&[root, &line]),
                    Err(SessionError::Protocol(PAST_BOUND))
                ),
                "{resources}"
            );
        }
        assert!(read(&[root, "* QUOTA \"\" (MESSAGE 9223372036854775807 1)\r\n"]).is_ok());
    }

    #[test]
    fn a_root_name_that_cannot_be_kept_and_too_many_roots_fail() {
        let long = "r".repeat(crate::session::MAX_WIRE_NAME_BYTES + 1);
        let literal = format!("* QUOTAROOT INBOX {{{}}}\r\n{long}\r\n", long.len());
        assert!(matches!(
            read(&[&literal]),
            Err(SessionError::Protocol(ROOT_NAME))
        ));
        let names = vec!["r"; MAX_QUOTA_ROOTS + 1].join(" ");
        let many = format!("* QUOTAROOT INBOX {names}\r\n");
        assert!(matches!(
            read(&[&many]),
            Err(SessionError::Protocol(TOO_MANY_ROOTS))
        ));
        assert!(matches!(
            read(&["* QUOTA \"\" (STORAGE 1 2)\r\n"]),
            Err(SessionError::Protocol(_))
        ));
    }
}
