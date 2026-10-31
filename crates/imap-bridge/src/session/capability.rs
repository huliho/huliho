// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! CAPABILITY in any state, sent under a tag of its own. The client
//! library offers no raw command before the sign-in and its own
//! CAPABILITY holds every line nobody asked for until the tagged answer;
//! read here, each response is dropped before the next one is parsed.

use std::collections::BTreeSet;
use std::time::Duration;

use async_imap::Connection;
use async_imap::imap_proto::{Capability, Response};
use tokio::io::AsyncWriteExt;
use tokio::time::timeout;

use super::imap::Wire;
use super::read::{Bounds, Count, answer};
use super::{SessionError, io_error};

/// The untagged lines one CAPABILITY answer may carry before its tagged
/// OK; one is the norm, the rest is room for alerts.
const CAPABILITY_LINES: usize = 8;

/// The names one connection may advertise, some four times what the
/// most talkative servers send.
const MAX_CAPABILITIES: usize = 256;

/// What the server advertises, compared without regard to case
/// (RFC 9051 section 9, note 1).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Capabilities {
    names: BTreeSet<String>,
}

impl Capabilities {
    /// Whether `name` was advertised, in any case.
    #[must_use]
    pub fn has(&self, name: &str) -> bool {
        self.names.contains(&name.to_ascii_uppercase())
    }

    /// Every advertised name in upper case, sorted.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.names.iter().map(String::as_str)
    }

    /// The commands the server offers for moving and expunging mail.
    #[must_use]
    pub fn moving(&self) -> Moving {
        Moving {
            move_command: self.has("MOVE"),
            uidplus: self.has("UIDPLUS"),
        }
    }

    /// How the server takes the literals of a search and whether it
    /// searches as Gmail does.
    #[must_use]
    pub fn searching(&self) -> Searching {
        let literals = if self.has("LITERAL+") {
            Literals::Plus
        } else if self.has("LITERAL-") {
            Literals::Minus
        } else {
            Literals::Synchronizing
        };
        Searching {
            literals,
            gmail: self.has(crate::gmail::CAPABILITY),
        }
    }

    /// Where the server names the user's rights on a mailbox.
    #[must_use]
    pub fn acl(&self) -> Acl {
        Acl {
            my_rights: self.has("ACL"),
            list_rights: self.has("LIST-MYRIGHTS"),
            current: self.iter().any(|name| name.starts_with("RIGHTS=")),
        }
    }
}

/// What a server says of the user's rights.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Acl {
    /// ACL: MYRIGHTS names them for one mailbox (RFC 4314 section 3.5).
    pub my_rights: bool,
    /// LIST-MYRIGHTS: a LIST with the MYRIGHTS return option names them
    /// for every mailbox (RFC 8440).
    pub list_rights: bool,
    /// `RIGHTS=`: the letters are RFC 4314's; a server without it names
    /// the `c` and `d` of RFC 2086 (RFC 4314 section 2.1.1).
    pub current: bool,
}

/// What a search needs to know of the server.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Searching {
    /// Which literals the server reads without a continuation request.
    pub literals: Literals,
    /// X-GM-RAW and X-GM-LABELS are search keys there.
    pub gmail: bool,
}

/// The literals a server reads without a continuation request (RFC
/// 7888).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Literals {
    /// Every literal waits for the server's continuation request (RFC
    /// 3501 section 4.3).
    #[default]
    Synchronizing,
    /// LITERAL-: one of 4096 bytes at most goes without waiting.
    Minus,
    /// LITERAL+: every literal goes without waiting.
    Plus,
}

/// The longest literal that goes without waiting under LITERAL- (RFC
/// 7888 section 5).
const LITERAL_MINUS_BYTES: usize = 4096;

impl Literals {
    /// Whether a literal of `bytes` goes without waiting for the
    /// server's continuation request (RFC 7888 sections 3 and 5).
    #[must_use]
    pub fn unsynchronized(self, bytes: usize) -> bool {
        match self {
            Self::Plus => true,
            Self::Minus => bytes <= LITERAL_MINUS_BYTES,
            Self::Synchronizing => false,
        }
    }
}

/// How a server moves mail: `UID MOVE` (RFC 6851) or, with UIDPLUS, a
/// copy and a `UID EXPUNGE` of the same set (RFC 4315), which a destroy
/// needs as well.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Moving {
    pub move_command: bool,
    pub uidplus: bool,
}

impl Moving {
    /// Whether the server moves mail at all.
    #[must_use]
    pub fn moves(self) -> bool {
        self.move_command || self.uidplus
    }
}

impl Extend<String> for Capabilities {
    fn extend<I: IntoIterator<Item = String>>(&mut self, names: I) {
        self.names
            .extend(names.into_iter().map(|name| name.to_ascii_uppercase()));
    }
}

impl FromIterator<String> for Capabilities {
    fn from_iter<I: IntoIterator<Item = String>>(names: I) -> Self {
        let mut found = Self::default();
        found.extend(names);
        found
    }
}

