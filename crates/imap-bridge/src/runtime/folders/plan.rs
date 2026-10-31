// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! Every object of a `Mailbox/set` checked against the rows before a
//! command: what the account holds, the rights, the roles, the children,
//! the mail and the names beside it (RFC 8620 section 5.3, RFC 8621
//! sections 2 and 2.5). A name is checked for what needs no delimiter
//! here; its wire form waits for the listing. A mailbox the call creates
//! is a row for the objects after it.

mod roles;

use std::collections::{HashMap, HashSet};

use unicode_normalization::UnicodeNormalization as _;

use super::step::{Naming, Planned, Step, Under, creation_order, invalid, name_error};
use super::{Ask, CreatedIds, Refusal, creation_id};
use crate::mailboxes::INBOX;
use crate::mailboxes::rights::{self, Right};
use crate::session::{MailboxName, Place};
use crate::store::{MailboxId, MailboxRow, MailboxSnapshot};

/// The rows as the call leaves them so far.
#[derive(Clone)]
struct Planning<'a> {
    snapshot: &'a MailboxSnapshot,
    rows: HashMap<String, MailboxRow>,
    /// The id each create would take, by creation id.
    created: HashMap<&'a str, MailboxId>,
    /// The creation ids the calls before this one resolved.
    references: &'a CreatedIds,
    /// The names in NFC taken under each parent.
    names: HashSet<(Option<MailboxId>, String)>,
    /// The children each mailbox holds.
    children: HashMap<MailboxId, usize>,
    /// The mailboxes the call destroys, by id.
    destroying: HashSet<String>,
    /// The role each update that lands sets, by the id of the mailbox
    /// it names; `None` while no role is contested.
    roles: Option<HashMap<String, Option<String>>>,
    planned: Planned,
}

/// Every object of the ask checked; a name's wire form, which needs the
/// server's delimiter, is checked when its step runs.
pub(super) fn planned(snapshot: &MailboxSnapshot, ask: &Ask, references: &CreatedIds) -> Planned {
    let mut planning = Planning::new(snapshot, ask, references);
    planning.roles = Some(roles::of_landing_updates(&planning, ask));
    planning.run(ask);
    planning.planned
}

fn nfc(name: &str) -> String {
    name.nfc().collect()
}

fn is_inbox(row: &MailboxRow) -> bool {
    row.facts.imap_name.eq_ignore_ascii_case(INBOX)
}

impl<'a> Planning<'a> {
    fn new(snapshot: &'a MailboxSnapshot, ask: &'a Ask, references: &'a CreatedIds) -> Self {
        let created: HashMap<&str, MailboxId> = ask
            .creates
            .iter()
            .map(|create| (create.creation_id.as_str(), MailboxId::generate()))
            .collect();
        let mut planning = Self {
            snapshot,
            rows: snapshot
                .rows
                .iter()
                .map(|row| (row.id.to_string(), row.clone()))
                .collect(),
            names: snapshot
                .rows
                .iter()
                .map(|row| (row.parent_id.clone(), nfc(&row.facts.name)))
                .collect(),
            children: HashMap::new(),
            destroying: HashSet::new(),
            roles: None,
            created,
            references,
            planned: Planned::default(),
        };
        planning.destroying = ask
            .destroys
            .iter()
            .map(|id| planning.resolved(id))
            .collect();
        for parent in snapshot.rows.iter().filter_map(|row| row.parent_id.clone()) {
            *planning.children.entry(parent).or_default() += 1;
        }
        planning
    }

    /// The creates in the order their references need, then the updates,
    /// then the destroys, each seeing what the ones before it did.
    fn run(&mut self, ask: &Ask) {
        for index in creation_order(&ask.creates) {
            let create = &ask.creates[index];
            let id = self.created[create.creation_id.as_str()].clone();
            let step = self.create(index, id, create);
            self.settle(index, step);
        }
        let after_creates = ask.creates.len();
        for (offset, update) in ask.updates.iter().enumerate() {
            let index = after_creates + offset;
            let step = self.update(index, update);
            self.settle(index, step);
        }
        let after_updates = after_creates + ask.updates.len();
        for (offset, id) in ask.destroys.iter().enumerate() {
            let index = after_updates + offset;
            let step = self.destroy(index, id, ask.remove_emails);
            self.settle(index, step);
        }
    }

    fn settle(&mut self, index: usize, step: Result<Step, Refusal>) {
        match step {
            Ok(step) => self.planned.steps.push(step),
            Err(refusal) => self.planned.refused.push((index, refusal)),
        }
    }

