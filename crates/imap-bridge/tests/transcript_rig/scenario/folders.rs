// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The folder steps of the first client: a label created through
//! `Mailbox/set`, a message added to it, the label renamed with the
//! message under its new name, then deleted with the message kept in
//! All Mail.

use serde_json::{Value, json};

use super::super::ACCOUNT;
use super::super::corpus::{FOLDER, FOLDER_RENAMED, LONG};
use super::Scenario;

/// The message the steps put under the label: archived by then, so it
/// lies in All Mail alone.
const FILED: u32 = LONG;

impl Scenario {
    /// Every folder step in order.
    pub(super) async fn folders(&mut self) {
        let label = self.created_label().await;
        let message = self.ids[&FILED].clone();
        let add = json!({ "accountId": ACCOUNT, "update": { (&message): { (format!("mailboxIds/{label}")): true } } });
        let added = self.call("Email/set", add).await;
        assert!(added["updated"].get(&message).is_some(), "{added}");
        let rename =
            json!({ "accountId": ACCOUNT, "update": { (&label): { "name": FOLDER_RENAMED } } });
        let renamed = self.call("Mailbox/set", rename).await;
        assert_eq!(renamed["updated"][&label], Value::Null, "{renamed}");
        let shown = json!({ "accountId": ACCOUNT, "ids": [&label], "properties": ["name"] });
        let shown = self.call("Mailbox/get", shown).await;
        assert_eq!(shown["list"][0]["name"], FOLDER_RENAMED);
        let filed = self.email(FILED, &["mailboxIds"]).await;
        assert!(Self::mailbox_ids(&filed).contains(&label), "{filed}");
        let destroy =
            json!({ "accountId": ACCOUNT, "destroy": [&label], "onDestroyRemoveEmails": true });
        let gone = self.call("Mailbox/set", destroy).await;
        assert_eq!(gone["destroyed"], json!([label]), "{gone}");
        let kept = Self::mailbox_ids(&self.email(FILED, &["mailboxIds"]).await);
        assert!(!kept.contains(&label));
        assert!(kept.contains(&self.suite.by_role("archive").id.to_string()));
    }

    /// The label made through `Mailbox/set`, subscribed and at the top.
    async fn created_label(&mut self) -> String {
        let create = json!({ "accountId": ACCOUNT, "create": { "f": { "name": FOLDER } } });
        let created = self.call("Mailbox/set", create).await;
        let object = &created["created"]["f"];
        assert_eq!(object["isSubscribed"], true, "{created}");
        assert_eq!(object["parentId"], Value::Null);
        assert_eq!(object["myRights"]["mayRename"], true);
        object["id"].as_str().unwrap().to_owned()
    }
}