/// The tags of the commands sent here, a fresh one per command (RFC 3501
/// section 2.2.1). The client library's own tags open with `A`, so the
/// two never meet.
#[derive(Debug, Default)]
pub(super) struct Tags(u32);

impl Tags {
    fn next(&mut self) -> String {
        self.0 = self.0.wrapping_add(1);
        format!("C{}", self.0)
    }
}

/// Sends CAPABILITY (RFC 9051 section 6.1.1) and reads its answer under
/// the line limit and the name limit. The count of a selected mailbox
/// follows the answer like any other.
pub(super) async fn capabilities_of<T: Wire>(
    connection: &mut Connection<T>,
    tags: &mut Tags,
    step: Duration,
    mut count: Option<&mut Count>,
) -> Result<Capabilities, SessionError> {
    let tag = tags.next();
    let command = format!("{tag} CAPABILITY\r\n");
    let sent = async {
        let stream = connection.get_mut();
        stream.write_all(command.as_bytes()).await?;
        stream.flush().await
    };
    timeout(step, sent)
        .await
        .map_err(|_elapsed| SessionError::Timeout)?
        .map_err(io_error)?;
    let mut found = Capabilities::default();
    let mut advertised = 0;
    let bounds = Bounds {
        step,
        max_lines: CAPABILITY_LINES,
    };
    answer(connection, &tag, bounds, |response| {
        if let Some(count) = count.as_deref_mut() {
            count.note(response);
        }
        if let Response::Capabilities(names) = response {
            advertised += names.len();
            if advertised > MAX_CAPABILITIES {
                return Err(SessionError::Protocol(
                    "the server advertises too many capabilities",
                ));
            }
            found.extend(names.iter().map(name));
        }
        Ok(())
    })
    .await?;
    if found.names.is_empty() {
        return Err(SessionError::Protocol("CAPABILITY answered without data"));
    }
    Ok(found)
}

fn name(capability: &Capability<'_>) -> String {
    match capability {
        Capability::Imap4rev1 => "IMAP4rev1".to_owned(),
        Capability::Auth(mechanism) => format!("AUTH={mechanism}"),
        Capability::Atom(atom) => atom.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn advertised(names: &[&str]) -> Capabilities {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn a_server_moves_mail_with_move_or_uidplus_rfc6851_rfc4315() {
        let both = advertised(&["move", "UIDPLUS"]).moving();
        assert!(both.move_command && both.uidplus && both.moves());
        assert!(advertised(&["MOVE"]).moving().moves());
        let uidplus = advertised(&["UIDPLUS"]).moving();
        assert!(uidplus.moves() && !uidplus.move_command);
        assert!(!advertised(&["IMAP4rev1"]).moving().moves());
    }

    #[test]
    fn a_server_names_the_literals_it_reads_without_waiting_rfc7888_1() {
        let literals = |names: &[&str]| advertised(names).searching().literals;
        assert_eq!(literals(&["literal+", "LITERAL-"]), Literals::Plus);
        assert_eq!(literals(&["LITERAL-"]), Literals::Minus);
        assert_eq!(literals(&["IMAP4rev1"]), Literals::Synchronizing);
        assert!(advertised(&["X-GM-EXT-1"]).searching().gmail);
        assert!(!advertised(&["LITERAL+"]).searching().gmail);
    }

    #[test]
    fn a_server_names_the_rights_with_acl_and_for_a_listing_with_list_myrights_rfc4314_rfc8440() {
        let cyrus = advertised(&["ACL", "RIGHTS=kxten", "list-myrights"]).acl();
        assert!(cyrus.my_rights && cyrus.list_rights && cyrus.current);
        let one_by_one = advertised(&["ACL"]).acl();
        assert!(one_by_one.my_rights && !one_by_one.list_rights && !one_by_one.current);
        assert_eq!(advertised(&["IMAP4rev1"]).acl(), Acl::default());
    }

    #[test]
    fn literal_minus_takes_4096_bytes_without_waiting_and_no_more_rfc7888_5() {
        assert!(Literals::Plus.unsynchronized(1 << 20));
        assert!(Literals::Minus.unsynchronized(LITERAL_MINUS_BYTES));
        assert!(!Literals::Minus.unsynchronized(LITERAL_MINUS_BYTES + 1));
        assert!(!Literals::Synchronizing.unsynchronized(1));
    }

    #[test]
    fn a_capability_matches_without_regard_to_case_rfc9051_9_note_1() {
        let capabilities: Capabilities = ["IMAP4rev2".to_owned(), "auth=xoauth2".to_owned()]
            .into_iter()
            .collect();
        assert!(capabilities.has("imap4rev2"));
        assert!(capabilities.has("AUTH=XOAUTH2"));
        assert!(!capabilities.has("STARTTLS"));
        assert_eq!(
            capabilities.iter().collect::<Vec<_>>(),
            ["AUTH=XOAUTH2", "IMAP4REV2"]
        );
    }
}
