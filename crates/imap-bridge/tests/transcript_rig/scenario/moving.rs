// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The steps that move mail from the first client: archive as the Inbox
//! label taken off, a move to the user label with the message marked
//! unread, not spam as a move into All Mail with the Inbox label, spam
//! and trash as moves between the stores, delete forever in Trash and
//! mark all as read in the label through a query of what is unread. A
//! second client sees each where one plays.

use huliho_imap_bridge::testing::mailboxes::{ALL_MAIL, SPAM, TRASH};
use serde_json::{Value, json};

use super::super::ACCOUNT;
use super::super::corpus::{self, DELIVERED, LABEL, LONG};
use super::super::gmail::{self, SETTLE, Stores};
use super::{LOOKS, Scenario, Second};

/// The messages the steps move, by number.
const ARCHIVED: u32 = LONG;
const LABELED: u32 = DELIVERED;
const RESTORED: u32 = 3;
const JUNKED: u32 = 1;
const TRASHED: u32 = 2;

/// The Inbox label as the server spells it.
const INBOX_LABEL: &str = "\\Inbox";

/// One of the three stores.
#[derive(Debug, Clone, Copy)]
enum Store {
    AllMail,
    Spam,
    Trash,
}

impl Store {
    /// Its name in the scripted server's model.
    fn modeled(self) -> &'static str {
        match self {
            Self::AllMail => ALL_MAIL,
            Self::Spam => SPAM,
            Self::Trash => TRASH,
        }
    }

    /// Its name on the live account.
    fn live(self, stores: &Stores) -> &str {
        match self {
            Self::AllMail => &stores.all_mail,
            Self::Spam => &stores.spam,
            Self::Trash => &stores.trash,
        }
    }
}

impl Scenario {
    /// The id of the mailbox with that role.
    fn role_id(&self, role: &str) -> String {
        self.suite.by_role(role).id.to_string()
    }

    /// One `Email/set` of a patch for the message with that number; the
    /// update stands.
    async fn set_one(&self, number: u32, patch: Value) {
        let id = self.ids[&number].clone();
        let arguments = json!({ "accountId": ACCOUNT, "update": { &id: patch } });
        let answered = self.call("Email/set", arguments).await;
        assert!(answered["updated"].get(&id).is_some(), "{answered}");
    }

    /// A patch that takes the email out of the first mailbox and into
    /// the second.
    fn moving(from: &str, to: &str) -> Value {
        json!({ format!("mailboxIds/{from}"): null, format!("mailboxIds/{to}"): true })
    }

    /// The mailbox ids of the message with that number, sorted.
    async fn mailboxes_of(&self, number: u32) -> Vec<String> {
        let mut ids = Self::mailbox_ids(&self.email(number, &["mailboxIds"]).await);
        ids.sort();
        ids
    }

    /// Whether the second client finds the message in the store, under
    /// the label when one is named, as `present` says; asked again until
    /// it does or the looks run out. Where nobody plays it holds.
    async fn second_finds(
        &mut self,
        number: u32,
        (store, label): (Store, Option<&str>),
        present: bool,
    ) -> bool {
        for _ in 0..LOOKS {
            let found = match &mut self.second {
                Second::Nobody => return true,
                Second::Model(mailboxes) => {
                    corpus::located(mailboxes, number).is_some_and(|(folder, labels)| {
                        folder == store.modeled()
                            && label.is_none_or(|label| labels.iter().any(|held| held == label))
                    })
                }
                Second::Account { editor, stores } => {
                    gmail::holds(editor, store.live(stores), number, label).await
                }
            };
            if found == present {
                return true;
            }
            if matches!(self.second, Second::Account { .. }) {
                tokio::time::sleep(SETTLE).await;
            }
        }
        false
    }

