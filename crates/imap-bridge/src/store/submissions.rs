// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The submissions this process sent, the last few per account, kept in
//! memory: RFC 8621 section 7 lets a basic SMTP proxy drop a record once
//! the message went. Every send moves the state.

use std::collections::VecDeque;

use super::memory::InMemory;
use super::{AccountKey, EmailId, Store, SubmissionId, ThreadId};

/// The submissions kept per account; `EmailSubmission/get` answers
/// `notFound` for an older one.
pub const RECENT_SUBMISSIONS: usize = 32;

/// One submission that went out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmissionRow {
    pub id: SubmissionId,
    pub identity_id: String,
    pub email_id: EmailId,
    pub thread_id: ThreadId,
    /// The sender as it went on the wire: the identity's address.
    pub mail_from: String,
    /// The recipients as they went on the wire, each as the message
    /// spells it.
    pub rcpt_to: Vec<String>,
    /// When the message went, in seconds since the epoch.
    pub send_at: i64,
}

/// The recent submissions of an account and the state they belong to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmissionSnapshot {
    pub state: String,
    /// Oldest first.
    pub rows: Vec<SubmissionRow>,
}

/// The record of every account and the mark of this store.
pub(super) type Submissions = InMemory<Kept>;

/// The record of one account.
#[derive(Debug, Default)]
pub(super) struct Kept {
    counter: u64,
    rows: VecDeque<SubmissionRow>,
}

impl Store {
    /// The recent submissions of the account.
    #[must_use]
    pub fn submissions(&self, key: &AccountKey) -> SubmissionSnapshot {
        let accounts = self.submissions.lock();
        let (counter, rows) = accounts.get(key).map_or((0, Vec::new()), |kept| {
            (kept.counter, kept.rows.iter().cloned().collect())
        });
        SubmissionSnapshot {
            state: self.submissions.state(counter),
            rows,
        }
    }

    /// Notes a submission that went out; the oldest leaves past the bound.
    /// The state afterwards. A forgotten account keeps nothing.
    pub fn record_submission(&self, key: &AccountKey, row: SubmissionRow) -> String {
        if self.is_forgotten(key) {
            return self.submissions(key).state;
        }
        let mut accounts = self.submissions.lock();
        let kept = accounts.entry(key.clone()).or_default();
        kept.counter += 1;
        kept.rows.push_back(row);
        while kept.rows.len() > RECENT_SUBMISSIONS {
            kept.rows.pop_front();
        }
        let counter = kept.counter;
        drop(accounts);
        self.submissions.state(counter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row() -> SubmissionRow {
        SubmissionRow {
            id: SubmissionId::generate(),
            identity_id: "a1".to_owned(),
            email_id: EmailId::generate(),
            thread_id: ThreadId::generate(),
            mail_from: "sanne@example.test".to_owned(),
            rcpt_to: vec!["mo@example.test".to_owned()],
            send_at: 0,
        }
    }

    #[test]
    fn the_record_keeps_the_newest_and_moves_its_state_with_each_send_rfc8621_7() {
        let store = Store::in_memory().unwrap();
        let key = AccountKey::new("a1");
        let empty = store.submissions(&key);
        assert_eq!(empty.rows, []);
        let rows: Vec<SubmissionRow> = (0..=RECENT_SUBMISSIONS).map(|_| row()).collect();
        let mut states = vec![empty.state];
        for sent in &rows {
            states.push(store.record_submission(&key, sent.clone()));
        }
        let kept = store.submissions(&key);
        assert_eq!(kept.rows, rows[1..]);
        assert_eq!(kept.state, *states.last().unwrap());
        states.dedup();
        assert_eq!(states.len(), RECENT_SUBMISSIONS + 2);
        store.forget(&key);
        assert_eq!(store.submissions(&key).rows, []);
        store.record_submission(&key, row());
        assert_eq!(store.submissions(&key).rows, []);
    }

    #[test]
    fn another_store_never_issues_the_same_state() {
        let key = AccountKey::new("a1");
        let first = Store::in_memory().unwrap().submissions(&key).state;
        let second = Store::in_memory().unwrap().submissions(&key).state;
        assert_ne!(first, second);
    }
}