    fn row(&self, id: &str) -> Option<&MailboxRow> {
        self.rows.get(id)
    }

    fn may(&self, row: &MailboxRow, right: Right) -> bool {
        rights::of(row, self.snapshot).may(right)
    }

    /// An id as written, or the id a creation id stands for: a create
    /// of this call first, then one of an earlier call of the request
    /// (RFC 8620 section 5.3). One that stands for nothing stays as
    /// written and names no row.
    fn resolved(&self, id: &str) -> String {
        let Some(creation) = creation_id(id) else {
            return id.to_owned();
        };
        let own = self.created.get(creation).map(MailboxId::to_string);
        own.or_else(|| self.references.get(creation).cloned())
            .unwrap_or_else(|| id.to_owned())
    }

    /// The parent a create or a move names: a mailbox or the top level.
    /// One that names nothing is invalid (RFC 8620 section 5.3); one
    /// that takes no child is forbidden.
    fn under(&self, parent: Option<&str>) -> Result<Under, Refusal> {
        let Some(parent) = parent else {
            return Ok(Under::Top);
        };
        let parent = self.resolved(parent);
        let row = self.row(&parent).ok_or_else(|| invalid("parentId"))?;
        if !self.may(row, Right::CreateChild) {
            return Err(Refusal::Forbidden);
        }
        Ok(Under::Mailbox(row.id.clone()))
    }

    /// The name as far as it reads without the server's delimiter, free
    /// under its parent; INBOX at the top is taken in any case.
    fn name(&self, typed: &str, under: &Under) -> Result<String, Refusal> {
        let anywhere = Place {
            parent: None,
            delimiter: None,
        };
        let leaf = MailboxName::new(typed, anywhere)
            .map_err(name_error)?
            .leaf()
            .to_owned();
        let parent = under.id().cloned();
        let inbox = parent.is_none() && leaf.eq_ignore_ascii_case(INBOX);
        if inbox || self.names.contains(&(parent, leaf.clone())) {
            return Err(invalid("name"));
        }
        Ok(leaf)
    }

    /// A create: its row joins the rows for the objects after it, with
    /// the wire name left for the work.
    fn create(
        &mut self,
        index: usize,
        id: MailboxId,
        create: &super::Create,
    ) -> Result<Step, Refusal> {
        let under = self.under(create.parent.as_deref())?;
        let leaf = self.name(&create.name, &under)?;
        let role = match &create.role {
            Some(role) => Some(self.role(role.as_deref(), None)?),
            None => None,
        };
        let parent = under.id().cloned();
        self.names.insert((parent.clone(), leaf.clone()));
        if let Some(parent) = parent {
            *self.children.entry(parent).or_default() += 1;
        }
        let step = Step::Create {
            index,
            id,
            naming: Naming {
                typed: create.name.clone(),
                leaf,
                under,
            },
            role,
            subscribe: create.subscribed.unwrap_or(true),
        };
        let subscribed = create.subscribed.unwrap_or(true);
        if let Some(row) = step.made("", subscribed, self.snapshot.gmail) {
            self.rows.insert(row.id.to_string(), row);
        }
        Ok(step)
    }

    fn update(&mut self, index: usize, update: &super::Update) -> Result<Step, Refusal> {
        let id = self.resolved(&update.id);
        let row = self.row(&id).ok_or(Refusal::NotFound)?.clone();
        if self.destroying.contains(&id) {
            return Err(Refusal::WillDestroy);
        }
        let role = match &update.role {
            Some(role) => Some(self.role(role.as_deref(), Some(&row))?),
            None => None,
        };
        // A role mailbox stays subscribed: the role is the one the call
        // leaves it with.
        let held = match &role {
            Some(role) => role.role().is_some(),
            None => row.facts.role.is_some(),
        };
        if update.subscribed == Some(false) && (held || is_inbox(&row)) {
            return Err(Refusal::Forbidden);
        }
        let rename = self.rename(&row, update)?;
        let followers = if rename.is_some() {
            self.followers(&row)
        } else {
            Vec::new()
        };
        Ok(Step::Update {
            index,
            id: row.id.clone(),
            rename,
            followers,
            subscribe: update
                .subscribed
                .filter(|wanted| *wanted != row.facts.subscribed),
            role,
        })
    }

