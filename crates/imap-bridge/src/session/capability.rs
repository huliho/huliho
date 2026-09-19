// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! CAPABILITY in any state, sent under a tag of its own. The client
//! library offers no raw command before the sign-in and its own
//! CAPABILITY holds every line nobody asked for until the tagged answer;
//! read here, each response is dropped before the next one is parsed.

use std::time::Duration;

use async_imap::Connection;
use async_imap::imap_proto::{Capability, Response};
use tokio::io::AsyncWriteExt;
use tokio::time::timeout;

use super::imap::Wire;
use super::read::{Bounds, answer};
use super::{Capabilities, SessionError, io_error};

/// The untagged lines one CAPABILITY answer may carry before its tagged
/// OK; one is the norm, the rest is room for alerts.
const CAPABILITY_LINES: usize = 8;

/// The names one connection may advertise, some four times what the
/// most talkative servers send.
const MAX_CAPABILITIES: usize = 256;

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
/// the line limit and the name limit.
pub(super) async fn capabilities_of<T: Wire>(
    connection: &mut Connection<T>,
    tags: &mut Tags,
    step: Duration,
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
