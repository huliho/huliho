// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What a folder command that failed comes to: how its object ends,
//! whether the session stands and what the work does afterwards.

use super::Refusal;
use super::plan::names_of;
use super::step::Naming;
use crate::session::SessionError;
use crate::store::{MailboxId, MailboxRow};

/// Why a step ended early: how its object ends and whether the session
/// still stands.
pub(super) struct Stop {
    pub refusal: Refusal,
    pub stands: bool,
}

/// What a failed command comes to. A NO is the server's refusal and
/// keeps the session, an answer that cannot be read is its failure and
/// costs it, anything else leaves the server unreached.
pub(super) fn failure(error: &SessionError) -> Stop {
    let (refusal, stands) = match error {
        SessionError::Refused => (Refusal::ServerFail, true),
        SessionError::Protocol(_) => (Refusal::ServerFail, false),
        _ => (Refusal::ServerUnavailable, false),
    };
    Stop { refusal, stands }
}

/// A command that tidies up after another: a NO leaves things as they
/// are, any other failure ends the session.
pub(super) fn kept(sent: Result<(), SessionError>) -> Result<(), Stop> {
    match sent {
        Ok(()) | Err(SessionError::Refused) => Ok(()),
        Err(error) => Err(failure(&error)),
    }
}

/// The folder commands the server refused or could not read, by what
/// they ask afterwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Refused {
    Nothing,
    /// A command: the next use signs in afresh.
    Command,
    /// A CREATE, a RENAME or a DELETE among them: a pass shows what
    /// stands.
    Tree,
}

/// A name a CREATE or a RENAME asked for that the server did not give.
pub(super) struct Contested {
    pub index: usize,
    pub naming: Naming,
    /// For a RENAME, the mailbox it moves.
    pub moving: Option<MailboxId>,
}

impl Contested {
    pub(super) fn new(index: usize, naming: &Naming, moving: Option<MailboxId>) -> Self {
        Self {
            index,
            naming: naming.clone(),
            moving,
        }
    }
}

/// The objects whose name stands on the rows a pass left, so another
/// client holds it. A RENAME counts while its mailbox still has its row.
/// A pass matches rows by wire name, so a mailbox that moved has lost it:
/// a server may move a tree one mailbox at a time and stop on the way.
pub(super) fn taken(contested: &[Contested], rows: &[MailboxRow]) -> Vec<usize> {
    let names = names_of(rows);
    let stayed = |id: &MailboxId| rows.iter().any(|row| row.id == *id);
    contested
        .iter()
        .filter(|asked| {
            let name = (asked.naming.under.id().cloned(), asked.naming.leaf.clone());
            names.contains(&name) && asked.moving.as_ref().is_none_or(stayed)
        })
        .map(|asked| asked.index)
        .collect()
}
