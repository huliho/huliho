// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! What a second client changes, each seen through the `/changes`
//! calls: a delivery, a flag, a label, a move to Spam and a deletion.

use super::super::corpus::{Change, DELIVERED, LABEL, SEEDED};
use super::Scenario;

impl Scenario {
    /// Every change in order.
    pub(super) async fn changes_seen(&mut self) {
        self.delivery().await;
        self.flag().await;
        self.label().await;
        self.spam().await;
        self.deletion().await;
    }

    async fn delivery(&mut self) {
        let before = self.suite.state();
        self.edit(Change::Deliver(DELIVERED)).await;
        let changed = self
            .changes_showing("Email", before, |answer| {
                !Self::listed(answer, "created").is_empty()
            })
            .await;
        let created = Self::listed(&changed, "created");
        assert_eq!(created.len(), 1, "{changed}");
        assert_eq!(Self::listed(&changed, "destroyed").len(), 0);
        let arrived = self
            .emails(&created, &["messageId", "mailboxIds"])
            .await
            .remove(0);
        assert_eq!(Self::number_of(&arrived), Some(DELIVERED));
        let inbox = self.suite.by_role("inbox").id.to_string();
        assert!(Self::mailbox_ids(&arrived).contains(&inbox));
        assert_eq!(
            Self::listed(&self.changes("Thread", before).await, "created").len(),
            1
        );
        self.ids.insert(DELIVERED, created[0].clone());
    }

    async fn flag(&mut self) {
        let before = self.suite.state();
        self.edit(Change::Flag(1, "\\Flagged")).await;
        let id = self.ids[&1].clone();
        let changed = self
            .changes_showing("Email", before, |answer| {
                Self::names(answer, "updated", &id)
            })
            .await;
        assert!(Self::names(&changed, "updated", &id), "{changed}");
        let flagged = self.email(1, &["keywords", "mailboxIds"]).await;
        assert_eq!(flagged["keywords"]["$flagged"], true);
        let starred = self.suite.by_role("flagged").id.to_string();
        assert!(Self::mailbox_ids(&flagged).contains(&starred), "{flagged}");
    }

    async fn label(&mut self) {
        let before = self.suite.state();
        self.edit(Change::Label(2)).await;
        let id = self.ids[&2].clone();
        let changed = self
            .changes_showing("Email", before, |answer| {
                Self::names(answer, "updated", &id)
            })
            .await;
        assert!(Self::names(&changed, "updated", &id), "{changed}");
        let project = self.suite.by_name(LABEL).id.to_string();
        let labeled = self.email(2, &["mailboxIds"]).await;
        assert!(Self::mailbox_ids(&labeled).contains(&project), "{labeled}");
        let mailboxes = self.changes("Mailbox", before).await;
        assert!(
            Self::listed(&mailboxes, "updated").contains(&project),
            "{mailboxes}"
        );
    }

    async fn spam(&mut self) {
        let before = self.suite.state();
        self.edit(Change::MoveToSpam(3)).await;
        let id = self.ids[&3].clone();
        let changed = self
            .changes_showing("Email", before, |answer| {
                Self::names(answer, "updated", &id)
            })
            .await;
        assert!(Self::names(&changed, "updated", &id), "{changed}");
        assert_eq!(Self::listed(&changed, "destroyed").len(), 0, "{changed}");
        let junk = self.suite.by_role("junk").id.to_string();
        let moved = self.email(3, &["mailboxIds"]).await;
        assert_eq!(Self::mailbox_ids(&moved), [junk], "{moved}");
    }

    async fn deletion(&mut self) {
        let before = self.suite.state();
        self.edit(Change::DeleteForever(SEEDED)).await;
        let id = self.ids[&SEEDED].clone();
        let changed = self
            .changes_showing("Email", before, |answer| {
                Self::names(answer, "destroyed", &id)
            })
            .await;
        assert!(Self::names(&changed, "destroyed", &id), "{changed}");
    }
}
