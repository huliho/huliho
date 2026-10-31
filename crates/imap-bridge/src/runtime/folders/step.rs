// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What the check of a `Mailbox/set` hands the work: one step per object
//! that passed, in the order they run.

use std::collections::HashSet;

use super::{Create, Refusal, creation_id};
use crate::mailboxes::sort_order;
use crate::session::NameError;
use crate::store::{MailboxFacts, MailboxId, MailboxRow, RoleOverride};

/// The creates in an order where each comes after the create its parent
/// names by creation id (RFC 8620 section 5.3); a create whose
/// reference never resolves comes last and fails on it.
pub(super) fn creation_order(creates: &[Create]) -> Vec<usize> {
    let mut placed: HashSet<&str> = HashSet::new();
    let mut order = Vec::with_capacity(creates.len());
    let mut left: Vec<usize> = (0..creates.len()).collect();
    while !left.is_empty() {
        let before = left.len();
        left.retain(|&index| {
            let create = &creates[index];
            if waits_on(create, creates).is_some_and(|parent| !placed.contains(parent)) {
                return true;
            }
            placed.insert(&create.creation_id);
            order.push(index);
            false
        });
        if left.len() == before {
            break;
        }
    }
    order.extend(left);
    order
}

/// The creation id of the same call a create's parent names, if any.
fn waits_on<'a>(create: &'a Create, creates: &[Create]) -> Option<&'a str> {
    let creation = creation_id(create.parent.as_deref()?)?;
    let known = creates.iter().any(|other| other.creation_id == creation);
    known.then_some(creation)
}

/// Where a name goes: at the top level or under a mailbox, which is a
/// row or one the same call creates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Under {
    Top,
    Mailbox(MailboxId),
}

impl Under {
    pub(super) fn id(&self) -> Option<&MailboxId> {
        match self {
            Self::Top => None,
            Self::Mailbox(id) => Some(id),
        }
    }
}

/// A name the user typed with its leaf in NFC and where it goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Naming {
    pub typed: String,
    pub leaf: String,
    pub under: Under,
}

/// What one object asks of the server once it passed every check. A
/// mailbox is named by id: the work spells it as the server holds it
/// by then.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Step {
    Create {
        index: usize,
        id: MailboxId,
        naming: Naming,
        role: Option<RoleOverride>,
        subscribe: bool,
    },
    Update {
        index: usize,
        id: MailboxId,
        rename: Option<Naming>,
        /// The subscribed mailboxes a rename carries, the mailbox first:
        /// a RENAME leaves their subscriptions on the old names (RFC
        /// 9051 section 6.3.6).
        followers: Vec<MailboxId>,
        subscribe: Option<bool>,
        role: Option<RoleOverride>,
    },
    Destroy {
        index: usize,
        id: MailboxId,
        /// Whether the name is subscribed, which a server keeps after a
        /// DELETE (RFC 9051 section 6.3.7).
        subscribed: bool,
    },
}

impl Step {
    /// Where the object stands in the ask.
    pub(super) fn index(&self) -> usize {
        match self {
            Self::Create { index, .. }
            | Self::Update { index, .. }
            | Self::Destroy { index, .. } => *index,
        }
    }

    /// The mailbox the step names; for a create, the id it takes.
    pub(super) fn id(&self) -> &MailboxId {
        match self {
            Self::Create { id, .. } | Self::Update { id, .. } | Self::Destroy { id, .. } => id,
        }
    }

    /// Whether the step sends nothing: an update of the role alone,
    /// which the rows keep, or of what the mailbox holds already.
    pub(super) fn is_local(&self) -> bool {
        matches!(
            self,
            Self::Update {
                rename: None,
                subscribe: None,
                ..
            }
        )
    }

    /// The row of the mailbox a create makes, under its parent with the
    /// role the user set, `wire` as the server spells the name and
    /// `subscribed` as it took it; a Gmail account shows it as a label.
    /// `None` for any other step.
    pub(super) fn made(&self, wire: &str, subscribed: bool, gmail: bool) -> Option<MailboxRow> {
        let Self::Create {
            id, naming, role, ..
        } = self
        else {
            return None;
        };
        let role = role
            .as_ref()
            .and_then(RoleOverride::role)
            .map(str::to_owned);
        Some(MailboxRow {
            id: id.clone(),
            parent_id: naming.under.id().cloned(),
            facts: MailboxFacts {
                name: naming.leaf.clone(),
                imap_name: wire.to_owned(),
                parent_imap_name: None,
                sort_order: sort_order(role.as_deref()),
                role,
                subscribed,
                selectable: true,
                store: !gmail,
                gmail_label: gmail.then(|| wire.to_owned()),
                uid_validity: None,
                uid_next: None,
                highest_modseq: None,
                total_emails: 0,
                unread_emails: 0,
            },
        })
    }
}

/// The steps in the order they run and the objects refused already, by
/// their place among the creates, then the updates, then the destroys.
#[derive(Debug, Clone, Default)]
pub(super) struct Planned {
    pub steps: Vec<Step>,
    pub refused: Vec<(usize, Refusal)>,
}

pub(super) fn invalid(property: &'static str) -> Refusal {
    Refusal::InvalidProperties(vec![property])
}

/// What a name the server would refuse answers: a parent on a server
/// without hierarchy is the parent's fault, any other case the name's.
pub(super) fn name_error(error: NameError) -> Refusal {
    match error {
        NameError::Flat => invalid("parentId"),
        NameError::Empty
        | NameError::TooLong
        | NameError::Control
        | NameError::Delimiter
        | NameError::EdgeSpace => invalid("name"),
    }
}

/// The sort order a mailbox takes under the role the user set.
pub(super) fn sort_order_of(role: &RoleOverride) -> u32 {
    sort_order(role.role())
}
