// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The labels of a move a Gmail account makes into All Mail. Per target
//! a SELECT, the labels of each patch stored, then the labels every
//! message carries read back, since it regains the ones it had.
//! Whatever comes of that, the rows move where the messages went.

use std::collections::BTreeMap;

use super::super::set::SetError;
use super::moves::{Relabel, relabeled};
use super::{Member, Work, failure};
use crate::session::{FlagFetch, MAX_STORE_UIDS, Session, SessionError, UidRange};
use crate::store::{MailboxId, MoveChange};
use crate::sync::mapping;

/// A message a move took to another folder: into All Mail its labels are
/// stored there next.
pub(super) struct Restore {
    pub(super) member: Member,
    pub(super) from: MailboxId,
    pub(super) to: MailboxId,
    pub(super) uid_validity: u32,
    pub(super) uid: u32,
    pub(super) relabel: Relabel,
}

impl Restore {
    /// The row's move, with the labels read where the message went.
    pub(super) fn change(&self, labels: Option<Vec<String>>) -> MoveChange {
        MoveChange {
            id: self.member.id.clone(),
            from: self.from.clone(),
            from_uid: self.member.uid,
            to: self.to.clone(),
            uid_validity: self.uid_validity,
            uid: self.uid,
            labels,
        }
    }
}

/// The moves into All Mail, per target: a SELECT, the labels of each
/// patch stored, then the labels every message carries read back.
/// Whether the session still stands.
pub(super) async fn restored<S: Session>(session: &mut S, work: &mut Work) -> bool {
    let mut by_target: BTreeMap<MailboxId, Vec<Restore>> = BTreeMap::new();
    for restore in std::mem::take(&mut work.restores) {
        by_target
            .entry(restore.to.clone())
            .or_default()
            .push(restore);
    }
    for (to, restores) in by_target {
        let (error, stands) = match labels_in(session, work, &to, &restores).await {
            Ok(Some(read)) => {
                for restore in &restores {
                    work.done(&restore.member);
                    let labels = read.get(&restore.uid).cloned();
                    work.stored.moves.push(restore.change(labels));
                }
                continue;
            }
            // The target cannot take its labels now: nothing was stored
            // there and the session stands.
            Ok(None) => (SetError::ServerFail, true),
            Err(error) => failure(&error),
        };
        for restore in &restores {
            work.settle(std::slice::from_ref(&restore.member), &error);
            work.stored.moves.push(restore.change(None));
        }
        if !stands {
            return false;
        }
    }
    true
}

/// The labels of the restores stored in the target and read back, by
/// new UID in the bridge's spelling. `None` when the target cannot take
/// them now and no STORE goes there: a `[READ-ONLY]` open takes none
/// and one under another UIDVALIDITY than the move's `COPYUID` named
/// holds nothing under the new UIDs. Every target is a key of the rows,
/// so the name lookup is a guard alone.
async fn labels_in<S: Session>(
    session: &mut S,
    work: &Work,
    to: &MailboxId,
    restores: &[Restore],
) -> Result<Option<BTreeMap<u32, Vec<String>>>, SessionError> {
    let Some(name) = work.wire_name(to) else {
        return Ok(None);
    };
    let writable = session.select(name).await?;
    let renumbered = restores
        .iter()
        .any(|restore| restore.uid_validity != writable.uid_validity);
    if writable.read_only || renumbered {
        return Ok(None);
    }
    let mut groups: Vec<(&Relabel, Vec<u32>)> = Vec::new();
    for restore in restores {
        match groups
            .iter_mut()
            .find(|(relabel, _)| **relabel == restore.relabel)
        {
            Some((_, uids)) => uids.push(restore.uid),
            None => groups.push((&restore.relabel, vec![restore.uid])),
        }
    }
    for (relabel, uids) in &groups {
        for uids in uids.chunks(MAX_STORE_UIDS) {
            relabeled(session, work, relabel, uids).await?;
        }
    }
    let uids: Vec<u32> = restores.iter().map(|restore| restore.uid).collect();
    let (Some(low), Some(high)) = (uids.iter().min(), uids.iter().max()) else {
        return Ok(Some(BTreeMap::new()));
    };
    let range = UidRange {
        low: *low,
        high: *high,
    };
    let flagged = session.uid_flags(FlagFetch::Range(range), true).await?;
    let read = flagged
        .into_iter()
        .filter(|found| uids.contains(&found.uid))
        .filter_map(|found| Some((found.uid, mapping::labels(&found.labels?))))
        .collect();
    Ok(Some(read))
}
