// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The roles of a `Mailbox/set` (RFC 8621 section 2): each held by one
//! mailbox once the call is done, where only an object that lands holds
//! or frees one.

use std::collections::{HashMap, HashSet};

use super::{Planning, is_inbox};
use crate::mailboxes::ROLES;
use crate::runtime::folders::step::{Step, invalid};
use crate::runtime::folders::{Ask, Refusal};
use crate::store::{MailboxId, MailboxRow, RoleOverride};

const INBOX_ROLE: &str = "inbox";

/// The role each update that lands sets, by the id of the mailbox it
/// names. A run of the fresh plan with no role contested tells which
/// updates pass their other checks; one refused there neither holds nor
/// frees a role. Against the map a create naming a role an update sets
/// is refused, and so are two updates that name one role.
pub(super) fn of_landing_updates(
    fresh: &Planning<'_>,
    ask: &Ask,
) -> HashMap<String, Option<String>> {
    let mut run = fresh.clone();
    run.roles = None;
    run.run(ask);
    let passed: HashSet<usize> = run.planned.steps.iter().map(Step::index).collect();
    ask.updates
        .iter()
        .enumerate()
        .filter_map(|(offset, update)| {
            let role = update.role.as_ref()?.as_deref().map(str::to_owned);
            let index = ask.creates.len() + offset;
            passed
                .contains(&index)
                .then(|| (run.resolved(&update.id), role))
        })
        .collect()
}

impl Planning<'_> {
    /// Whether a mailbox other than `own` holds the role once every role
    /// the call sets is in place (RFC 8621 section 2): the rows, the
    /// creates that passed and the landing updates that name one of
    /// those. Nothing is held while no role is contested.
    fn held_elsewhere(&self, role: &str, own: Option<&MailboxId>) -> bool {
        let Some(roles) = &self.roles else {
            return false;
        };
        self.rows
            .values()
            .filter(|other| Some(&other.id) != own)
            .any(|other| {
                let set = roles.get(other.id.as_str());
                set.map_or(other.facts.role.as_deref(), Option::as_deref) == Some(role)
            })
    }

    pub(super) fn role(
        &self,
        role: Option<&str>,
        row: Option<&MailboxRow>,
    ) -> Result<RoleOverride, Refusal> {
        if self.snapshot.gmail || row.is_some_and(is_inbox) || role == Some(INBOX_ROLE) {
            return Err(Refusal::Forbidden);
        }
        let Some(role) = role else {
            return Ok(RoleOverride::NoRole);
        };
        let known = ROLES.contains(&role);
        let selectable = row.is_none_or(|row| row.facts.selectable);
        if !known || !selectable || self.held_elsewhere(role, row.map(|row| &row.id)) {
            return Err(invalid("role"));
        }
        Ok(RoleOverride::Role(role.to_owned()))
    }
}
