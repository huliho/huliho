// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What one write did to each object, folded so the log gets one row per
//! object and sequence (RFC 8620 section 5.2).

use std::collections::BTreeMap;

use rusqlite::Transaction;

use super::changes::{ChangeKind, ObjectType, log};
use super::{AccountKey, StoreError};

/// The changes of one transaction by object.
#[derive(Debug, Default)]
pub(super) struct Ledger {
    entries: BTreeMap<(&'static str, String), (ObjectType, ChangeKind)>,
}

impl Ledger {
    /// Notes one change. An object created and destroyed inside the
    /// write never existed for a client and leaves the ledger; one a
    /// client held that is created again reads as updated.
    pub(super) fn note(&mut self, object: ObjectType, id: &str, kind: ChangeKind) {
        let slot = (object.as_str(), id.to_owned());
        let folded = match (self.entries.get(&slot).map(|(_, before)| *before), kind) {
            (Some(ChangeKind::Created), ChangeKind::Destroyed) => None,
            (Some(ChangeKind::Created), _) => Some(ChangeKind::Created),
            (Some(ChangeKind::Destroyed | ChangeKind::Updated), ChangeKind::Created) => {
                Some(ChangeKind::Updated)
            }
            (Some(ChangeKind::Destroyed), _) => Some(ChangeKind::Destroyed),
            (_, kind) => Some(kind),
        };
        match folded {
            Some(kind) => self.entries.insert(slot, (object, kind)),
            None => self.entries.remove(&slot),
        };
    }

    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Writes every entry under `sequence`.
    pub(super) fn write(
        &self,
        transaction: &Transaction<'_>,
        key: &AccountKey,
        sequence: u64,
    ) -> Result<(), StoreError> {
        for ((_, id), (object, kind)) in &self.entries {
            log(transaction, key, (sequence, *object), (id, *kind))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(ledger: &Ledger) -> Vec<(&str, ChangeKind)> {
        ledger
            .entries
            .iter()
            .map(|((_, id), (_, kind))| (id.as_str(), *kind))
            .collect()
    }

    #[test]
    fn an_object_keeps_one_entry_per_write_rfc8620_5_2() {
        let mut ledger = Ledger::default();
        ledger.note(ObjectType::Thread, "born", ChangeKind::Created);
        ledger.note(ObjectType::Thread, "born", ChangeKind::Updated);
        ledger.note(ObjectType::Thread, "brief", ChangeKind::Created);
        ledger.note(ObjectType::Thread, "brief", ChangeKind::Destroyed);
        ledger.note(ObjectType::Thread, "gone", ChangeKind::Updated);
        ledger.note(ObjectType::Thread, "gone", ChangeKind::Destroyed);
        ledger.note(ObjectType::Thread, "back", ChangeKind::Destroyed);
        ledger.note(ObjectType::Thread, "back", ChangeKind::Created);
        ledger.note(ObjectType::Thread, "held", ChangeKind::Updated);
        ledger.note(ObjectType::Thread, "held", ChangeKind::Created);
        assert_eq!(
            kinds(&ledger),
            [
                ("back", ChangeKind::Updated),
                ("born", ChangeKind::Created),
                ("gone", ChangeKind::Destroyed),
                ("held", ChangeKind::Updated),
            ]
        );
    }

    #[test]
    fn two_types_may_share_an_id() {
        let mut ledger = Ledger::default();
        ledger.note(ObjectType::Thread, "x", ChangeKind::Created);
        ledger.note(ObjectType::Email, "x", ChangeKind::Destroyed);
        assert_eq!(ledger.entries.len(), 2);
        assert!(!ledger.is_empty());
    }
}