    /// A new name or parent: the mailbox must take a rename and its new
    /// parent a child (RFC 4314 section 4). The parent may not lie under
    /// the mailbox; the name must be free there. `None` when nothing
    /// moves.
    fn rename(
        &mut self,
        row: &MailboxRow,
        update: &super::Update,
    ) -> Result<Option<Naming>, Refusal> {
        let typed = update.name.as_deref().unwrap_or(&row.facts.name);
        let parent = match &update.parent {
            Some(parent) => parent.as_deref(),
            None => row.parent_id.as_ref().map(MailboxId::as_str),
        };
        let unmoved = parent == row.parent_id.as_ref().map(MailboxId::as_str);
        if unmoved && nfc(typed) == nfc(&row.facts.name) {
            return Ok(None);
        }
        if !self.may(row, Right::Rename) {
            return Err(Refusal::Forbidden);
        }
        let under = self.under(parent)?;
        if under
            .id()
            .is_some_and(|parent| self.lies_under(parent, &row.id))
        {
            return Err(invalid("parentId"));
        }
        let own = (row.parent_id.clone(), nfc(&row.facts.name));
        self.names.remove(&own);
        let leaf = match self.name(typed, &under) {
            Ok(leaf) => leaf,
            Err(refusal) => {
                self.names.insert(own);
                return Err(refusal);
            }
        };
        let parent = under.id().cloned();
        self.names.insert((parent.clone(), leaf.clone()));
        if !unmoved {
            self.leave_parent(row.parent_id.as_ref());
            if let Some(parent) = parent {
                *self.children.entry(parent).or_default() += 1;
            }
        }
        Ok(Some(Naming {
            typed: typed.to_owned(),
            leaf,
            under,
        }))
    }

    /// The subscribed mailboxes among the mailbox and every one under
    /// it, the mailbox first and the rest by depth.
    fn followers(&self, row: &MailboxRow) -> Vec<MailboxId> {
        let mut found: Vec<(usize, MailboxId)> = self
            .rows
            .values()
            .filter(|other| other.facts.subscribed && self.lies_under(&other.id, &row.id))
            .map(|other| (self.depth(&other.id), other.id.clone()))
            .collect();
        found.sort();
        found.into_iter().map(|(_, id)| id).collect()
    }

    /// How many parents stand above the mailbox.
    fn depth(&self, id: &MailboxId) -> usize {
        let mut depth = 0;
        let mut current = self.row(id.as_str()).and_then(|row| row.parent_id.clone());
        while let Some(parent) = current.filter(|_| depth < self.rows.len()) {
            depth += 1;
            current = self
                .row(parent.as_str())
                .and_then(|row| row.parent_id.clone());
        }
        depth
    }

    /// Whether `id` is `ancestor` or lies anywhere under it.
    fn lies_under(&self, id: &MailboxId, ancestor: &MailboxId) -> bool {
        let mut current = Some(id.clone());
        let mut steps = self.rows.len();
        while let Some(found) = current {
            if found == *ancestor {
                return true;
            }
            if steps == 0 {
                return false;
            }
            steps -= 1;
            current = self
                .row(found.as_str())
                .and_then(|row| row.parent_id.clone());
        }
        false
    }

    fn leave_parent(&mut self, parent: Option<&MailboxId>) {
        if let Some(count) = parent.and_then(|parent| self.children.get_mut(parent)) {
            *count = count.saturating_sub(1);
        }
    }

    /// A destroy: the mailbox must take one and hold no child. Unless
    /// the call removes its emails it must hold no email either (RFC
    /// 8621 section 2.5).
    fn destroy(&mut self, index: usize, id: &str, remove_emails: bool) -> Result<Step, Refusal> {
        let id = self.resolved(id);
        let row = self.row(&id).ok_or(Refusal::NotFound)?.clone();
        if !self.may(&row, Right::Delete) {
            return Err(Refusal::Forbidden);
        }
        if self.children.get(&row.id).copied().unwrap_or_default() > 0 {
            return Err(Refusal::MailboxHasChild);
        }
        if !remove_emails && self.emails(&row) > 0 {
            return Err(Refusal::MailboxHasEmail);
        }
        self.leave_parent(row.parent_id.as_ref());
        Ok(Step::Destroy {
            index,
            id: row.id,
            subscribed: row.facts.subscribed,
        })
    }

    /// The emails as `Mailbox/get` counts them.
    fn emails(&self, row: &MailboxRow) -> u32 {
        if self.snapshot.done.contains(&row.id) {
            let counts = self.snapshot.counts.get(&row.id);
            counts.map_or(0, |counts| counts.synced_emails)
        } else {
            row.facts.total_emails
        }
    }
}

#[cfg(test)]
mod tests;
