// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The search steps: the seeded word over every mailbox but Spam and
//! Trash, under the inbox's label and over the whole account, a repeat
//! the kept hits answer with no SEARCH, the sender over the default
//! scope and the unread mail with no SEARCH at all. Behind a transcript
//! a SEARCH the recording lacks is a mismatch, so the replay proves the
//! repeat and the unread mail cost none.

use serde_json::{Value, json};

use super::super::ACCOUNT;
use super::super::corpus::{SEARCH_WORD, SEARCHED, SEEDED};
use super::Scenario;

impl Scenario {
    /// The ids of the mailboxes with those roles.
    fn roles(&self, roles: &[&str]) -> Vec<String> {
        roles
            .iter()
            .map(|role| self.suite.by_role(role).id.to_string())
            .collect()
    }

    /// The ids `Email/query` answers for the filter, newest first.
    async fn searched(&self, filter: Value) -> Vec<String> {
        let arguments = json!({ "accountId": ACCOUNT, "filter": filter, "calculateTotal": true });
        let answer = self.call("Email/query", arguments).await;
        assert!(answer["ids"].is_array(), "{answer}");
        Self::listed(&answer, "ids")
    }

    /// The ids of the seeded messages with those numbers, in that order.
    fn seeded(&self, numbers: &[u32]) -> Vec<String> {
        numbers
            .iter()
            .map(|number| self.ids[number].clone())
            .collect()
    }

    pub(super) async fn search(&mut self) {
        let bins = self.roles(&["junk", "trash"]);
        let word = json!({ "inMailboxOtherThan": bins, "text": SEARCH_WORD });
        let only = self.seeded(&[SEARCHED]);
        assert_eq!(self.searched(word.clone()).await, only);
        let inbox = self.suite.by_role("inbox").id.to_string();
        let labeled = json!({ "inMailbox": inbox, "text": SEARCH_WORD });
        assert_eq!(self.searched(labeled).await, only);
        let everywhere = json!({ "text": SEARCH_WORD });
        assert_eq!(self.searched(everywhere).await, only);
        assert_eq!(self.searched(word).await, only, "the kept hits");
        let sender = json!({ "inMailboxOtherThan": bins, "from": "Sanne" });
        let newest_first: Vec<u32> = (1..=SEEDED).rev().collect();
        assert_eq!(self.searched(sender).await, self.seeded(&newest_first));
        // The live account may hold unread mail of its own.
        let unread = json!({ "inMailboxOtherThan": bins, "notKeyword": "$seen" });
        let mut found = self.searched(unread).await;
        found.retain(|id| self.ids.values().any(|seeded| seeded == id));
        assert_eq!(found, self.seeded(&[SEEDED]));
    }
}
