// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The steps of the Gmail scenario suite: the mailbox list with its
//! roles and the quota the same pass read, the first sync of the stores,
//! an inbox window, thread grouping, the properties and previews of the
//! seeded mail, the searches, a message
//! opened with its body and its blobs and marked read, then what a
//! second client changes: a delivery, a flag, a label, a move to Spam
//! and a deletion; then the moves of the first client. Every assertion
//! holds against the live account, the scripted server and a transcript
//! of either.

mod changing;
mod moving;
mod quota;
mod reading;
mod searching;

use std::collections::HashMap;

use huliho_imap_bridge::store::MailboxRow;
use huliho_imap_bridge::sync::Step;
use huliho_imap_bridge::sync::preview::PREVIEW_CHARS;
use huliho_imap_bridge::testing::Mailboxes;
use huliho_imap_bridge::testing::record::fixed::{FIXED_DOMAIN, fill};
use serde_json::{Value, json};

use super::corpus::{self, Change, LABEL, LONG, SEEDED, seed};
use super::gmail::{self, Editor, SETTLE, Stores};
use super::{ACCOUNT, Suite};

/// The inbox window the suite asks for.
const WINDOW: usize = 3;

/// How many times a change the second client made is looked for before
/// the suite gives up; on the live account each look waits `SETTLE`.
const LOOKS: usize = 15;

/// Enough of a folder to find the seeded mail among what else is there.
const LISTING: usize = 50;

/// The roles a Gmail account shows.
const ROLES: [&str; 8] = [
    "inbox",
    "archive",
    "junk",
    "trash",
    "sent",
    "drafts",
    "flagged",
    "important",
];

/// Who plays the second client.
pub enum Second {
    Nobody,
    Model(Mailboxes),
    Account { editor: Box<Editor>, stores: Stores },
}

/// One run of the suite: the bridge, the second client and what the
/// steps learned about ids.
pub struct Scenario {
    suite: Suite,
    second: Second,
    /// The email id of every seeded message by number.
    ids: HashMap<u32, String>,
}

impl Scenario {
    pub fn new(suite: Suite, second: Second) -> Self {
        Self {
            suite,
            second,
            ids: HashMap::new(),
        }
    }

    /// Every step in order, the account cleared afterwards when it is
    /// the live one, then the suite's own end.
    pub async fn run(mut self) {
        self.reads().await;
        self.changes_seen().await;
        self.moves().await;
        if let Second::Account { editor, stores } = &mut self.second {
            gmail::clear(editor, stores).await;
            gmail::within(editor.logout()).await.unwrap();
        }
        self.suite.finish();
    }

    /// The steps over the account as the first sync leaves it.
    async fn reads(&mut self) {
        self.list_and_roles().await;
        self.sync_the_stores().await;
        self.inbox_window().await;
        self.threads().await;
        self.properties().await;
        self.previews().await;
        self.search().await;
        self.body().await;
        self.download().await;
        self.mark_read().await;
    }

    /// The moves of the first client.
    async fn moves(&mut self) {
        self.archive().await;
        self.move_to_label().await;
        self.not_spam().await;
        self.junk().await;
        self.trash().await;
        self.delete_forever().await;
        self.mark_all_read().await;
    }

    async fn edit(&mut self, change: Change) {
        match &mut self.second {
            Second::Nobody => {}
            Second::Model(mailboxes) => corpus::apply_to_model(mailboxes, change),
            Second::Account { editor, stores } => gmail::apply(editor, stores, change).await,
        }
    }

    async fn call(&self, method: &str, arguments: Value) -> Value {
        self.suite.call(&json!([method, arguments, "c1"])).await
    }

    async fn changes(&self, object: &str, since: u64) -> Value {
        let arguments = json!({ "accountId": ACCOUNT, "sinceState": since.to_string() });
        self.call(&format!("{object}/changes"), arguments).await
    }

    /// `<Type>/changes` since a state, asked again until the answer
    /// shows what the second client did or the looks run out; behind a
    /// transcript every look plays as it was recorded.
    async fn changes_showing(
        &self,
        object: &str,
        since: u64,
        shows: impl Fn(&Value) -> bool,
    ) -> Value {
        let mut answer = self.changes(object, since).await;
        for _ in 1..LOOKS {
            if shows(&answer) {
                break;
            }
            if matches!(self.second, Second::Account { .. }) {
                tokio::time::sleep(SETTLE).await;
            }
            answer = self.changes(object, since).await;
        }
        answer
    }

    /// Whether a `/changes` answer names the id in that list.
    fn names(answer: &Value, list: &str, id: &str) -> bool {
        Self::listed(answer, list).iter().any(|found| found == id)
    }

    /// `Email/get` of these ids with the named properties.
    async fn emails(&self, ids: &[String], properties: &[&str]) -> Vec<Value> {
        let arguments = json!({ "accountId": ACCOUNT, "ids": ids, "properties": properties });
        self.call("Email/get", arguments).await["list"]
            .as_array()
            .unwrap()
            .clone()
    }

    /// One seeded message with the named properties.
    async fn email(&self, number: u32, properties: &[&str]) -> Value {
        let id = self.ids[&number].clone();
        self.emails(&[id], properties).await.remove(0)
    }

    /// The ids of a mailbox by `receivedAt`, newest first.
    async fn query(&self, mailbox: &MailboxRow, limit: usize, collapse: bool) -> Value {
        let arguments = json!({
            "accountId": ACCOUNT,
            "filter": { "inMailbox": mailbox.id },
            "sort": [{ "property": "receivedAt", "isAscending": false }],
            "limit": limit,
            "calculateTotal": true,
            "collapseThreads": collapse,
        });
        self.call("Email/query", arguments).await
    }