    /// Archive takes the Inbox label off and leaves the message in All
    /// Mail.
    pub(super) async fn archive(&mut self) {
        let (inbox, archive) = (self.role_id("inbox"), self.role_id("archive"));
        self.set_one(ARCHIVED, Self::moving(&inbox, &archive)).await;
        assert_eq!(self.mailboxes_of(ARCHIVED).await, [archive]);
        assert!(
            self.second_finds(ARCHIVED, (Store::AllMail, Some(INBOX_LABEL)), false)
                .await
        );
        assert!(
            self.second_finds(ARCHIVED, (Store::AllMail, None), true)
                .await
        );
    }

    /// A move from the inbox to the user label, the message marked
    /// unread with it.
    pub(super) async fn move_to_label(&mut self) {
        let label = self.suite.by_name(LABEL).id.to_string();
        let mut patch = Self::moving(&self.role_id("inbox"), &label);
        patch["keywords/$seen"] = Value::Null;
        self.set_one(LABELED, patch).await;
        let mut expected = vec![self.role_id("archive"), label];
        expected.sort();
        assert_eq!(self.mailboxes_of(LABELED).await, expected);
        assert!(
            self.second_finds(LABELED, (Store::AllMail, Some(LABEL)), true)
                .await
        );
    }

    /// Not spam takes the message into All Mail under the Inbox label.
    pub(super) async fn not_spam(&mut self) {
        let inbox = self.role_id("inbox");
        self.set_one(RESTORED, Self::moving(&self.role_id("junk"), &inbox))
            .await;
        let mut expected = vec![self.role_id("archive"), inbox];
        expected.sort();
        assert_eq!(self.mailboxes_of(RESTORED).await, expected);
        assert!(
            self.second_finds(RESTORED, (Store::AllMail, Some(INBOX_LABEL)), true)
                .await
        );
    }

    /// Spam is a move into the Spam store with the keyword beside it.
    pub(super) async fn junk(&mut self) {
        let junk = self.role_id("junk");
        let mut patch = Self::moving(&self.role_id("inbox"), &junk);
        patch["keywords/$junk"] = json!(true);
        self.set_one(JUNKED, patch).await;
        assert_eq!(self.mailboxes_of(JUNKED).await, [junk]);
        assert!(self.second_finds(JUNKED, (Store::Spam, None), true).await);
    }

    /// Delete is a move into the Trash store.
    pub(super) async fn trash(&mut self) {
        let trash = self.role_id("trash");
        self.set_one(TRASHED, Self::moving(&self.role_id("inbox"), &trash))
            .await;
        assert_eq!(self.mailboxes_of(TRASHED).await, [trash]);
        assert!(self.second_finds(TRASHED, (Store::Trash, None), true).await);
    }

    /// Delete forever destroys the message in Trash.
    pub(super) async fn delete_forever(&mut self) {
        let id = self.ids[&TRASHED].clone();
        let arguments = json!({ "accountId": ACCOUNT, "destroy": [&id] });
        let answered = self.call("Email/set", arguments).await;
        assert_eq!(answered["destroyed"], json!([&id]), "{answered}");
        assert!(
            self.second_finds(TRASHED, (Store::Trash, None), false)
                .await
        );
    }

    /// The unread mail of the label: the query names it and a set marks
    /// it read, after which the query names nothing.
    pub(super) async fn mark_all_read(&mut self) {
        let label = self.suite.by_name(LABEL).id.to_string();
        let unread = json!({
            "accountId": ACCOUNT,
            "filter": { "inMailbox": label, "notKeyword": "$seen" },
        });
        let found = self.call("Email/query", unread.clone()).await;
        assert_eq!(
            Self::listed(&found, "ids"),
            [self.ids[&LABELED].clone()],
            "{found}"
        );
        self.set_one(LABELED, json!({ "keywords/$seen": true }))
            .await;
        let after = self.call("Email/query", unread).await;
        assert!(Self::listed(&after, "ids").is_empty(), "{after}");
        assert!(self.second_sees_read(LABELED).await);
    }
}