    /// The corpus number of an email, read from its Message-ID, which
    /// the redaction leaves as written; `None` for any other mail.
    fn number_of(email: &Value) -> Option<u32> {
        let id = email["messageId"][0].as_str()?;
        id.trim_matches(['<', '>'])
            .strip_prefix('m')?
            .strip_suffix(&format!("@{FIXED_DOMAIN}"))?
            .parse()
            .ok()
    }

    /// The mailbox ids an email is a member of.
    fn mailbox_ids(email: &Value) -> Vec<String> {
        email["mailboxIds"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect()
    }

    fn listed(answer: &Value, list: &str) -> Vec<String> {
        answer[list]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| id.as_str().unwrap().to_owned())
            .collect()
    }

    /// A body text as this run sees it: the seeded words, or filler of
    /// their length behind a transcript.
    fn expect_text(&self, preview: &str, body: &str) {
        if self.suite.is_replay() {
            assert_eq!(preview, fill(preview.len()));
        } else {
            assert_eq!(preview, body.trim_end());
        }
    }

    async fn list_and_roles(&mut self) {
        self.suite.pass().await;
        let rows = self.suite.rows();
        for role in ROLES {
            let row = self.suite.by_role(role);
            assert_eq!(
                row.facts.store,
                matches!(role, "archive" | "junk" | "trash"),
                "{role}"
            );
        }
        let label = self.suite.by_name(LABEL);
        assert!(label.facts.role.is_none() && !label.facts.store && label.facts.selectable);
        let listed = self
            .call("Mailbox/get", json!({ "accountId": ACCOUNT, "ids": null }))
            .await;
        assert_eq!(listed["list"].as_array().unwrap().len(), rows.len());
        self.quota().await;
    }

    async fn sync_the_stores(&mut self) {
        for row in self.suite.rows().into_iter().filter(|row| row.facts.store) {
            assert_eq!(
                self.suite.sync(&row).await,
                Step::Done,
                "{}",
                row.facts.imap_name
            );
        }
    }

    async fn inbox_window(&mut self) {
        let inbox = self.suite.by_role("inbox");
        let window = self.query(&inbox, WINDOW, false).await;
        let ids = Self::listed(&window, "ids");
        assert_eq!(ids.len(), WINDOW);
        assert!(
            window["total"].as_u64().unwrap() >= u64::from(SEEDED),
            "{window}"
        );
        let newest: Vec<Option<u32>> = self
            .emails(&ids, &["messageId"])
            .await
            .iter()
            .map(Self::number_of)
            .collect();
        let expected: Vec<Option<u32>> = (0..WINDOW)
            .map(|offset| Some(SEEDED - u32::try_from(offset).unwrap()))
            .collect();
        assert_eq!(newest, expected);
        let all_mail = self.suite.by_role("archive");
        let listing = Self::listed(&self.query(&all_mail, LISTING, false).await, "ids");
        for email in self.emails(&listing, &["id", "messageId"]).await {
            if let Some(number) = Self::number_of(&email).filter(|number| *number <= SEEDED) {
                self.ids
                    .insert(number, email["id"].as_str().unwrap().to_owned());
            }
        }
        assert_eq!(
            self.ids.len(),
            usize::try_from(SEEDED).unwrap(),
            "{:?}",
            self.ids
        );
    }

    async fn threads(&mut self) {
        let thread = |email: Value| email["threadId"].as_str().unwrap().to_owned();
        let first = thread(self.email(1, &["threadId"]).await);
        assert_eq!(thread(self.email(2, &["threadId"]).await), first);
        assert_ne!(thread(self.email(3, &["threadId"]).await), first);
        let found = self
            .call(
                "Thread/get",
                json!({ "accountId": ACCOUNT, "ids": [first] }),
            )
            .await;
        let members = Self::listed(&found["list"][0], "emailIds");
        assert_eq!(members.len(), 2, "{found}");
        assert!(members.contains(&self.ids[&1]) && members.contains(&self.ids[&2]));
        let inbox = self.suite.by_role("inbox");
        let collapsed = Self::listed(&self.query(&inbox, LISTING, true).await, "ids");
        let of_thread = collapsed
            .iter()
            .filter(|id| **id == self.ids[&1] || **id == self.ids[&2])
            .count();
        assert_eq!(of_thread, 1, "one email per thread");
    }

    async fn properties(&mut self) {
        let properties = ["subject", "from", "keywords", "hasAttachment", "size"];
        let first = self.email(1, &properties).await;
        assert_eq!(first["subject"], seed(1).subject());
        assert_eq!(first["hasAttachment"], false);
        assert_eq!(first["keywords"]["$seen"], true);
        let sender = first["from"][0]["email"].as_str().unwrap();
        assert!(sender.ends_with(&format!("@{FIXED_DOMAIN}")), "{sender}");
        assert!(first["size"].as_u64().unwrap() > 0);
        let reply = self.email(2, &properties).await;
        assert_eq!(reply["subject"], seed(2).subject());
        let attached = self.email(3, &properties).await;
        assert_eq!(attached["hasAttachment"], true);
        let unseen = self.email(SEEDED, &properties).await;
        assert_eq!(unseen["keywords"].get("$seen"), None);
    }

    async fn previews(&mut self) {
        let short = self.email(3, &["preview"]).await;
        self.expect_text(short["preview"].as_str().unwrap(), &seed(3).body);
        let long = self.email(LONG, &["preview"]).await;
        let preview = long["preview"].as_str().unwrap();
        assert_eq!(preview.chars().count(), PREVIEW_CHARS);
        self.expect_text(preview, &seed(LONG).body[..PREVIEW_CHARS]);
    }
}
